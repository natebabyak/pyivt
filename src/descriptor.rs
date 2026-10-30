//! Dimension descriptor: names, counts, types from the header block.

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashSet;

use crate::bytes::{latin1, u16, u32};

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
    pub source: &'static str,
}

/// Only the native master axis matrix is authoritative. Missing or malformed
/// matrices are rejected; scanning prose for plausible dimensions is unsafe.
pub fn parse_descriptor(data: &[u8]) -> Option<Descriptor> {
    structured_descriptor(data)
}

pub fn structured_descriptor(data: &[u8]) -> Option<Descriptor> {
    use crate::header::HDR_MASTER_PTR;
    if data.len() < HDR_MASTER_PTR + 12 || u32(data, HDR_MASTER_PTR + 8) < 2 {
        return None;
    }
    let directory = crate::metadata_directory::MetadataDirectory::new(data, HDR_MASTER_PTR).ok()?;
    let offset = u32(directory.entry(data, 1).ok()?, 0);
    let raw = crate::metadata_directory::record(data, HDR_MASTER_PTR, 1)?;
    let decoded = crate::decode::decompress_component(raw).ok()?;
    if decoded.get(..2) != Some(&[1, 2]) || decoded.len() < 6 || u16(&decoded, 4) != 82 {
        return None;
    }
    let count = u16(&decoded, 2) as usize;
    if !(1..=32).contains(&count) {
        return None;
    }
    let mut dims = Vec::with_capacity(count);
    for row in decoded[6..].as_chunks::<82>().0 {
        let items = u32(row, 0);
        let kind = u16(row, 6);
        let end = row[12..30].iter().position(|&b| b == 0).unwrap_or(18);
        let name = latin1(&row[12..12 + end]);
        if items == 0 || items > i32::MAX as u32 || name.is_empty() || kind > 2 {
            return None;
        }
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
    if dims.len() != count
        || dims
            .iter()
            .try_fold(1usize, |n, d| n.checked_mul(d.count as usize))
            .is_none()
    {
        return None;
    }
    Some(Descriptor {
        offset,
        dims,
        source: "axis_matrix",
    })
}

fn is_geographic_dim(d: &DimDesc) -> bool {
    d.double01 || d.type_code == 0x12
}

pub fn geo_dim_index(dims: &[DimDesc]) -> usize {
    if dims.is_empty() {
        return 1;
    }
    let name = dims[0].name.to_lowercase();
    if dims[0].count == 1 && (name == "values" || name == "value") {
        return dims.len();
    }
    // Prefer the geographic axis kind stored in the native matrix.
    if let Some((i, _)) = dims.iter().enumerate().find(|(_, d)| is_geographic_dim(d)) {
        return i + 1;
    }
    // Census-style tables keep geography first even when Type is Normal.
    if dims[0].name.eq_ignore_ascii_case("Geography")
        || dims[0].name.eq_ignore_ascii_case("Géographie")
    {
        return 1;
    }
    1
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
