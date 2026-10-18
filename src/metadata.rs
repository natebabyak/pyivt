//! File summary, dimension members, geographies, footnotes.

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{HashMap, HashSet};

use crate::bytes::{latin1, rd_pascal, u16, u32};
use crate::descriptor::{geo_dim_index, parse_descriptor, Descriptor, DimDesc};
use crate::header::{
    dim_slots, read_block_dir, read_header, HeaderLayout, HDR_MASTER_PTR, HDR_STATUS_LEGEND,
};

static FACET_AUTHOR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?:\d{4} Census - Statistics Canada|Recensement de \d{4}-Statistique Canada|Beyond 20/20 Inc\.?|Ivation|OCDE|OECD)$",
    )
    .unwrap()
});
static TIME_SERIES_NAME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)timeseries|time\s*series|time\s*period|\bdate\b").unwrap());
static FREQ_NAME_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(annual|yearly|quarterly|monthly|weekly|daily|semi-?annual|annuel(?:le)?|trimestriel(?:le)?|mensuel(?:le)?|hebdomadaire|quotidien(?:ne)?|semi-?annuel(?:le)?)\b",
    )
    .unwrap()
});
static TS_RANGE_EN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|May|Jun(?:e)?|Jul(?:y)?|Aug(?:ust)?|Sep(?:t(?:ember)?)?|Oct(?:ober)?|Nov(?:ember)?|Dec(?:ember)?)\s*(\d{4})\b(?:.*?\b(Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|May|Jun(?:e)?|Jul(?:y)?|Aug(?:ust)?|Sep(?:t(?:ember)?)?|Oct(?:ober)?|Nov(?:ember)?|Dec(?:ember)?)\s*(\d{4})\b)?",
    )
    .unwrap()
});
static TS_RANGE_FR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(jan(?:vier)?|f[eé]v(?:rier)?|mars|avr(?:il)?|mai|juin|juil(?:let)?|ao[uû]t|sep(?:t(?:embre)?)?|oct(?:obre)?|nov(?:embre)?|d[eé]c(?:embre)?)\s*(\d{4})\b(?:.*?\b(jan(?:vier)?|f[eé]v(?:rier)?|mars|avr(?:il)?|mai|juin|juil(?:let)?|ao[uû]t|sep(?:t(?:embre)?)?|oct(?:obre)?|nov(?:embre)?|d[eé]c(?:embre)?)\s*(\d{4})\b)?",
    )
    .unwrap()
});
static FR_CHARS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[àâäéèêëïîôùûüçœæÀÂÄÉÈÊËÏÎÔÙÛÜÇ]").unwrap()
});
static FR_WORDS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(de|des|les|une|pour|dans|recensement|année|âge)\b").unwrap()
});
static PID_FIRST: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\w.-]+$").unwrap());
static PID_FULL: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\dA-Za-z][\w./-]*$").unwrap());
static FOOTNOTE_START_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(Footnote|Renvoi)\s*(\d+)\s*").unwrap());
static GEO_INLINE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^(.*)\s+\(([0-9A-Za-z.]+)\)(?:,\s*|\s+)(?:([^0-9\s]\S*)\s+)?([0-9]+)(?:\s*\(([^)]*)\))?\s*$",
    )
    .unwrap()
});
static GEO_SIMPLE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(.*?)\s*\(([0-9A-Za-z.]+)\)\s*$").unwrap());
static TNR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*([0-9]+(?:[.,][0-9]+)?)\s*%\s*$").unwrap());
/// Match Python 3 `re` `\s` on 8-bit/Latin text (includes file-separator controls).
static WS_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[\t\n\x0b\x0c\r\x1c-\x1f \x85\xa0]+").unwrap());
static PRODUCT_ID_HEAD: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"Product ID:\s*([^\r\n]+)").unwrap());
static TITLE_EN_HEAD: Lazy<Regex> = Lazy::new(|| Regex::new(r"Title:\s*([^\r\n]+)").unwrap());
static TITLE_FR_HEAD: Lazy<Regex> = Lazy::new(|| Regex::new(r"Titre\s*:\s*([^\r\n]+)").unwrap());
static UNIVERSE_HEAD: Lazy<Regex> = Lazy::new(|| Regex::new(r"Universe:\s*([^\r\n]+)").unwrap());
static PID_FALLBACK: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(\d{2}-\d{3}-[A-Z]\d{7,}|\d{8})\b").unwrap());
static GEO_CODE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\(\[]\d").unwrap());

/// Longest-first category suffixes stripped from glued FACET04 runs.
const FACET_CATEGORIES: &[&str] = &[
    "Beyond 20/20 Demonstration Tables and Extract",
    "Tables et Extrait de démonstration de Beyond 20/20",
    "Demostración de Tablas y Extractos de Beyond 20/20",
    "Tableaux de données",
    "Data Tables",
    "OECD Publications",
    "Publication d'OCDE",
    "Publicaciones de la OECD",
];
const READONLY_FLAG_BIT: u8 = 0x10;

#[derive(Debug, Clone)]
pub struct Dimension {
    pub name: String,
    pub name_fr: Option<String>,
    pub count: i64,
    pub type_code: i64,
    pub is_geography: bool,
    pub description: Option<String>,
    pub description_fr: Option<String>,
    pub type_label: String,
    pub members: Option<Vec<String>>,
    pub members_fr: Option<Vec<String>>,
    pub ordinal: Option<Vec<i64>>,
}

#[derive(Debug, Clone)]
pub struct Footnote {
    pub language: String,
    pub number: i64,
    pub text: String,
    pub scope: Option<String>,
    pub dimension: Option<String>,
    pub member_id: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct LocalizedText {
    pub en: Option<String>,
    pub fr: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MissingValue {
    pub symbol: String,
    pub label: LocalizedText,
    pub code: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct Geographies {
    pub member_id: Vec<i64>,
    pub geo_label: Option<Vec<String>>,
    pub geo_name: Option<Vec<Option<String>>>,
    pub geo_uid: Option<Vec<Option<String>>>,
    pub geo_label_fr: Option<Vec<String>>,
    pub dqf_code: Option<Vec<Option<String>>>,
    pub tnr_short_form: Option<Vec<Option<String>>>,
}

#[derive(Debug, Clone)]
pub struct IvtMetadata {
    pub product_id: Option<String>,
    pub title: LocalizedText,
    pub universe: Option<String>,
    pub notes: LocalizedText,
    pub version: String,
    pub signature: String,
    pub author: LocalizedText,
    pub category: LocalizedText,
    pub keywords: LocalizedText,
    pub number_of_datapoints: Option<i64>,
    pub time_series: LocalizedText,
    pub read_only: Option<bool>,
    data: Vec<u8>,
    desc: Descriptor,
    dimensions: Option<Vec<Dimension>>,
    geographies: Option<Geographies>,
    footnotes: Option<Vec<Footnote>>,
    missing_values: Option<Vec<MissingValue>>,
}

impl IvtMetadata {
    pub fn dimensions(&mut self) -> &Vec<Dimension> {
        self.ensure_dims_geo();
        self.dimensions.as_ref().unwrap()
    }

    pub fn geographies(&mut self) -> &Geographies {
        self.ensure_dims_geo();
        self.geographies.as_ref().unwrap()
    }

    pub fn footnotes(&mut self) -> &Vec<Footnote> {
        if self.footnotes.is_none() {
            self.footnotes = Some(footnotes(&self.data));
        }
        self.footnotes.as_ref().unwrap()
    }

    pub fn missing_values(&mut self) -> &Vec<MissingValue> {
        if self.missing_values.is_none() {
            self.missing_values = Some(missing_values(&self.data));
        }
        self.missing_values.as_ref().unwrap()
    }

    fn ensure_dims_geo(&mut self) {
        if self.dimensions.is_some() && self.geographies.is_some() {
            return;
        }
        let dims = build_dimensions(&self.data, &self.desc);
        let gd = geo_dim_index(&self.desc.dims);
        let geos = geo_from_dir(&self.data, &self.desc.dims[gd - 1], gd - 1);
        self.dimensions = Some(dims);
        self.geographies = Some(geos);
    }

    pub fn loaded(&self, name: &str) -> bool {
        match name {
            "dimensions" => self.dimensions.is_some(),
            "geographies" => self.geographies.is_some(),
            "footnotes" => self.footnotes.is_some(),
            "missing_values" => self.missing_values.is_some(),
            _ => false,
        }
    }

    pub fn desc(&self) -> &Descriptor {
        &self.desc
    }

}

fn frscore(s: &str) -> usize {
    if s.is_empty() {
        return 0;
    }
    FR_CHARS.find_iter(s).count() + FR_WORDS.find_iter(s).count()
}

fn master_dir_entries(data: &[u8], max_entries: usize) -> Vec<(u32, u16)> {
    let master = u32(data, HDR_MASTER_PTR) as usize;
    let mut out = Vec::new();
    if !(0 < master && master < data.len().saturating_sub(8)) {
        return out;
    }
    let mut o = master;
    for _ in 0..max_entries {
        if o + 8 > data.len() {
            break;
        }
        let off = u32(data, o);
        let s1 = u16(data, o + 4);
        o += 8;
        if off == 0 && s1 == 0 {
            continue;
        }
        if s1 == 0 || (off as usize) + (s1 as usize) > data.len() {
            continue;
        }
        out.push((off, s1));
    }
    out
}

fn split_facet_run(text: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut rest = text.trim().trim_end_matches(|c: char| (c as u32) < 32 || c == '\u{fffd}').to_string();
    if rest.is_empty() {
        return (None, None, None);
    }
    // Strip trailing control / high junk that sometimes sticks to FACET runs.
    while rest
        .chars()
        .last()
        .map(|c| (c as u32) < 32 || c == '\u{81}' || c == '\u{fffd}')
        .unwrap_or(false)
    {
        rest.pop();
    }
    let mut category = None;
    for cat in FACET_CATEGORIES {
        if rest.ends_with(cat) {
            category = Some((*cat).to_string());
            rest.truncate(rest.len() - cat.len());
            break;
        }
    }
    let mut author = None;
    if let Some(m) = FACET_AUTHOR_RE.find(&rest) {
        author = Some(m.as_str().to_string());
        rest.truncate(m.start());
    }
    let title = {
        let t = rest.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    };
    (title, author, category)
}

fn parse_facet_summary(data: &[u8]) -> HashMap<&'static str, Option<String>> {
    let mut runs = Vec::new();
    let mut start = 0;
    while let Some(rel) = data[start..]
        .windows(7)
        .position(|w| w == b"FACET04")
    {
        let i = start + rel;
        let run = &data[i + 7..];
        let mut end = 0;
        while end < run.len() && run[end] >= 32 {
            end += 1;
        }
        let text = latin1(&run[..end]).trim().to_string();
        if !text.is_empty() {
            runs.push(text);
        }
        start = i + 7;
    }
    let mut parsed: Vec<_> = runs.iter().map(|r| split_facet_run(r)).collect();
    parsed.retain(|(t, _, _)| t.is_some());
    parsed.sort_by_key(|(t, _, _)| frscore(t.as_deref().unwrap_or("")));

    let mut out = HashMap::new();
    out.insert("title_en", None);
    out.insert("title_fr", None);
    out.insert("author_en", None);
    out.insert("author_fr", None);
    out.insert("category_en", None);
    out.insert("category_fr", None);

    if parsed.is_empty() {
        return out;
    }
    let en = parsed[0].clone();
    let mut fr = None;
    if parsed.len() > 1
        && frscore(parsed.last().unwrap().0.as_deref().unwrap_or(""))
            > frscore(parsed[0].0.as_deref().unwrap_or(""))
    {
        fr = Some(parsed.last().unwrap().clone());
    }
    let (mut title_en, mut author_en, mut category_en) = en;
    let (mut title_fr, mut author_fr, mut category_fr) = (None, None, None);
    if let Some(f) = fr {
        title_fr = f.0;
        author_fr = f.1;
        category_fr = f.2;
    } else if frscore(title_en.as_deref().unwrap_or("")) >= 2 {
        title_fr = title_en.take();
        author_fr = author_en.take();
        category_fr = category_en.take();
    }
    out.insert("title_en", title_en);
    out.insert("title_fr", title_fr);
    out.insert("author_en", author_en);
    out.insert("author_fr", author_fr);
    out.insert("category_en", category_en);
    out.insert("category_fr", category_fr);
    out
}

fn identity_from_blob(text: &str) -> (Option<String>, Option<String>) {
    let text = text.trim_start();
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized.splitn(2, '\n');
    let pid = lines.next().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let title = lines.next().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if let Some(ref p) = pid {
        let first = p.split_whitespace().next().unwrap_or("");
        if !PID_FIRST.is_match(first) {
            return (None, Some(text.trim().to_string()));
        }
    }
    (pid, title)
}

fn parse_identity_notes(data: &[u8]) -> (Option<String>, Option<String>, Option<String>) {
    let mut notes_en = None;
    let mut notes_fr = None;
    let mut product_id = None;
    let mut long_blobs = Vec::new();
    for (off, length) in master_dir_entries(data, 32) {
        let blk = &data[off as usize..off as usize + length as usize];
        if blk.len() < 2 || &blk[..2] != b"\x01\x01" || length < 40 {
            continue;
        }
        let payload = &blk[4..];
        let mut i = 0;
        while i < payload.len() && payload[i] < 0x20 {
            i += 1;
        }
        let text = latin1(&payload[i..]).trim_end_matches('\0').trim().to_string();
        if text.is_empty() || (!text.contains('\n') && !text.contains('\r')) {
            continue;
        }
        long_blobs.push(text.clone());
        let (pid, _) = identity_from_blob(&text);
        if let Some(p) = pid {
            if PID_FULL.is_match(&p) {
                product_id = product_id.or(Some(p));
            }
        }
    }
    long_blobs.sort_by_key(|s| frscore(s));
    if !long_blobs.is_empty() {
        let en = long_blobs[0].clone();
        let mut fr = None;
        if long_blobs.len() > 1
            && frscore(long_blobs.last().unwrap()) > frscore(&long_blobs[0])
        {
            fr = Some(long_blobs.last().unwrap().clone());
        }
        notes_en = Some(en.clone());
        notes_fr = fr;
        if notes_fr.is_none() && frscore(&en) >= 2 {
            notes_fr = Some(en);
            notes_en = None;
        }
    }
    (notes_en, notes_fr, product_id)
}

fn parse_keywords(data: &[u8], product_id: Option<&str>) -> LocalizedText {
    let mut candidates = Vec::new();
    for (off, length) in master_dir_entries(data, 32) {
        if !(8..=40).contains(&length) {
            continue;
        }
        let blk = &data[off as usize..off as usize + length as usize];
        if blk.len() < 2 || &blk[..2] != b"\x01\x01" {
            continue;
        }
        let payload = &blk[4..];
        let mut i = 0;
        while i < payload.len() && payload[i] < 0x20 {
            i += 1;
        }
        if i < payload.len() && (1..64).contains(&payload[i]) {
            let n = payload[i] as usize;
            if i + 1 + n <= payload.len() {
                let kw = latin1(&payload[i + 1..i + 1 + n])
                    .trim_end_matches('\0')
                    .trim()
                    .to_string();
                if !kw.is_empty() {
                    candidates.push(kw);
                    continue;
                }
            }
        }
        let text = latin1(&payload[i..])
            .trim_end_matches('\0')
            .trim()
            .to_string();
        if !text.is_empty() && !text.contains('\n') && text.len() <= 40 {
            candidates.push(text);
        }
    }
    if candidates.is_empty() {
        return LocalizedText::default();
    }
    let mut unique = Vec::new();
    for c in candidates {
        if !unique.contains(&c) {
            unique.push(c);
        }
    }
    if let Some(pid) = product_id {
        for c in &unique {
            if c == pid {
                let others: Vec<_> = unique.iter().filter(|x| *x != c).cloned().collect();
                if !others.is_empty() {
                    let mut scored = others;
                    scored.sort_by_key(|s| frscore(s));
                    let fr = if frscore(scored.last().unwrap()) > frscore(c) {
                        Some(scored.last().unwrap().clone())
                    } else {
                        None
                    };
                    return LocalizedText {
                        en: Some(c.clone()),
                        fr,
                    };
                }
                return LocalizedText {
                    en: Some(c.clone()),
                    fr: None,
                };
            }
        }
    }
    if unique.len() == 1 {
        return LocalizedText {
            en: Some(unique[0].clone()),
            fr: None,
        };
    }
    let mut scored = unique;
    scored.sort_by_key(|s| frscore(s));
    let en = Some(scored[0].clone());
    let fr = if frscore(scored.last().unwrap()) > frscore(&scored[0]) {
        Some(scored.last().unwrap().clone())
    } else {
        None
    };
    LocalizedText { en, fr }
}

fn time_dim_desc(dims: &[DimDesc]) -> Option<&DimDesc> {
    for d in dims {
        if d.type_code == 6 {
            return Some(d);
        }
        if TIME_SERIES_NAME_RE.is_match(&d.name) {
            return Some(d);
        }
    }
    None
}

fn has_time_series_marker(data: &[u8]) -> bool {
    let mut o = 0;
    while let Some(rel) = data[o..].windows(2).position(|w| w == b"\x81\x02") {
        let i = o + rel;
        if i + 6 > data.len() {
            break;
        }
        if &data[i + 4..i + 6] == b"\x08\x00" {
            return true;
        }
        o = i + 2;
    }
    false
}

fn title_case_freq(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

fn frequency_hint_name(name: &str) -> Option<String> {
    FREQ_NAME_RE
        .captures(name)
        .map(|c| title_case_freq(c.get(1).unwrap().as_str()))
}

fn month_abbr_en(raw: &str) -> &'static str {
    let s = raw.to_ascii_lowercase();
    match s.as_str() {
        "january" | "jan" => "Jan",
        "february" | "feb" => "Feb",
        "march" | "mar" => "Mar",
        "april" | "apr" => "Apr",
        "may" => "May",
        "june" | "jun" => "Jun",
        "july" | "jul" => "Jul",
        "august" | "aug" => "Aug",
        "september" | "sept" | "sep" => "Sep",
        "october" | "oct" => "Oct",
        "november" | "nov" => "Nov",
        "december" | "dec" => "Dec",
        _ => "Jan",
    }
}

fn month_abbr_fr(raw: &str) -> &'static str {
    let s = raw.to_lowercase();
    match s.as_str() {
        "janvier" | "jan" => "jan",
        "février" | "fevrier" | "fév" | "fev" => "fév",
        "mars" => "mars",
        "avril" | "avr" => "avr",
        "mai" => "mai",
        "juin" => "juin",
        "juillet" | "juil" => "juil",
        "août" | "aout" => "août",
        "septembre" | "sept" | "sep" => "sept",
        "octobre" | "oct" => "oct",
        "novembre" | "nov" => "nov",
        "décembre" | "decembre" | "déc" | "dec" => "déc",
        _ => "jan",
    }
}

fn freq_pair_from_text(text: &str) -> Option<(String, String)> {
    let lower = text.to_lowercase();
    if lower.contains("semi-annual") || lower.contains("semiannual") || lower.contains("semi-annuel")
    {
        return Some(("Semi-annual".into(), "Semi-annuelle".into()));
    }
    if lower.contains("monthly") || lower.contains("mensuel") {
        return Some(("Monthly".into(), "Mensuelle".into()));
    }
    if lower.contains("quarterly") || lower.contains("trimestriel") {
        return Some(("Quarterly".into(), "Trimestrielle".into()));
    }
    if lower.contains("annual") || lower.contains("yearly") || lower.contains("annuel") {
        return Some(("Annual".into(), "Annuelle".into()));
    }
    if lower.contains("weekly") || lower.contains("hebdomadaire") {
        return Some(("Weekly".into(), "Hebdomadaire".into()));
    }
    if lower.contains("daily") || lower.contains("quotidien") {
        return Some(("Daily".into(), "Quotidienne".into()));
    }
    None
}

fn month_index_en(raw: &str) -> Option<u32> {
    match month_abbr_en(raw) {
        "Jan" => Some(1),
        "Feb" => Some(2),
        "Mar" => Some(3),
        "Apr" => Some(4),
        "May" => Some(5),
        "Jun" => Some(6),
        "Jul" => Some(7),
        "Aug" => Some(8),
        "Sep" => Some(9),
        "Oct" => Some(10),
        "Nov" => Some(11),
        "Dec" => Some(12),
        _ => None,
    }
}

fn abbr_en_from_index(m: u32) -> &'static str {
    match m {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "Jan",
    }
}

fn add_months(year: u32, month: u32, add: i64) -> (u32, u32) {
    let idx = (year as i64) * 12 + (month as i64 - 1) + add;
    let y = (idx / 12) as u32;
    let m = (idx % 12 + 1) as u32;
    (y, m)
}

fn format_range_en(m1: &str, y1: &str, m2: &str, y2: &str) -> String {
    format!(
        "{}{}-{}{}",
        month_abbr_en(m1),
        y1,
        month_abbr_en(m2),
        y2
    )
}

fn format_range_fr_from_en(en: &str) -> String {
    en.replace("Jan", "jan")
        .replace("Feb", "fév")
        .replace("Mar", "mars")
        .replace("Apr", "avr")
        .replace("May", "mai")
        .replace("Jun", "juin")
        .replace("Jul", "juil")
        .replace("Aug", "août")
        .replace("Sep", "sept")
        .replace("Oct", "oct")
        .replace("Nov", "nov")
        .replace("Dec", "déc")
}

fn range_from_text(text: &str, time_count: Option<i64>) -> (Option<String>, Option<String>) {
    let mut en = None;
    let mut fr = None;
    if let Some(c) = TS_RANGE_EN_RE.captures(text) {
        let m1 = c.get(1).unwrap().as_str();
        let y1 = c.get(2).unwrap().as_str();
        if let (Some(m2), Some(y2)) = (c.get(3), c.get(4)) {
            en = Some(format_range_en(m1, y1, m2.as_str(), y2.as_str()));
        } else if let (Some(count), Some(mi), Ok(year)) =
            (time_count, month_index_en(m1), y1.parse::<u32>())
        {
            if count >= 1 {
                let (ey, em) = add_months(year, mi, count - 1);
                en = Some(format!(
                    "{}{}-{}{}",
                    month_abbr_en(m1),
                    y1,
                    abbr_en_from_index(em),
                    ey
                ));
            }
        }
    }
    if let Some(c) = TS_RANGE_FR_RE.captures(text) {
        let m1 = c.get(1).unwrap().as_str();
        let y1 = c.get(2).unwrap().as_str();
        if let (Some(m2), Some(y2)) = (c.get(3), c.get(4)) {
            fr = Some(format!(
                "{}{}-{}{}",
                month_abbr_fr(m1),
                y1,
                month_abbr_fr(m2.as_str()),
                y2.as_str()
            ));
        }
    }
    // Prefer translating a complete EN range (includes count-derived ends).
    if let Some(ref e) = en {
        fr = Some(format_range_fr_from_en(e));
    }
    (en, fr)
}

fn time_series_label(
    data: &[u8],
    dims: &[DimDesc],
    notes_en: Option<&str>,
    notes_fr: Option<&str>,
) -> LocalizedText {
    let dim = time_dim_desc(dims);
    if dim.is_none() && !has_time_series_marker(data) {
        return LocalizedText {
            en: Some("No".into()),
            fr: Some("No".into()),
        };
    }
    // Dim notes (frequency/range) often live outside the identity-notes blobs.
    let file_text = latin1(data);
    let mut freq_en = None;
    let mut freq_fr = None;
    for text in [notes_en, notes_fr, Some(file_text.as_str())]
        .into_iter()
        .flatten()
    {
        if let Some((e, f)) = freq_pair_from_text(text) {
            freq_en = Some(e);
            freq_fr = Some(f);
            break;
        }
    }
    if freq_en.is_none() {
        if let Some(dim) = dim {
            if let Some(f) = frequency_hint_name(&dim.name) {
                let pair = freq_pair_from_text(&f).unwrap_or_else(|| (f.clone(), f));
                freq_en = Some(pair.0);
                freq_fr = Some(pair.1);
            }
        }
    }
    let time_count = dim.map(|d| d.count);
    let mut range_en = None;
    let mut range_fr = None;
    for text in [notes_en, notes_fr, Some(file_text.as_str())]
        .into_iter()
        .flatten()
    {
        let (e, f) = range_from_text(text, time_count);
        if range_en.is_none() {
            range_en = e;
        }
        if range_fr.is_none() {
            range_fr = f;
        }
        if range_en.is_some() && range_fr.is_some() {
            break;
        }
    }
    if let (Some(fe), Some(re)) = (freq_en.clone(), range_en.clone()) {
        return LocalizedText {
            en: Some(format!("{fe}, {re}")),
            fr: Some(format!(
                "{}, {}",
                freq_fr.unwrap_or(fe),
                range_fr.unwrap_or(re)
            )),
        };
    }
    if let Some(fe) = freq_en {
        return LocalizedText {
            en: Some(fe.clone()),
            fr: freq_fr.or(Some(fe)),
        };
    }
    if let Some(dim) = dim {
        if !dim.name.is_empty() {
            return LocalizedText {
                en: Some(dim.name.clone()),
                fr: None,
            };
        }
    }
    LocalizedText {
        en: Some("Yes".into()),
        fr: Some("Yes".into()),
    }
}

fn type_label_for(d: &DimDesc) -> String {
    if d.type_code == 6 || TIME_SERIES_NAME_RE.is_match(&d.name) {
        return "Time Series".into();
    }
    if d.double01 || d.type_code == 0x12 {
        return "Geographic".into();
    }
    "Normal".into()
}

fn read_only_flag(data: &[u8]) -> bool {
    for (off, length) in master_dir_entries(data, 32) {
        if length < 5 {
            continue;
        }
        let blk = &data[off as usize..off as usize + length as usize];
        if blk.len() >= 5 && &blk[..4] == b"\x81\x01\x20\x00" {
            return (blk[4] & READONLY_FLAG_BIT) != 0;
        }
    }
    false
}

fn parse_table_identity(data: &[u8]) -> HashMap<&'static str, Option<String>> {
    let head = latin1(&data[..data.len().min(8000)]);
    let grab = |re: &Regex| re.captures(&head).map(|m| m.get(1).unwrap().as_str().trim().to_string());
    let mut info: HashMap<&'static str, Option<String>> = HashMap::new();
    info.insert("product_id", grab(&PRODUCT_ID_HEAD));
    info.insert("title_en", grab(&TITLE_EN_HEAD));
    info.insert("title_fr", grab(&TITLE_FR_HEAD));
    info.insert("universe", grab(&UNIVERSE_HEAD));

    let mut blobs = Vec::new();
    for (off, s1) in master_dir_entries(data, 32) {
        let blk = &data[off as usize..off as usize + s1 as usize];
        if blk.len() >= 2 && &blk[..2] == b"\x01\x01" && s1 > 8 {
            let payload = &blk[4..];
            let mut i = 0;
            while i < payload.len() && payload[i] < 0x20 {
                i += 1;
            }
            let text = latin1(&payload[i..]).trim().to_string();
            if !text.is_empty() {
                blobs.push(text);
            }
        } else if !blk.is_empty()
            && (blk[0] == b' ' || blk[0].is_ascii_alphabetic())
        {
            let text = latin1(blk).trim().to_string();
            if text.contains("98-") || text.contains("Title") {
                blobs.push(text);
            }
        }
    }

    let facet = parse_facet_summary(data);
    let mut pid = info.get("product_id").cloned().flatten();
    let mut title_en = info.get("title_en").cloned().flatten();
    let mut title_fr = info.get("title_fr").cloned().flatten();

    for blob in &blobs {
        let (p, t) = identity_from_blob(blob);
        if let Some(ref p) = p {
            if (pid.is_none() || p.len() > pid.as_ref().map(|x| x.len()).unwrap_or(0))
                && PID_FULL.is_match(p)
            {
                pid = Some(p.clone());
            }
        }
        if let Some(t) = t {
            if title_en.is_none() && title_fr.is_none() {
                if frscore(&t) >= 2 {
                    title_fr = Some(t);
                } else {
                    title_en = Some(t);
                }
            }
        }
    }

    if let Some(t) = facet.get("title_en").cloned().flatten() {
        title_en = Some(t);
    }
    if let Some(t) = facet.get("title_fr").cloned().flatten() {
        title_fr = Some(t);
    }
    if pid.is_none() {
        if let Some(m) = PID_FALLBACK.captures(&head) {
            pid = Some(m.get(1).unwrap().as_str().to_string());
        }
    }

    info.insert("product_id", pid);
    info.insert("title_en", title_en);
    info.insert("title_fr", title_fr);
    info.insert("author_en", facet.get("author_en").cloned().flatten());
    info.insert("author_fr", facet.get("author_fr").cloned().flatten());
    info.insert("category_en", facet.get("category_en").cloned().flatten());
    info.insert("category_fr", facet.get("category_fr").cloned().flatten());
    info
}

