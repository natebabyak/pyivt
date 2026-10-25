//! Dimension descriptor: names, counts, types from the header block.

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashSet;

use crate::bytes::{latin1, u16, u32};
use crate::header::{dir_anchor_header, HDR_DESCRIPTOR_PTR};

const GEOTYPES: [u8; 7] = [0x10, 0x0D, 0x0A, 0x0C, 0x09, 0x0F, 0x0B];

static NAME_REST_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(.+?)(?:\s{2,}|\z)").unwrap());
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
}

fn descriptor_name(run: &[u8], first_record: bool) -> Option<String> {
    if run.is_empty() {
        return None;
    }
    // Match Python decode("ascii", errors="ignore")
    let s: String = run
        .iter()
        .copied()
        .filter(|&b| b.is_ascii())
        .map(|b| b as char)
        .collect();
    let n = s.len();
    if n < 2 {
        return None;
    }
    for half in (1..=n / 2).rev() {
        if s[..half] == s[half..2 * half] && 2 * half == n {
            return Some(s[..half].to_string());
        }
        if s[..half] == s[half..2 * half] && !s[..half].is_empty() {
            let rest = &s[2 * half..];
            if rest.is_empty() || !rest.chars().next().unwrap().is_alphanumeric() {
                return Some(s[..half].to_string());
            }
        }
    }
    if n.is_multiple_of(2) && s[..n / 2].eq_ignore_ascii_case(&s[n / 2..]) {
        // Short name and description may differ only in letter case, such
        // as the stored code "TIME PERIOD" and description "Time Period".
        return Some(s[..n / 2].to_string());
    }
    for first_len in (4..=n.min(40)).rev() {
        let first = &s[..first_len];
        if s[first_len..].starts_with(first) {
            let rest = &s[first_len..];
            let cand = NAME_REST_RE
                .captures(rest)
                .map(|c| c.get(1).unwrap().as_str())
                .unwrap_or(rest);
            if cand.len() >= first_len {
                return Some(cand.to_string());
            }
        }
    }
    // Code glued to a spaced description: "RegionReporting Region", "DateMonthly reporting period".
    if let Some(sp) = s.find(' ') {
        let before = &s[..sp];
        let bytes = before.as_bytes();
        for i in (2..before.len().saturating_sub(1)).rev() {
            if bytes[i].is_ascii_uppercase() && bytes[i - 1].is_ascii_lowercase() {
                let code = &before[..i];
                if (2..=24).contains(&code.len()) {
                    return Some(code.to_string());
                }
            }
        }
    }
    if first_record && s.starts_with("Geography") {
        return Some("Geography".into());
    }
    let mid = n / 2;
    let a = &s[..mid];
    let b = &s[mid..];
    if !a.is_empty()
        && !b.is_empty()
        && a.chars().next().unwrap().is_uppercase()
        && b.chars().next().unwrap().is_uppercase()
    {
        return Some(if b.len() >= a.len() {
            b.to_string()
        } else {
            a.to_string()
        });
    }
    let take = s[..n.min(48)].trim();
    if take.is_empty() {
        None
    } else {
        Some(take.to_string())
    }
}

/// Decode the dimension chain carried by a descriptor's `81 01 20 00`
/// record. The per-axis integer following the item count is the number of
/// bits needed for an item *index*, not an axis type. In particular, an
/// index width of 9 does not make the preceding count a geographic ID.
///
/// Unlike the compatibility scanner below, this reader stays inside the
/// declared chain and cannot collect plausible-looking strings from notes.
fn dimension_chain(v: &[u8]) -> Option<Vec<DimDesc>> {
    if v.len() < 18 || !v.starts_with(b"\x81\x01\x20\x00") {
        return None;
    }
    let ndim = u16(v, 16) as usize;
    if !(1..=32).contains(&ndim) {
        return None;
    }
    let mut pos = 22usize.checked_add(ndim.checked_mul(10)?)?;
    let mut dims = Vec::with_capacity(ndim);
    for _ in 0..ndim {
        let (count, index_bits, name_start) = chain_prefix(v, pos)?;
        let printable_end = v[name_start..]
            .iter()
            .position(|&b| b < 32 || b == 127)
            .map(|n| name_start + n)
            .unwrap_or(v.len());
        // The descriptor carries both a short name and its description. A
        // repeated name is self-delimiting even if the next count starts with
        // a printable byte (for example, a count of 90 starts with 'Z').
        let remaining = &v[name_start..printable_end];
        let mut record = None;
        for half in 1..=remaining.len() / 2 {
            if remaining[..half] != remaining[half..2 * half] {
                continue;
            }
            let end = name_start + 2 * half;
            let boundary = if dims.len() + 1 == ndim {
                v.get(end..end + 2) == Some(&b"\x81\x01"[..])
            } else {
                chain_prefix(v, end).is_some()
            };
            if boundary {
                let name = latin1(&remaining[..half]);
                if !name.trim().is_empty() {
                    record = Some((name, end));
                    break;
                }
            }
        }
        let (name, end) = record?;
        dims.push(DimDesc {
            name,
            count,
            type_code: index_bits as i64,
            double01: false,
        });
        pos = end;
    }
    Some(dims)
}

