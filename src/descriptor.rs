//! Dimension descriptor: names, counts, types from the header block.

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashSet;

use crate::bytes::{latin1, u16, u32};
use crate::error::{Context, IvtError};

static SLUG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[^A-Za-z0-9]+").unwrap());

#[derive(Debug, Clone)]
pub struct DimDesc {
    pub name: String,
    pub count: i64,
    pub type_code: i64,
    #[allow(dead_code)]
    pub double01: bool,
}

#[derive(Debug, Clone)]
pub struct Descriptor {
    #[allow(dead_code)]
    pub offset: u32,
    pub dims: Vec<DimDesc>,
    /// Full English and French axis titles, when the file stores them.
    pub titles: Vec<[Option<String>; 2]>,
    pub geo_dim: Option<usize>,
    pub member_slots: Vec<Option<Vec<usize>>>,
    pub allocation_bits: Option<Vec<u16>>,
}

/// Only the native master axis matrix is authoritative. Missing or malformed
/// matrices are rejected; scanning prose for plausible dimensions is unsafe.
pub fn parse_descriptor(data: &[u8]) -> Result<Descriptor, IvtError> {
    structured_descriptor(data)
}

const NO_FALLBACK: &str =
    "native axis matrix is missing or invalid; unverified descriptor fallback is disabled";
const MATRIX_ROW: usize = 82;

/// An envelope-valid file whose axis matrix this reader does not recognise.
fn unrecognised(data: &[u8], reason: String) -> IvtError {
    let version = data.first().copied().unwrap_or(0);
    IvtError::unsupported(
        format!("descriptor layout not recognised (container version {version}): {reason}"),
        NO_FALLBACK,
    )
}

