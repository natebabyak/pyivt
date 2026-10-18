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
                let double01 = anchor_a && v[k - 1] == 0x01;
                let (typ, count) = if anchor_b {
                    (v[k] as i64, count_b)
                } else if double01 {
                    (v[k - 3] as i64, v[k - 2] as i64)
                } else {
                    let typ = v[k - 1] as i64;
                    let count = if GEOTYPES.contains(&v[k - 1]) {
                        v[k - 3] as i64 + (v[k - 2] as i64) * 256
                    } else {
                        v[k - 2] as i64
                    };
                    (typ, count)
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
                    if count >= 1 {
                        dims.push(DimDesc {
                            name,
                            count,
                            type_code: typ,
                            double01,
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

pub fn parse_descriptor(data: &[u8]) -> Option<Descriptor> {
    if !data.is_empty() && data[0] == 0x02 {
        return None;
    }
    let d_off = u32(data, HDR_DESCRIPTOR_PTR) as usize;
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
    let lend = if let Some(facet) = text.find("FACET04") {
        if facet > 0 {
            facet
        } else {
            v.len()
        }
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
    } else {
        16
    };
    let mut dims = walk_records(v, lend, cap, false);
    if dims.len() < 2 {
        dims = walk_records(v, lend, cap, true);
    }
    if dims.len() < 2 {
        return None;
    }
    Some(Descriptor {
        offset: d_off as u32,
        dims,
    })
}

pub fn geo_dim_index(dims: &[DimDesc]) -> usize {
    if dims.is_empty() {
        return 1;
    }
    let name = dims[0].name.to_lowercase();
    if dims[0].count == 1 && (name == "values" || name == "value") {
        return dims.len();
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