fn labels_plausible(labels: &[String]) -> bool {
    if labels.is_empty() {
        return false;
    }
    for lab in labels {
        let u = lab.to_uppercase();
        if u.contains("DEFINITION") || lab.contains("DÉFINITION") {
            return false;
        }
        if lab.contains('\n') || lab.contains('\r') {
            return false;
        }
    }
    let joined = labels.iter().take(5).cloned().collect::<Vec<_>>().join(" ");
    if joined.contains("Digit description") || joined.contains("Flag description") {
        return false;
    }
    if joined.contains("Incomplete enumeration") {
        return false;
    }
    let short = labels.iter().filter(|x| x.trim().len() <= 1).count();
    short <= (1.max(labels.len() / 3))
}

fn read_member_array(block: &[u8], expect: usize) -> Vec<String> {
    if expect < 1 || block.len() < 8 {
        return Vec::new();
    }
    if block[..2] != *b"\x01\x01" && block[..2] != *b"\x81\x01" {
        return Vec::new();
    }
    let starts = [6usize, 7, 8, 4, 10, 12];
    for start in starts {
        let mut labels = Vec::new();
        let mut p = start;
        while p < block.len() && labels.len() < expect {
            if block[p] == 0 && p + 1 < block.len() {
                p += 1;
                continue;
            }
            match rd_pascal(block, p, 250) {
                Some(rec) => {
                    labels.push(rec.text);
                    p = rec.end;
                }
                None => break,
            }
        }
        if labels.len() == expect && labels_plausible(&labels) {
            return labels;
        }
    }
    if block[..2] == *b"\x81\x01" {
        let max_start = 64.min(block.len().saturating_sub(expect));
        for start in 4..max_start {
            let mut labels = Vec::new();
            let mut p = start;
            while p < block.len() && labels.len() < expect {
                match rd_pascal(block, p, 250) {
                    Some(rec) => {
                        labels.push(rec.text);
                        p = rec.end;
                    }
                    None => break,
                }
            }
            if labels.len() == expect && labels_plausible(&labels) {
                return labels;
            }
        }
    }
    Vec::new()
}