fn hex_prefix(raw: &[u8]) -> String {
    raw.iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn structured_descriptor(data: &[u8]) -> Result<Descriptor, IvtError> {
    use crate::header::HDR_MASTER_PTR;
    let fail = |reason: String| unrecognised(data, reason);
    let master = crate::header::slot_offset(data, HDR_MASTER_PTR)
        .ok_or_else(|| fail("the header has no master directory slot".into()))?;
    let entries = u32(data, master + 8);
    if entries < 2 {
        return Err(fail(format!(
            "the master directory has {entries} entries, so it has no axis matrix at entry 1"
        )));
    }
    let directory = crate::metadata_directory::MetadataDirectory::new(data, master)?;
    let entry = directory.entry(data, 1)?;
    let (offset, length) = (u32(entry, 0), usize::from(u16(entry, 4)));
    if offset == 0 || length == 0 {
        return Err(fail(format!(
            "master directory entry 1 is empty (offset={offset} length={length})"
        )));
    }
    let raw = (offset as usize)
        .checked_add(length)
        .and_then(|end| data.get(offset as usize..end))
        .ok_or_else(|| {
            IvtError::corrupt(
                format!(
                    "the axis matrix record points outside the file: it needs bytes {offset}..{} but the file is {} bytes",
                    offset as u64 + length as u64,
                    data.len()
                ),
                NO_FALLBACK,
            )
            .with(
                Context::offset(offset)
                    .and_length(length)
                    .and_file_len(data.len())
                    .and_slot(HDR_MASTER_PTR)
                    .and_entry(1),
            )
        })?;
    let decoded = crate::decode::decompress_component(raw).map_err(|error| {
        fail(format!(
            "the axis matrix record could not be decoded ({error}); its first bytes are {}",
            hex_prefix(raw)
        ))
    })?;
    if decoded.len() < 6 {
        return Err(fail(format!(
            "the axis matrix is {} bytes, shorter than its 6-byte heading",
            decoded.len()
        )));
    }
    let (tag, width) = (&decoded[..2], usize::from(u16(&decoded, 4)));
    if tag != [1, 2] || width != MATRIX_ROW {
        return Err(fail(format!(
            "the axis matrix has component header {:02x} {:02x} and {width}-byte rows; supported matrices have header 01 02 (byte rows) and {MATRIX_ROW}-byte rows",
            tag[0], tag[1]
        )));
    }
    let count = u16(&decoded, 2) as usize;
    if !(1..=32).contains(&count) {
        return Err(fail(format!(
            "the axis matrix declares {count} axes; supported tables have 1 to 32"
        )));
    }
    let mut dims = Vec::with_capacity(count);
    let mut allocation_bits = Vec::with_capacity(count);
    for (axis, row) in decoded[6..].as_chunks::<82>().0.iter().enumerate() {
        let items = u32(row, 0);
        let kind = u16(row, 6);
        let bits = u16(row, 4);
        let end = row[12..30].iter().position(|&b| b == 0).unwrap_or(18);
        let name = latin1(&row[12..12 + end]);
        let axis_error = |what: &str| {
            fail(format!(
                "axis {} ({name:?}) {what}: items={items} kind={kind} allocation_bits={bits}",
                axis + 1
            ))
        };
        if items == 0 || items > i32::MAX as u32 {
            return Err(axis_error("has an out-of-range member count"));
        }
        if name.is_empty() {
            return Err(axis_error("has no name"));
        }
        if kind > 2 {
            return Err(axis_error("has an unrecognised axis kind"));
        }
        if data.first() == Some(&2) && (bits > 30 || (1u32 << bits) < items) {
            return Err(axis_error("allocates fewer slots than its member count"));
        }
        allocation_bits.push(bits);
        dims.push(DimDesc {
            name,
            count: items as i64,
            type_code: match kind {
                1 => 0x12,
                2 => 6,
                _ => 0,
            },
            double01: kind == 1,
        });
    }
    if dims.len() != count {
        return Err(fail(format!(
            "the axis matrix declares {count} axes but holds {} complete {MATRIX_ROW}-byte rows",
            dims.len()
        )));
    }
    if dims
        .iter()
        .try_fold(1usize, |n, d| n.checked_mul(d.count as usize))
        .is_none()
    {
        let counts = dims.iter().map(|d| d.count.to_string()).collect::<Vec<_>>();
        return Err(fail(format!(
            "the product of its axis member counts ({}) overflows",
            counts.join(" x ")
        )));
    }
    let geo_dim = geo_dim_index(&dims).or_else(|| {
        (0..dims.len())
            .find(|&axis| crate::members::has_geo_uid_field(data, axis))
            .map(|axis| axis + 1)
    });
    let member_slots = (0..dims.len())
        .map(|axis| {
            if data.first() == Some(&2) {
                crate::members::member_slots(data, axis, dims[axis].count as usize)
                    .map_err(|error| fail(format!("axis {} member slots: {error}", axis + 1)))
            } else {
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let titles = crate::metadata::full_dimension_titles(data, &dims);
    Ok(Descriptor {
        offset,
        titles,
        member_slots,
        allocation_bits: (data.first() == Some(&2)).then_some(allocation_bits),
        dims,
        geo_dim,
    })
}

fn is_geographic_dim(d: &DimDesc) -> bool {
    d.double01 || d.type_code == 0x12
}

pub fn geo_dim_index(dims: &[DimDesc]) -> Option<usize> {
    dims.iter().position(is_geographic_dim).map(|axis| axis + 1)
}

pub fn dim_slugs(names: &[String]) -> Vec<String> {
    let mut used: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for name in names {
        let mut base = SLUG_RE
            .replace_all(name.trim(), "_")
            .trim_matches('_')
            .to_lowercase();
        if base.len() > 40 {
            base.truncate(40);
            // Full titles are long; never leave a dangling separator.
            base.truncate(base.trim_end_matches('_').len());
        }
        if base.is_empty() {
            base = "dim".into();
        }
        let mut slug = base.clone();
        let mut suffix = 2usize;
        while !used.insert(slug.clone()) {
            slug = format!("{base}_{suffix}");
            suffix += 1;
        }
        out.push(slug);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimension_slugs_are_unique_including_existing_suffixes() {
        let names = ["A", "A", "A_2", "A", "東京", "東京", "年月日"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let slugs = dim_slugs(&names);
        assert_eq!(&slugs[..4], ["a", "a_2", "a_2_2", "a_3"]);
        assert!(slugs.iter().all(|slug| !slug.is_empty()));
        assert_eq!(slugs.iter().collect::<HashSet<_>>().len(), slugs.len());
    }

    #[test]
    fn long_titles_truncate_to_forty_without_trailing_separator() {
        let names = [
            "Age (in single years), average age and median age (128)",
            "abcdefghij abcdefghij abcdefghij abcdef (x)",
        ]
        .map(str::to_string);
        assert_eq!(
            dim_slugs(&names),
            [
                "age_in_single_years_average_age_and_medi",
                "abcdefghij_abcdefghij_abcdefghij_abcdef"
            ]
        );
    }

    #[test]
    fn supplied_ivt_dimensions_match_beyond_summary() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let items = Regex::new(r#""name_en":\s*"([^"]+)"[\s\S]*?"items":\s*(\d+)"#).unwrap();
        let mut checked = 0;
        for path in crate::test_data::files() {
            let expected = root
                .join("tests/expected")
                .join(path.file_stem().unwrap())
                .with_extension("json");
            if !expected.exists() {
                continue;
            }
            let json = std::fs::read_to_string(expected).unwrap();
            let dimension_json = json
                .split("\"dimensions\":")
                .nth(1)
                .unwrap()
                .split("\"footnotes\":")
                .next()
                .unwrap();
            let expected = items
                .captures_iter(dimension_json)
                .map(|c| (c[1].to_string(), c[2].parse::<i64>().unwrap()))
                .collect::<Vec<_>>();
            let data = std::fs::read(&path).unwrap();
            let dims = parse_descriptor(&data).unwrap().dims;
            let actual = dims
                .into_iter()
                .map(|d| (d.name, d.count))
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{}", path.display());
            checked += 1;
        }
        assert!(
            !crate::test_data::required() || checked > 0,
            "PYIVT_REQUIRE_DATA=1: no IVT fixtures with descriptor goldens"
        );
    }
}