fn chain_prefix(v: &[u8], pos: usize) -> Option<(i64, u8, usize)> {
    // A singleton has no index bits and omits that zero-width integer.
    if v.get(pos..pos + 2) == Some(&b"\x01\x01"[..])
        && v.get(pos + 2).is_some_and(|&b| b > 32 && b != 127)
    {
        return Some((1, 0, pos + 2));
    }
    for width in 1..=4 {
        let bytes = v.get(pos..pos + width)?;
        let index_bits = *v.get(pos + width)?;
        if v.get(pos + width + 1) != Some(&1)
            || !v.get(pos + width + 2).is_some_and(|&b| b > 32 && b != 127)
        {
            continue;
        }
        let count = bytes
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | ((b as u32) << (8 * i)));
        if count > 1
            && index_bits == (32 - (count - 1).leading_zeros()) as u8
            && (width == 1 || bytes[width - 1] != 0)
        {
            return Some((count as i64, index_bits, pos + width + 2));
        }
    }
    None
}

fn walk_records(v: &[u8], lend: usize, cap: usize, lenient: bool) -> Vec<DimDesc> {
    let mut dims = Vec::new();
    let mut k = 3usize;
    while k + 1 < lend && dims.len() < cap {
        let c1 = if k + 1 < v.len() { v[k + 1] } else { 0 };
        let namestart = (65..=90).contains(&c1)
            || (48..=57).contains(&c1)
            || (lenient && (97..=122).contains(&c1));
        let bare02 =
            v[k] == 0x02 && v[k - 1] != 0x01 && (1..=0x10).contains(&v[k - 1]) && v[k - 2] >= 1;
        let anchor_a = (v[k] == 0x01
            || (v[k] == 0x02 && v[k - 1] == 0x01 && (1..0x20).contains(&v[k - 2]))
            || bare02)
            && namestart;
        let count_b = v[k - 2] as i64 + (v[k - 1] as i64) * 256;
        let anchor_b = lenient
            && !anchor_a
            && namestart
            && k >= 3
            && GEOTYPES.contains(&v[k])
            && v[k - 1] > 0
            && count_b <= 200_000;
        if anchor_a || anchor_b {
            let mut e = k + 1;
            while e < v.len() && (32..=126).contains(&v[e]) {
                e += 1;
            }
            let run = &v[k + 1..e];
            if !lenient || run.len() >= 4 {
                // Layout variants before NAME:
                //   [type][count] 01 01 NAME          geographic (People REGION)
                //   [count][type] 01 01 02 NAME       older geographic (Oecd REGION)
                //   [type][count] 01 02 NAME          first-axis (People UNITS)
                //   [count][type] … 02 NAME           bare02 (Oecd ITEM/TIME)
                //   [count][type] 01 NAME             normal
                let geo_double01 = anchor_a && v[k] == 0x01 && v[k - 1] == 0x01;
                let triple010102 =
                    anchor_a && v[k] == 0x02 && v[k - 1] == 0x01 && k >= 4 && v[k - 2] == 0x01;
                let lead_01_02 =
                    anchor_a && v[k] == 0x02 && v[k - 1] == 0x01 && k >= 3 && v[k - 2] != 0x01;
                let (typ, count, is_geo_flag) = if anchor_b {
                    (v[k] as i64, count_b, false)
                } else if triple010102 {
                    (v[k - 3] as i64, v[k - 4] as i64, true)
                } else if geo_double01 {
                    // The count still precedes its index width. A count of
                    // eighteen (0x12) is not a geographic type discriminator.
                    (v[k - 2] as i64, v[k - 3] as i64, true)
                } else if lead_01_02 {
                    (v[k - 3] as i64, v[k - 2] as i64, false)
                } else if bare02 && k >= 3 {
                    // Oecd-style [count][type][flag≤4][02]NAME vs
                    // People/Products [count][type][02]NAME (type often 6).
                    if v[k - 1] <= 0x04 && k >= 4 {
                        (v[k - 2] as i64, v[k - 3] as i64, false)
                    } else {
                        (v[k - 1] as i64, v[k - 2] as i64, false)
                    }
                } else {
                    let typ = v[k - 1] as i64;
                    let count = if GEOTYPES.contains(&v[k - 1]) {
                        v[k - 3] as i64 + (v[k - 2] as i64) * 256
                    } else {
                        v[k - 2] as i64
                    };
                    (typ, count, false)
                };
                let mut nm = descriptor_name(run, dims.is_empty());
                if nm.is_none() && lenient {
                    let s: String = run
                        .iter()
                        .take(24)
                        .map(|&b| b as char)
                        .filter(|c| c.is_ascii())
                        .collect::<String>()
                        .trim()
                        .to_string();
                    if !s.is_empty() {
                        nm = Some(s);
                    }
                }
                if let Some(name) = nm {
                    if (1..=2_000_000).contains(&count) && dims_name_ok(&name) {
                        dims.push(DimDesc {
                            name,
                            count,
                            type_code: typ,
                            double01: is_geo_flag,
                        });
                        k = e;
                        continue;
                    }
                }
            }
        }
        k += 1;
    }
    dims
}