fn split_en_fr_glued(code: &str, glued: &str) -> (String, Option<String>) {
    let cl = code.to_ascii_lowercase();
    let gl = glued.to_ascii_lowercase();
    if gl.starts_with(&cl) && glued.len() >= code.len() {
        let fr = glued[code.len()..].to_string();
        let en = if glued[..code.len()]
            .chars()
            .any(|c| c.is_ascii_lowercase())
        {
            // Prefer the title-case English spelling from the glued run.
            glued[..code.len()].to_string()
        } else {
            code.to_string()
        };
        return (en, if fr.is_empty() { None } else { Some(fr) });
    }
    // e.g. "Time PeriodTEMPS" — English phrase then an uppercase French token.
    let bytes = glued.as_bytes();
    for i in 1..glued.len() {
        if bytes[i - 1].is_ascii_lowercase() && bytes[i].is_ascii_uppercase() {
            let en = glued[..i].to_string();
            let fr = glued[i..].to_string();
            if !en.is_empty() && !fr.is_empty() {
                return (en, Some(fr));
            }
        }
    }
    (glued.to_string(), None)
}

/// Demo-table title block: CODE 01 04 32 EN+FR 02 05 32 …
fn parse_dim_title_demo(block: &[u8]) -> (Option<String>, Option<String>, Option<String>) {
    let marker = b"\x01\x042";
    let Some(rel) = block.windows(marker.len()).position(|w| w == marker) else {
        return (None, None, None);
    };
    // Walk back over the ASCII code name.
    let mut start = rel;
    while start > 0 && (32..=126).contains(&block[start - 1]) {
        start -= 1;
    }
    let code = latin1(&block[start..rel]).trim().to_string();
    if code.len() < 2 {
        return (None, None, None);
    }
    let p = rel + marker.len();
    let mut end = p;
    while end < block.len() {
        if block[end] < 32 {
            break;
        }
        // Next language marker: 0x02/0x03 … 0x32
        if end + 2 < block.len()
            && (block[end] == 0x02 || block[end] == 0x03)
            && block[end + 2] == 0x32
        {
            break;
        }
        end += 1;
    }
    // Trim if we over-consumed into a 0x02/0x03 marker without 0x32 yet.
    while end > p && block[end - 1] < 32 {
        end -= 1;
    }
    let glued = latin1(&block[p..end]);
    let (desc_en, desc_fr) = split_en_fr_glued(&code, &glued);
    // COOKIE TYPE → Name COOKIE; keep TIME PERIOD / REGION as-is.
    let short = if code.to_ascii_uppercase().ends_with(" TYPE") {
        code.split_whitespace()
            .next()
            .unwrap_or(&code)
            .to_string()
    } else {
        code.clone()
    };
    (
        Some(short),
        Some(if desc_en.is_empty() { code } else { desc_en }),
        desc_fr,
    )
}

