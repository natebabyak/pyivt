//! Dimension descriptor: names, counts, types from the header block.

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

use crate::bytes::{u16, u32};
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

fn walk_records(v: &[u8], lend: usize, cap: usize, lenient: bool) -> Vec<DimDesc> {
    let mut dims = Vec::new();
    let mut k = 3usize;
    while k + 1 < lend && dims.len() < cap {
        let c1 = if k + 1 < v.len() { v[k + 1] } else { 0 };
        let namestart = (65..=90).contains(&c1)
            || (48..=57).contains(&c1)
            || (lenient && (97..=122).contains(&c1));
        let bare02 = v[k] == 0x02
            && v[k - 1] != 0x01
            && (1..=0x10).contains(&v[k - 1])
            && v[k - 2] >= 1;
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
                let triple010102 = anchor_a
                    && v[k] == 0x02
                    && v[k - 1] == 0x01
                    && k >= 4
                    && v[k - 2] == 0x01;
                let lead_01_02 = anchor_a
                    && v[k] == 0x02
                    && v[k - 1] == 0x01
                    && k >= 3
                    && v[k - 2] != 0x01;
                let (typ, count, is_geo_flag) = if anchor_b {
                    (v[k] as i64, count_b, false)
                } else if triple010102 {
                    (v[k - 3] as i64, v[k - 4] as i64, true)
                } else if geo_double01 {
                    // People REGION uses [type=0x12][count]; Products REGION uses
                    // [count][type] with a normal type byte before the 01 01.
                    let a = v[k - 3] as i64;
                    let b = v[k - 2] as i64;
                    if a == 0x12 {
                        (a, b, true)
                    } else {
                        (b, a, true)
                    }
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
                    if count >= 1 && count <= 2_000_000 && dims_name_ok(&name) {
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
    let facet_lend = text.find("FACET04").and_then(|facet| {
        if facet >= 200 {
            Some(facet)
        } else {
            None
        }
    });
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
    let d_off = u32(data, HDR_DESCRIPTOR_PTR) as usize;
    let mut best: Option<(usize, Vec<DimDesc>, i64)> = None;
    let consider = |best: &mut Option<(usize, Vec<DimDesc>, i64)>, off: usize, dims: Vec<DimDesc>| {
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
    let mut used: HashMap<String, i32> = HashMap::new();
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
        let slug = if let Some(c) = used.get_mut(&base) {
            *c += 1;
            format!("{base}_{c}")
        } else {
            used.insert(base.clone(), 1);
            base
        };
        out.push(slug);
    }
    out
}