fn dims_name_ok(name: &str) -> bool {
    let t = name.trim();
    if t.len() < 2 || t.len() > 48 {
        return false;
    }
    // Reject prose / notes fragments mistaken for dimension names.
    if t.contains(". ") || t.contains(".\r") || t.contains(".\n") || t.contains(',') {
        return false;
    }
    let words = t.split_whitespace().count();
    if words > 5 {
        return false;
    }
    let lower = t.to_lowercase();
    for bad in [
        "this table",
        "cette table",
        "esta tabla",
        "the data",
        "diskettes",
        "secretariat",
        "economic outlook",
        "demonstration",
        "contient",
        "contains",
        "enregistrements",
        "revenus",
        "ingresos",
    ] {
        if lower.contains(bad) {
            return false;
        }
    }
    true
}

fn dims_quality_score(dims: &[DimDesc]) -> i64 {
    if !dims_quality_ok(dims) {
        return -1;
    }
    // Prefer more axes and a sane cell product.
    let prod = dims.iter().fold(1i64, |a, d| a.saturating_mul(d.count));
    (dims.len() as i64) * 1_000_000 + prod.min(999_999)
}

fn dims_quality_ok(dims: &[DimDesc]) -> bool {
    if dims.len() < 2 || dims.len() > 16 {
        return false;
    }
    let mut prod: i64 = 1;
    for d in dims {
        if !dims_name_ok(&d.name) || d.count < 1 {
            return false;
        }
        prod = prod.saturating_mul(d.count);
        if prod > 500_000_000 {
            return false;
        }
    }
    true
}

fn walk_at(data: &[u8], d_off: usize, cap_hint: usize) -> Option<Vec<DimDesc>> {
    let n = data.len();
    if d_off < 1 || d_off + 18 > n {
        return None;
    }
    let win = (d_off + 4000).min(n);
    let v = &data[d_off..win];
    if let Some(dims) = dimension_chain(v) {
        return Some(dims);
    }
    let text: String = v
        .iter()
        .map(|&b| {
            if (32..=126).contains(&b) {
                b as char
            } else {
                '.'
            }
        })
        .collect();
    // FACET04 usually ends the dim block, but an earlier FACET (title summary)
    // can sit before the name chain in pre-marker windows — ignore those.
    let facet_lend = text.find("FACET04").filter(|&facet| facet >= 200);
    let lend = if let Some(facet) = facet_lend {
        facet
    } else if let Some(anchor) = dir_anchor_header(data) {
        let vb = u32(data, anchor) as usize;
        if vb > d_off + 32 {
            (vb - d_off).min(v.len())
        } else {
            v.len()
        }
    } else {
        v.len()
    };
    let ndim_field = u16(data, d_off + 16);
    let cap = if (1..=32).contains(&ndim_field) {
        ndim_field as usize
    } else if (2..=16).contains(&cap_hint) {
        cap_hint
    } else {
        16
    };
    let try_walk = |end: usize| -> Option<Vec<DimDesc>> {
        let mut dims = walk_records(v, end, cap, false);
        if !dims_quality_ok(&dims) {
            dims = walk_records(v, end, cap, true);
        }
        if dims_quality_ok(&dims) {
            Some(dims)
        } else {
            None
        }
    };
    if let Some(dims) = try_walk(lend) {
        return Some(dims);
    }
    // Retry full window when FACET/dir truncation hid the real chain.
    if lend < v.len() {
        try_walk(v.len())
    } else {
        None
    }
}

/// Demo tables sometimes store the dim-name chain just before an `81 01 20 00`
/// marker while `HDR_DESCRIPTOR_PTR` points at a Timeseries/options blob.
fn scan_dim_marker_windows(data: &[u8]) -> Option<(usize, Vec<DimDesc>)> {
    let needle = b"\x81\x01\x20\x00";
    let mut best: Option<(usize, Vec<DimDesc>, i64)> = None;
    let mut start = 0usize;
    while let Some(rel) = data[start..]
        .windows(needle.len())
        .position(|w| w == needle)
    {
        let marker = start + rel;
        for off in [marker.saturating_sub(256), marker] {
            if let Some(dims) = walk_at(data, off, 16) {
                let score = dims_quality_score(&dims);
                if score >= 0 && best.as_ref().map(|(_, _, s)| *s).unwrap_or(-1) < score {
                    best = Some((off, dims, score));
                }
            }
        }
        start = marker + needle.len();
    }
    best.map(|(o, d, _)| (o, d))
}