/// Returns (short_code, description_en, description_fr).
fn parse_dim_title(block: &[u8]) -> (Option<String>, Option<String>, Option<String>) {
    if block.len() < 16 || block[..2] != *b"\x81\x02" {
        return (None, None, None);
    }
    let demo = parse_dim_title_demo(block);
    if demo.0.is_some() {
        return demo;
    }
    for i in 0..block.len().saturating_sub(10) {
        if block[i + 1..i + 3] != *b"\x02\x00" {
            continue;
        }
        let p = i + 8;
        if p >= block.len()
            || !((65..=90).contains(&block[p]) || (97..=122).contains(&block[p]))
        {
            continue;
        }
        let end = match block[p..].iter().position(|&b| b == 0x01) {
            Some(rel) => p + rel,
            None => continue,
        };
        if !(1..=40).contains(&(end - p)) {
            continue;
        }
        let short = latin1(&block[p..end]);
        if short.is_empty() || !short.chars().all(|c| (32..127).contains(&(c as u8))) {
            continue;
        }
        let mut q = end + 1;
        while q < block.len() && block[q] != 0x32 {
            if block[q] > 0x20 {
                break;
            }
            q += 1;
        }
        if q >= block.len() || block[q] != 0x32 {
            continue;
        }
        q += 1;
        let mut end2 = None;
        let mut fr_start = None;
        let mut r = q;
        while r < block.len() {
            let j = match block[r..].iter().position(|&b| b == 0x02) {
                Some(rel) => r + rel,
                None => break,
            };
            let mut k = j + 1;
            while k < block.len() && block[k] != 0x32 {
                if block[k] > 0x20 {
                    break;
                }
                k += 1;
            }
            if k < block.len() && block[k] == 0x32 {
                end2 = Some(j);
                fr_start = Some(k + 1);
                break;
            }
            r = j + 1;
        }
        let glued = latin1(match end2 {
            Some(e) => &block[q..e],
            None => &block[q..],
        });
        let description = if glued.is_empty() {
            short.clone()
        } else if glued.starts_with(&short) && glued.len() > short.len() {
            // Prefer the longer English label when code is a prefix (COOKIE / COOKIE TYPE).
            glued.clone()
        } else {
            glued.clone()
        };
        let mut description_fr = None;
        if let Some(fs) = fr_start {
            let raw_fr = latin1(&block[fs..]);
            let mut out = String::new();
            for c in raw_fr.chars() {
                if (c as u32) < 32 {
                    break;
                }
                out.push(c);
            }
            if !out.is_empty() {
                description_fr = Some(out);
            }
        }
        if description_fr.is_none() && glued.starts_with(&short) && glued.len() > short.len() {
            description_fr = Some(glued[short.len()..].to_string());
        }
        return (Some(short), Some(description), description_fr);
    }
    (None, None, None)
}