pub fn parse_descriptor(data: &[u8]) -> Option<Descriptor> {
    if !data.is_empty() && data[0] == 0x02 {
        return None;
    }
    // Master entry 1 is the authoritative axis matrix. The printable names in
    // its compressed representation are not self-delimiting records.
    if let Some(descriptor) = structured_descriptor(data) {
        return Some(descriptor);
    }
    let d_off = u32(data, HDR_DESCRIPTOR_PTR) as usize;
    if let Some(dims) = data.get(d_off..).and_then(dimension_chain) {
        return Some(Descriptor {
            offset: d_off as u32,
            dims,
        });
    }
    let mut best: Option<(usize, Vec<DimDesc>, i64)> = None;
    let consider =
        |best: &mut Option<(usize, Vec<DimDesc>, i64)>, off: usize, dims: Vec<DimDesc>| {
            let score = dims_quality_score(&dims);
            if score < 0 {
                return;
            }
            if best.as_ref().map(|(_, _, s)| *s).unwrap_or(-1) < score {
                *best = Some((off, dims, score));
            }
        };
    if let Some(dims) = walk_at(data, d_off, 16) {
        consider(&mut best, d_off, dims);
    }
    if let Some((off, dims)) = scan_dim_marker_windows(data) {
        consider(&mut best, off, dims);
    }
    best.map(|(offset, dims, _)| Descriptor {
        offset: offset as u32,
        dims,
    })
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
        if items == 0 || name.is_empty() || kind > 2 {
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
    Some(Descriptor { offset, dims })
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
    // Prefer an explicit Geographic-typed axis (demo REGION) over assuming dim 0.
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

    fn chain(records: &[(&str, u32)]) -> Vec<u8> {
        let mut data = vec![0; 22 + records.len() * 10];
        data[..4].copy_from_slice(b"\x81\x01\x20\x00");
        data[16..18].copy_from_slice(&(records.len() as u16).to_le_bytes());
        for &(name, count) in records {
            if count == 1 {
                data.extend_from_slice(&[1, 1]);
            } else {
                let bytes = count.to_le_bytes();
                let width = ((32 - count.leading_zeros()) as usize).div_ceil(8);
                data.extend_from_slice(&bytes[..width]);
                data.extend_from_slice(&[(32 - (count - 1).leading_zeros()) as u8, 1]);
            }
            data.extend_from_slice(name.as_bytes());
            data.extend_from_slice(name.as_bytes());
        }
        data.extend_from_slice(b"\x81\x01\x04\x03");
        data
    }

    #[test]
    fn declared_chain_includes_singletons_and_printable_counts() {
        let bytes = chain(&[
            ("One", 1),
            ("Places", 90),
            ("Years", 256),
            ("Values", 70_000),
        ]);
        let dims = dimension_chain(&bytes).unwrap();
        assert_eq!(
            dims.iter().map(|d| d.count).collect::<Vec<_>>(),
            [1, 90, 256, 70_000]
        );
        assert_eq!(
            dims.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            ["One", "Places", "Years", "Values"]
        );
        assert!(dims.iter().all(|d| !d.double01));
    }

    #[test]
    fn invalid_chain_is_not_partially_accepted() {
        let mut bytes = chain(&[("First", 174), ("Second", 13)]);
        bytes[22 + 20 + 1] = 7; // 174 needs eight index bits.
        assert!(dimension_chain(&bytes).is_none());
    }

    #[test]
    fn legacy_geographic_count_is_not_a_type_discriminator() {
        let bytes = b"\x00\x04\x12\x05\x01\x01REGIONREGION\x04";
        let dims = walk_records(bytes, bytes.len(), 1, false);
        assert_eq!(dims.len(), 1);
        assert_eq!(dims[0].count, 18);
        assert!(dims[0].double01);
    }

    #[test]
    fn legacy_short_name_preserves_stored_case() {
        assert_eq!(
            descriptor_name(b"TIME PERIODTime Period", false).as_deref(),
            Some("TIME PERIOD")
        );
        assert_eq!(
            descriptor_name(b"REGIONRegion", false).as_deref(),
            Some("REGION")
        );
    }

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
        let Ok(entries) = std::fs::read_dir(root.join("data")) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|ext| ext != "ivt") {
                continue;
            }
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
        if checked == 0 {
            eprintln!("No local IVT fixtures were available for descriptor verification");
        }
    }
}