fn dim_title(
    data: &[u8],
    slot_index: usize,
) -> (Option<String>, Option<String>, Option<String>) {
    let slots = dim_slots(data, 32);
    if slot_index >= slots.len() {
        return (None, None, None);
    }
    let slot = slots[slot_index];
    for (off, length) in read_block_dir(data, slot.dir_ptr, slot.n_entries) {
        if length < 16 {
            continue;
        }
        let (short, description, description_fr) =
            parse_dim_title(&data[off as usize..off as usize + length as usize]);
        if short.is_some() || description.is_some() {
            return (short, description, description_fr);
        }
    }
    (None, None, None)
}

fn dim_dir_labels(
    data: &[u8],
    dim: &DimDesc,
    slot_index: usize,
    geography: bool,
) -> (Option<Vec<String>>, Option<Vec<String>>) {
    let slots = dim_slots(data, 32);
    if slot_index >= slots.len() {
        return (None, None);
    }
    let slot = slots[slot_index];
    let entries = read_block_dir(data, slot.dir_ptr, slot.n_entries);
    let mut arrays = Vec::new();
    for (off, length) in entries {
        if (length as i64) < dim.count + 4 {
            continue;
        }
        let block = &data[off as usize..off as usize + length as usize];
        if block.len() >= 2 && block[..2] == *b"\x81\x02" {
            continue;
        }
        let labels = read_member_array(block, dim.count as usize);
        if labels.len() != dim.count as usize {
            continue;
        }
        if geography {
            let sample = labels.iter().take(3).cloned().collect::<Vec<_>>().join(" ");
            if !sample.contains("Canada")
                && !labels.iter().take(5).any(|x| GEO_CODE_RE.is_match(x))
            {
                continue;
            }
        }
        arrays.push(labels);
    }
    if arrays.is_empty() {
        return (None, None);
    }
    let mut en = arrays.remove(0);
    let mut fr = if arrays.is_empty() {
        None
    } else {
        Some(arrays.remove(0))
    };
    if let Some(ref f) = fr {
        if frscore(&f[..3.min(f.len())].join(" ")) < frscore(&en[..3.min(en.len())].join(" ")) {
            let tmp = en;
            en = f.clone();
            fr = Some(tmp);
        }
    }
    (Some(en), fr)
}

struct InlineGeo {
    geo_name: Option<String>,
    geo_uid: Option<String>,
    dqf_code: Option<String>,
    tnr_short_form: Option<String>,
}

fn parse_inline_geo(label: &str) -> InlineGeo {
    let mut out = InlineGeo {
        geo_name: None,
        geo_uid: None,
        dqf_code: None,
        tnr_short_form: None,
    };
    if let Some(m) = GEO_INLINE_RE.captures(label) {
        out.geo_name = Some(m.get(1).unwrap().as_str().trim_end().to_string());
        out.geo_uid = Some(m.get(2).unwrap().as_str().to_string());
        out.dqf_code = Some(m.get(4).unwrap().as_str().to_string());
        if let Some(g5) = m.get(5) {
            if let Some(tm) = TNR_RE.captures(g5.as_str()) {
                out.tnr_short_form = Some(tm.get(1).unwrap().as_str().replace(',', "."));
            }
        }
        return out;
    }
    if let Some(m2) = GEO_SIMPLE_RE.captures(label) {
        out.geo_name = Some(m2.get(1).unwrap().as_str().trim_end().to_string());
        out.geo_uid = Some(m2.get(2).unwrap().as_str().to_string());
    } else {
        out.geo_name = Some(label.trim().to_string());
    }
    out
}

fn geo_from_dir(data: &[u8], dim: &DimDesc, slot_index: usize) -> Geographies {
    let (en, fr) = dim_dir_labels(data, dim, slot_index, true);
    let n = dim.count as usize;
    let mut geo = Geographies {
        member_id: (1..=dim.count).collect(),
        ..Default::default()
    };
    if let Some(mut en) = en {
        if en.len() >= n {
            en.truncate(n);
            let parsed: Vec<_> = en.iter().map(|x| parse_inline_geo(x)).collect();
            geo.geo_label = Some(en);
            geo.geo_name = Some(parsed.iter().map(|p| p.geo_name.clone()).collect());
            geo.geo_uid = Some(parsed.iter().map(|p| p.geo_uid.clone()).collect());
            if parsed.iter().any(|p| p.dqf_code.is_some()) {
                geo.dqf_code = Some(parsed.iter().map(|p| p.dqf_code.clone()).collect());
            }
            if parsed.iter().any(|p| p.tnr_short_form.is_some()) {
                geo.tnr_short_form =
                    Some(parsed.iter().map(|p| p.tnr_short_form.clone()).collect());
            }
        }
    }
    if let Some(mut fr) = fr {
        if fr.len() >= n {
            fr.truncate(n);
            geo.geo_label_fr = Some(fr);
        }
    }
    geo
}

fn footnotes(data: &[u8]) -> Vec<Footnote> {
    let start = data.len().saturating_sub(200_000);
    let regions: [&[u8]; 2] = [&data[..data.len().min(12000)], &data[start..]];
    let mut out = Vec::new();
    let mut counts = HashMap::from([("en", 0i64), ("fr", 0i64)]);
    for region in regions {
        let s = latin1(region);
        let starts: Vec<_> = FOOTNOTE_START_RE.captures_iter(&s).collect();
        for (i, m) in starts.iter().enumerate() {
            let lang = if m.get(1).unwrap().as_str().to_lowercase().starts_with("foot") {
                "en"
            } else {
                "fr"
            };
            let body_start = m.get(0).unwrap().end();
            let body_end = if i + 1 < starts.len() {
                starts[i + 1].get(0).unwrap().start()
            } else {
                s.len()
            };
            let body = WS_RE
                .replace_all(&s[body_start..body_end], " ")
                .trim()
                .to_string();
            if body.len() < 8 {
                continue;
            }
            *counts.get_mut(lang).unwrap() += 1;
            let num = *counts.get(lang).unwrap();
            let text = if body.chars().count() > 4000 {
                body.chars().take(4000).collect()
            } else {
                body
            };
            out.push(Footnote {
                language: lang.into(),
                number: num,
                text,
                scope: None,
                dimension: None,
                member_id: None,
            });
        }
    }
    let mut seen = HashSet::new();
    let mut uniq = Vec::new();
    for f in out {
        let key = f.text.chars().take(80).collect::<String>();
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        uniq.push(f);
        if uniq.len() >= 200 {
            break;
        }
    }
    uniq
}

fn parse_status_legend_record(blob: &[u8]) -> Option<(String, String)> {
    if blob.len() < 8 || (blob[..2] != *b"\x82\x01" && blob[..2] != *b"\x02\x01") {
        return None;
    }
    let body = &blob[4..];
    for i in 0..16.min(body.len().saturating_sub(4)) {
        let n = body[i] as usize;
        if !(1..=8).contains(&n) || i + 1 + n + 2 > body.len() {
            continue;
        }
        let sym = &body[i + 1..i + 1 + n];
        if *sym.last().unwrap() != 0 {
            continue;
        }
        let symbol = latin1(&sym[..sym.len() - 1]);
        if symbol.is_empty() || !symbol.chars().all(|c| (33..127).contains(&(c as u8))) {
            continue;
        }
        let text_len = u16(body, i + 1 + n) as usize;
        let start = i + 1 + n + 2;
        let take = text_len.min(body.len().saturating_sub(start));
        if take < 1 {
            continue;
        }
        let raw = &body[start..start + take];
        let mut out = Vec::new();
        for &b in raw {
            if b == 0 || b < 9 || b == 127 {
                break;
            }
            out.push(b);
        }
        let text = latin1(&out).trim().to_string();
        if text.len() < 3 {
            continue;
        }
        return Some((symbol, text));
    }
    None
}

fn status_legend_dir(data: &[u8], ptr: u32, n_entries: u32) -> Vec<(u32, u16)> {
    let mut entries = Vec::new();
    let mut o = ptr as usize;
    for _ in 0..n_entries {
        if o + 8 > data.len() {
            break;
        }
        let off = u32(data, o);
        let s1 = u16(data, o + 4);
        let s2 = u16(data, o + 6);
        o += 8;
        if off == 0 && s1 == 0 {
            entries.push((0, 0));
            continue;
        }
        let ok = s1 == s2 || s2 == 4 * ((s1 + 3) / 4) || s2 >= s1;
        if !ok || s1 == 0 || (off as usize) + (s1 as usize) > data.len() {
            break;
        }
        entries.push((off, s1));
    }
    entries
}

fn missing_values(data: &[u8]) -> Vec<MissingValue> {
    if data.len() < HDR_STATUS_LEGEND + 14 {
        return Vec::new();
    }
    let dir_ptr = u32(data, HDR_STATUS_LEGEND);
    let n_entries = u32(data, HDR_STATUS_LEGEND + 8);
    if dir_ptr == 0 || n_entries < 1 {
        return Vec::new();
    }
    let entries = status_legend_dir(data, dir_ptr, n_entries);
    if entries.is_empty() {
        return Vec::new();
    }
    let (idx_off, idx_len) = entries[0];
    if idx_off == 0 || idx_len < 6 {
        return Vec::new();
    }
    let index = &data[idx_off as usize..idx_off as usize + idx_len as usize];
    if index[..2] != *b"\x04\x02" {
        return Vec::new();
    }
    let n_codes = u16(index, 2) as usize;
    let per_code = u16(index, 4) as usize;
    if n_codes < 1 || per_code < 1 {
        return Vec::new();
    }
    let need = 6 + n_codes * per_code * 4;
    if index.len() < need {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut pos = 6;
    for code in 1..=n_codes {
        let mut eidxs = Vec::new();
        for j in 0..per_code {
            eidxs.push(u32(index, pos + 4 * j) as usize);
        }
        pos += 4 * per_code;
        let mut labels: Vec<Option<String>> = Vec::new();
        let mut symbol = None;
        for ei in eidxs {
            if ei >= entries.len() {
                labels.push(None);
                continue;
            }
            let (off, length) = entries[ei];
            if off == 0 || length < 1 {
                labels.push(None);
                continue;
            }
            match parse_status_legend_record(
                &data[off as usize..off as usize + length as usize],
            ) {
                Some((sym, text)) => {
                    if symbol.is_none() {
                        symbol = Some(sym);
                    }
                    labels.push(Some(text));
                }
                None => labels.push(None),
            }
        }
        if symbol.is_none() || !labels.iter().any(|l| l.is_some()) {
            continue;
        }
        let en = labels.first().cloned().flatten();
        let fr = if labels.len() > 1 {
            labels[1].clone()
        } else {
            None
        };
        out.push(MissingValue {
            symbol: symbol.unwrap(),
            label: LocalizedText { en, fr },
            code: Some(code as i64),
        });
    }
    out
}

fn signature_hex(sig: &[u8; 4]) -> String {
    format!("{:02x}{:02x}{:02x}{:02x}", sig[0], sig[1], sig[2], sig[3])
}

pub fn build_dimensions(data: &[u8], desc: &Descriptor) -> Vec<Dimension> {
    let gd = geo_dim_index(&desc.dims);
    let mut dims = Vec::new();
    for (i, d) in desc.dims.iter().enumerate() {
        let is_geo = (i + 1) == gd;
        let (members, members_fr) = if !is_geo {
            dim_dir_labels(data, d, i, false)
        } else {
            (None, None)
        };
        let (short, description, description_fr) = dim_title(data, i);
        // Prefer the dim-title short code (COOKIE) over a long descriptor name
        // (COOKIE TYPE) when available; keep descriptor name otherwise.
        let name = short
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| d.name.clone());
        let description = description.or_else(|| Some(name.clone()));
        let mut name_fr = description_fr.clone();
        if is_geo && name.eq_ignore_ascii_case("Geography") && name_fr.is_none() {
            name_fr = Some("Géographie".into());
        }
        dims.push(Dimension {
            name,
            name_fr,
            count: d.count,
            type_code: d.type_code,
            is_geography: is_geo,
            description,
            description_fr,
            type_label: type_label_for(d),
            members,
            members_fr,
            ordinal: None,
        });
    }
    dims
}

pub fn build_metadata(
    data: Vec<u8>,
    desc: Option<Descriptor>,
    hdr: Option<HeaderLayout>,
) -> Result<IvtMetadata, String> {
    let hdr = match hdr {
        Some(h) => h,
        None => read_header(&data)?,
    };
    if hdr.signature != [0x04, 0x00, 0x20, 0x00] && hdr.signature != [0x02, 0x00, 0x20, 0x00] {
        return Err(format!(
            "unrecognised IVT signature: {}",
            signature_hex(&hdr.signature)
        ));
    }
    let desc = match desc {
        Some(d) => d,
        None => parse_descriptor(&data).ok_or("could not parse IVT dimension descriptor")?,
    };

    let info = parse_table_identity(&data);
    let (notes_en, notes_fr, notes_pid) = parse_identity_notes(&data);
    let product_id = info
        .get("product_id")
        .cloned()
        .flatten()
        .or(notes_pid);
    let keywords = parse_keywords(&data, product_id.as_deref());
    let number_of_datapoints = if desc.dims.is_empty() {
        0
    } else {
        desc.dims.iter().fold(1i64, |a, d| a.saturating_mul(d.count))
    };
    let time_series = time_series_label(
        &data,
        &desc.dims,
        notes_en.as_deref(),
        notes_fr.as_deref(),
    );

    Ok(IvtMetadata {
        product_id,
        title: LocalizedText {
            en: info.get("title_en").cloned().flatten(),
            fr: info.get("title_fr").cloned().flatten(),
        },
        universe: info.get("universe").cloned().flatten(),
        notes: LocalizedText {
            en: notes_en,
            fr: notes_fr,
        },
        version: hdr.version,
        signature: signature_hex(&hdr.signature),
        author: LocalizedText {
            en: info.get("author_en").cloned().flatten(),
            fr: info.get("author_fr").cloned().flatten(),
        },
        category: LocalizedText {
            en: info.get("category_en").cloned().flatten(),
            fr: info.get("category_fr").cloned().flatten(),
        },
        keywords,
        number_of_datapoints: Some(number_of_datapoints),
        time_series,
        read_only: Some(read_only_flag(&data)),
        data,
        desc,
        dimensions: None,
        geographies: None,
        footnotes: None,
        missing_values: None,
    })
}
