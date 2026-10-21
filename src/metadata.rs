//! File summary, dimension members, geographies, footnotes.

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::bytes::{latin1, rd_pascal, u16, u32};
use crate::descriptor::{
    geo_dim_index, parse_descriptor, scan_dim_marker_in_buffers, walk_at_window, Descriptor,
    DimDesc,
};
use crate::error::IvtError;
use crate::header::{
    dim_slots, read_block_dir, read_block_dir_bytes, read_header, read_header_ex, HeaderLayout,
    HDR_DESCRIPTOR_PTR, HDR_DIM_SLOT0, HDR_MASTER_PTR, HDR_STATUS_LEGEND,
    DIM_SLOT_STRIDE,
};
use crate::io::{ByteRange, IvtSource, LoadedRange, DEFAULT_MERGE_GAP};

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
/// Prefer parenthetical year spans like `(1980-1992)` over bare pairs in prose.
static TS_YEAR_SPAN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\((\d{4})\s*[-–—]\s*(\d{4})\)").unwrap());
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
/// OECD demo tables set read-only on byte 8 of the `81 01 20 00` master block.
const READONLY_FLAG_BIT_ALT: u8 = 0x02;

#[derive(Debug, Clone)]
pub struct Dimension {
    pub name_en: String,
    pub name_fr: Option<String>,
    pub count: i64,
    pub type_code: i64,
    /// Internal: geo dim flag for tidy export (not exposed to Python).
    pub is_geography: bool,
    pub description_en: Option<String>,
    pub description_fr: Option<String>,
    pub type_label: String,
    pub members_en: Option<Vec<String>>,
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

/// Parse intermediate for time-series bounds (flattened onto `IvtMetadata`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TimeSeriesInfo {
    frequency: Option<String>,
    start: Option<String>,
    end: Option<String>,
}

impl TimeSeriesInfo {
    fn into_fields(self) -> (Option<String>, Option<String>, Option<String>) {
        (self.frequency, self.start, self.end)
    }
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
    pub frequency: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub read_only: Option<bool>,
    source: IvtSource,
    desc: Descriptor,
    #[allow(dead_code)]
    master_entries: Vec<(u32, u16)>,
    dimensions: Option<Vec<Dimension>>,
    geographies: Option<Geographies>,
    footnotes: Option<Vec<Footnote>>,
    missing_values: Option<Vec<MissingValue>>,
}

impl IvtMetadata {
    pub fn dimensions(&mut self) -> Result<&Vec<Dimension>, String> {
        self.ensure_dimensions().map_err(|e| e.to_string())?;
        Ok(self.dimensions.as_ref().unwrap())
    }

    pub fn geographies(&mut self) -> Result<&Geographies, String> {
        self.ensure_geographies().map_err(|e| e.to_string())?;
        Ok(self.geographies.as_ref().unwrap())
    }

    pub fn footnotes(&mut self) -> Result<&Vec<Footnote>, String> {
        if self.footnotes.is_none() {
            self.footnotes = Some(footnotes_from_source(&self.source).map_err(|e| e.to_string())?);
        }
        Ok(self.footnotes.as_ref().unwrap())
    }

    pub fn missing_values(&mut self) -> Result<&Vec<MissingValue>, String> {
        if self.missing_values.is_none() {
            self.missing_values =
                Some(missing_values_from_source(&self.source).map_err(|e| e.to_string())?);
        }
        Ok(self.missing_values.as_ref().unwrap())
    }

    fn ensure_dimensions(&mut self) -> Result<(), IvtError> {
        if self.dimensions.is_some() {
            return Ok(());
        }
        let dims = build_dimensions_from_source(&self.source, &self.desc)?;
        self.dimensions = Some(dims);
        Ok(())
    }

    fn ensure_geographies(&mut self) -> Result<(), IvtError> {
        if self.geographies.is_some() {
            return Ok(());
        }
        let gd = geo_dim_index(&self.desc.dims);
        let geos = geo_from_source(&self.source, &self.desc.dims[gd - 1], gd - 1)?;
        self.geographies = Some(geos);
        Ok(())
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

    pub fn source(&self) -> &IvtSource {
        &self.source
    }

    pub fn io_metrics(&self) -> crate::io::IoMetrics {
        self.source.metrics()
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
    parse_facet_summary_bufs(&[data])
}

fn parse_facet_summary_bufs(bufs: &[&[u8]]) -> HashMap<&'static str, Option<String>> {
    let mut runs = Vec::new();
    for data in bufs {
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
    has_time_series_marker_bufs(&[data])
}

fn has_time_series_marker_bufs(bufs: &[&[u8]]) -> bool {
    for data in bufs {
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

fn month_index_fr(raw: &str) -> Option<u32> {
    let s = raw.to_lowercase();
    match s.as_str() {
        "janvier" | "jan" => Some(1),
        "février" | "fevrier" | "fév" | "fev" => Some(2),
        "mars" => Some(3),
        "avril" | "avr" => Some(4),
        "mai" => Some(5),
        "juin" => Some(6),
        "juillet" | "juil" => Some(7),
        "août" | "aout" => Some(8),
        "septembre" | "sept" | "sep" => Some(9),
        "octobre" | "oct" => Some(10),
        "novembre" | "nov" => Some(11),
        "décembre" | "decembre" | "déc" | "dec" => Some(12),
        _ => None,
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

fn add_months(year: u32, month: u32, add: i64) -> (u32, u32) {
    let idx = (year as i64) * 12 + (month as i64 - 1) + add;
    let y = (idx / 12) as u32;
    let m = (idx % 12 + 1) as u32;
    (y, m)
}

fn iso_ym(year: u32, month: u32) -> String {
    format!("{:04}-{:02}-01", year, month)
}

fn period_months_for_frequency(frequency: Option<&str>) -> i64 {
    let Some(f) = frequency else {
        return 1;
    };
    let lower = f.to_lowercase();
    if lower.contains("semi") {
        6
    } else if lower.contains("quarter") || lower.contains("trimestr") {
        3
    } else if lower.contains("annual") || lower.contains("year") || lower.contains("annuel") {
        12
    } else {
        1
    }
}

/// Year span `(1980-1992)` → Jan of start year; end from count×period when available.
fn year_span_bounds(
    text: &str,
    time_count: Option<i64>,
    period_months: i64,
) -> (Option<(u32, u32)>, Option<(u32, u32)>) {
    let Some(c) = TS_YEAR_SPAN_RE.captures(text) else {
        return (None, None);
    };
    let (Ok(y1), Ok(y2)) = (c[1].parse::<u32>(), c[2].parse::<u32>()) else {
        return (None, None);
    };
    if y2 < y1 {
        return (None, None);
    }
    let start = Some((y1, 1));
    let end = if let Some(count) = time_count {
        if count >= 1 {
            Some(add_months(y1, 1, (count - 1) * period_months))
        } else {
            Some((y2, 1))
        }
    } else {
        Some(add_months(y2, 1, period_months.saturating_sub(1)))
    };
    (start, end)
}

/// Parse a start/end year-month pair from notes or file text.
/// When `synthesize_end` is false, only explicit two-ended ranges are returned.
fn range_bounds_from_text(
    text: &str,
    time_count: Option<i64>,
    period_months: i64,
    synthesize_end: bool,
) -> (Option<(u32, u32)>, Option<(u32, u32)>) {
    let mut start = None;
    let mut end = None;
    if let Some(c) = TS_RANGE_EN_RE.captures(text) {
        let m1 = c.get(1).unwrap().as_str();
        let y1 = c.get(2).unwrap().as_str();
        if let (Some(mi), Ok(year)) = (month_index_en(m1), y1.parse::<u32>()) {
            start = Some((year, mi));
            if let (Some(m2), Some(y2)) = (c.get(3), c.get(4)) {
                if let (Some(emi), Ok(ey)) = (month_index_en(m2.as_str()), y2.as_str().parse::<u32>())
                {
                    end = Some((ey, emi));
                }
            } else if synthesize_end {
                if let Some(count) = time_count {
                    if count >= 1 {
                        end = Some(add_months(year, mi, (count - 1) * period_months));
                    }
                }
            }
        }
    }
    if start.is_none() {
        if let Some(c) = TS_RANGE_FR_RE.captures(text) {
            let m1 = c.get(1).unwrap().as_str();
            let y1 = c.get(2).unwrap().as_str();
            if let (Some(mi), Ok(year)) = (month_index_fr(m1), y1.parse::<u32>()) {
                start = Some((year, mi));
                if let (Some(m2), Some(y2)) = (c.get(3), c.get(4)) {
                    if let (Some(emi), Ok(ey)) =
                        (month_index_fr(m2.as_str()), y2.as_str().parse::<u32>())
                    {
                        end = Some((ey, emi));
                    }
                } else if synthesize_end {
                    if let Some(count) = time_count {
                        if count >= 1 {
                            end = Some(add_months(year, mi, (count - 1) * period_months));
                        }
                    }
                }
            }
        }
    }
    (start, end)
}

fn parse_time_series(
    marker_bufs: &[&[u8]],
    searchable_text: &str,
    dims: &[DimDesc],
    notes_en: Option<&str>,
    notes_fr: Option<&str>,
) -> Option<TimeSeriesInfo> {
    let dim = time_dim_desc(dims);
    if dim.is_none() && !has_time_series_marker_bufs(marker_bufs) {
        return None;
    }
    let mut frequency = None;
    for text in [notes_en, notes_fr, Some(searchable_text)]
        .into_iter()
        .flatten()
    {
        if let Some((e, _)) = freq_pair_from_text(text) {
            frequency = Some(e);
            break;
        }
    }
    if frequency.is_none() {
        if let Some(dim) = dim {
            if let Some(f) = frequency_hint_name(&dim.name) {
                let pair = freq_pair_from_text(&f).unwrap_or_else(|| (f.clone(), f));
                frequency = Some(pair.0);
            }
        }
    }
    let time_count = dim.map(|d| d.count);
    let period_months = period_months_for_frequency(frequency.as_deref());
    let texts = [notes_en, notes_fr, Some(searchable_text)];
    let mut start = None;
    let mut end = None;

    // Prefer year spans (e.g. OECD `(1980-1992)`) over lone publication months.
    for text in texts.into_iter().flatten() {
        let (s, e) = year_span_bounds(text, time_count, period_months);
        if start.is_none() {
            start = s;
        }
        if end.is_none() {
            end = e;
        }
        if start.is_some() && end.is_some() {
            break;
        }
    }

    // Explicit two-ended month ranges (e.g. Products January 1991 … March 1994).
    if start.is_none() || end.is_none() {
        for text in texts.into_iter().flatten() {
            let (s, e) = range_bounds_from_text(text, time_count, period_months, false);
            if start.is_none() {
                start = s;
            }
            if end.is_none() {
                end = e;
            }
            if start.is_some() && end.is_some() {
                break;
            }
        }
    }

    // Last resort: single month-year + count scaled by frequency.
    if start.is_none() || end.is_none() {
        for text in texts.into_iter().flatten() {
            let (s, e) = range_bounds_from_text(text, time_count, period_months, true);
            if start.is_none() {
                start = s;
            }
            if end.is_none() {
                end = e;
            }
            if start.is_some() && end.is_some() {
                break;
            }
        }
    }

    Some(TimeSeriesInfo {
        frequency,
        start: start.map(|(y, m)| iso_ym(y, m)),
        end: end.map(|(y, m)| iso_ym(y, m)),
    })
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

fn flag_block_read_only(blk: &[u8]) -> bool {
    if blk.len() < 5 || &blk[..4] != *b"\x81\x01\x20\x00" {
        return false;
    }
    if (blk[4] & READONLY_FLAG_BIT) != 0 {
        return true;
    }
    blk.len() > 8 && (blk[8] & READONLY_FLAG_BIT_ALT) != 0
}

fn read_only_flag(data: &[u8]) -> bool {
    for (off, length) in master_dir_entries(data, 32) {
        if length < 5 {
            continue;
        }
        let blk = &data[off as usize..off as usize + length as usize];
        if flag_block_read_only(blk) {
            return true;
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
        let rest = &glued[code.len()..];
        // FR codes are typically uppercase tokens (RÉGION, TEMPS). Reject sentence
        // tails like " being reportedUnités" from Sample.ivt.
        let rest_ok = !rest.is_empty()
            && !rest.starts_with(' ')
            && rest
                .chars()
                .next()
                .map(|c| !c.is_ascii_lowercase())
                .unwrap_or(false);
        if rest_ok {
            let en = if glued[..code.len()]
                .chars()
                .any(|c| c.is_ascii_lowercase())
            {
                // Prefer the title-case English spelling from the glued run.
                glued[..code.len()].to_string()
            } else {
                code.to_string()
            };
            return (en, Some(rest.to_string()));
        }
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

/// Bare `0x32` is also ASCII `'2'`; only treat it as a title separator when it
/// sits between a letter (end of CODE/DESC) and a following letter (next label).
fn is_bare_dim_title_sep(block: &[u8], at: usize) -> bool {
    if at >= block.len() || block[at] != 0x32 {
        return false;
    }
    if at == 0 || !(block[at - 1] as char).is_ascii_alphabetic() {
        return false;
    }
    let next = at + 1;
    if next >= block.len() {
        return true;
    }
    let n = block[next];
    (65..=90).contains(&n) || (97..=122).contains(&n)
}

/// Length of a language/title marker ending at `0x32`: bare, `lang 32`, or `lang len 32`.
fn dim_title_marker_len(block: &[u8], at: usize) -> Option<usize> {
    if at >= block.len() {
        return None;
    }
    if at + 1 < block.len() && (1..=9).contains(&block[at]) && block[at + 1] == 0x32 {
        return Some(2);
    }
    if at + 2 < block.len() && (1..=9).contains(&block[at]) && block[at + 2] == 0x32 {
        return Some(3);
    }
    if is_bare_dim_title_sep(block, at) {
        return Some(1);
    }
    None
}

fn dim_title_next_marker(block: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i < block.len() {
        if let Some(n) = dim_title_marker_len(block, i) {
            // Lang-prefixed markers use control bytes; bare 0x32 is printable.
            if n >= 2 || is_bare_dim_title_sep(block, i) {
                return Some(i);
            }
        }
        if block[i] < 32 && dim_title_marker_len(block, i).is_none() {
            return None;
        }
        i += 1;
    }
    None
}

fn trim_desc_before_next_code(desc: &str) -> String {
    // OECD glues DESC+NEXT_CODE (`RegionRÉGION`); keep title-case / mixed DESC only.
    let bytes = desc.as_bytes();
    for i in 1..desc.len() {
        if bytes[i - 1].is_ascii_lowercase() && bytes[i].is_ascii_uppercase() {
            return desc[..i].to_string();
        }
    }
    desc.to_string()
}

/// Demo / OECD title block: CODE [lang [len]] 32 DESC [CODE_FR…] [lang [len]] 32 DESC_FR …
/// Returns (name_en, name_fr, description_en, description_fr).
fn parse_dim_title_demo(
    block: &[u8],
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    // Locate the first CODE immediately before a title marker.
    // Prefer lang[+len]+0x32 (Products/OECD) over bare 0x32 (OECD TIME PERIOD).
    let mut code_end = None;
    let mut marker_len = 0usize;
    let code_is_dim_code = |code: &str| {
        code.chars().any(|c| c.is_ascii_uppercase())
            && !code.chars().any(|c| c.is_ascii_lowercase())
    };
    for min_marker in [2usize, 1] {
        for i in 0..block.len() {
            let Some(n) = dim_title_marker_len(block, i) else {
                continue;
            };
            if n < min_marker {
                continue;
            }
            let mut start = i;
            while start > 0 && (32..=126).contains(&block[start - 1]) {
                start -= 1;
            }
            let code = latin1(&block[start..i]).trim().to_string();
            if code.len() < 2
                || !code
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_alphabetic())
                    .unwrap_or(false)
                || !code.chars().any(|c| c.is_ascii_uppercase())
            {
                continue;
            }
            // lang+len+0x32 with lang≠1 is used by some demos (People UNITS) for
            // ALL-CAPS codes, but Sample "Units\x02\x04\x32Units being reported…"
            // is a sentence description — skip mixed-case codes there.
            if n == 3 && block[i] != 1 && !code_is_dim_code(&code) {
                continue;
            }
            code_end = Some((start, i, code));
            marker_len = n;
            break;
        }
        if code_end.is_some() {
            break;
        }
    }
    let Some((_start, rel, code)) = code_end else {
        return (None, None, None, None);
    };
    let p = rel + marker_len;
    let end = dim_title_next_marker(block, p).unwrap_or_else(|| {
        let mut e = p;
        while e < block.len() && block[e] >= 32 {
            e += 1;
        }
        e
    });
    let glued = latin1(&block[p..end]);
    let (desc_en_raw, code_fr) = split_en_fr_glued(&code, &glued);
    let desc_en = if desc_en_raw.is_empty() {
        code.clone()
    } else {
        // When split kept a glued FR code via case transition, desc is already clean;
        // when it used code-prefix split, desc_en is the EN portion.
        desc_en_raw
    };

    let mut description_fr = code_fr.clone();
    let mut name_fr = code_fr.clone();
    // Second language segment: DESC_FR (and possibly further glued codes).
    if let Some(m2) = dim_title_next_marker(block, p) {
        if let Some(n2) = dim_title_marker_len(block, m2) {
            let p2 = m2 + n2;
            let end2 = dim_title_next_marker(block, p2).unwrap_or_else(|| {
                let mut e = p2;
                while e < block.len() && block[e] >= 32 {
                    e += 1;
                }
                e
            });
            if p2 < end2 {
                let raw_fr = latin1(&block[p2..end2]);
                let fr_desc = trim_desc_before_next_code(&raw_fr);
                // Prefer second-segment text only when it looks like a label
                // (has lowercase). Products often repeats an ALL-CAPS FR code here
                // and may glue Spanish without a clean separator.
                if !fr_desc.is_empty() && fr_desc.chars().any(|c| c.is_lowercase()) {
                    description_fr = Some(fr_desc);
                }
            }
        }
    }

    // COOKIE TYPE → Name COOKIE; keep TIME PERIOD / REGION as-is.
    let short = if code.to_ascii_uppercase().ends_with(" TYPE") {
        code.split_whitespace()
            .next()
            .unwrap_or(&code)
            .to_string()
    } else {
        code.clone()
    };

    // Products REGION: glued REGIONRÉGION → desc_en=REGION, name_fr=RÉGION, second seg RÉGION.
    // OECD REGION: glued RegionRÉGION → desc_en=Region, name_fr=RÉGION, second seg Région.
    if name_fr.is_none() {
        name_fr = description_fr.clone();
    }

    (Some(short), name_fr, Some(desc_en), description_fr)
}

/// Returns (name_en, name_fr, description_en, description_fr).
fn parse_dim_title(
    block: &[u8],
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    if block.len() < 16 || block[..2] != *b"\x81\x02" {
        return (None, None, None, None);
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
        let name_fr = description_fr.clone();
        return (Some(short), name_fr, Some(description), description_fr);
    }
    (None, None, None, None)
}

fn dim_title(
    data: &[u8],
    slot_index: usize,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let slots = dim_slots(data, 32);
    if slot_index >= slots.len() {
        return (None, None, None, None);
    }
    let slot = slots[slot_index];
    for (off, length) in read_block_dir(data, slot.dir_ptr, slot.n_entries) {
        if length < 16 {
            continue;
        }
        let parsed = parse_dim_title(&data[off as usize..off as usize + length as usize]);
        if parsed.0.is_some() || parsed.2.is_some() {
            return parsed;
        }
    }
    (None, None, None, None)
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
    footnotes_regions(&data[..data.len().min(12000)], &data[start..])
}

fn footnotes_regions(head: &[u8], tail: &[u8]) -> Vec<Footnote> {
    let regions: [&[u8]; 2] = [head, tail];
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
        let (members_en, members_fr) = if !is_geo {
            dim_dir_labels(data, d, i, false)
        } else {
            (None, None)
        };
        let (short, title_name_fr, description_en, description_fr) = dim_title(data, i);
        // Prefer the dim-title short code (COOKIE) over a long descriptor name
        // (COOKIE TYPE) when available; keep descriptor name otherwise.
        let name_en = short
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| d.name.clone());
        let description_en = description_en.or_else(|| Some(name_en.clone()));
        let mut name_fr = title_name_fr.or_else(|| description_fr.clone());
        if is_geo && name_en.eq_ignore_ascii_case("Geography") && name_fr.is_none() {
            name_fr = Some("Géographie".into());
        }
        dims.push(Dimension {
            name_en,
            name_fr,
            count: d.count,
            type_code: d.type_code,
            is_geography: is_geo,
            description_en,
            description_fr,
            type_label: type_label_for(d),
            members_en,
            members_fr,
            ordinal: None,
        });
    }
    dims
}

/// Oracle / bytes path: build metadata from a complete in-memory IVT.
pub fn build_metadata(
    data: Vec<u8>,
    desc: Option<Descriptor>,
    hdr: Option<HeaderLayout>,
) -> Result<IvtMetadata, String> {
    let source = IvtSource::from_bytes(data);
    build_metadata_from_source(source, desc, hdr).map_err(|e| e.to_string())
}

/// Path-based entry: open a file without reading it entirely for summary metadata.
pub fn open_metadata_path(path: &Path) -> Result<IvtMetadata, IvtError> {
    let source = IvtSource::open_file(path)?;
    // Small files: one full read matches the legacy oracle and is cheaper than
    // many tiny range requests. Large files use selective range I/O.
    const SMALL_FILE: u64 = 2 * 1024 * 1024;
    if source.len()? <= SMALL_FILE {
        let data = source.read_all()?;
        let mut metrics = source.metrics();
        // Attribute the full-file load for benchmarks (one logical read).
        if metrics.read_ops == 0 {
            metrics.record(data.len());
        }
        return build_metadata_from_source(
            IvtSource::from_bytes_with_metrics(data, metrics),
            None,
            None,
        );
    }
    build_metadata_from_source(source, None, None)
}

pub fn build_metadata_from_source(
    source: IvtSource,
    desc: Option<Descriptor>,
    hdr: Option<HeaderLayout>,
) -> Result<IvtMetadata, IvtError> {
    if let Some(data) = source.memory_arc() {
        return build_metadata_memory(data.as_ref(), source, desc, hdr);
    }
    build_metadata_selective(source, desc, hdr)
}

fn build_metadata_memory(
    data: &[u8],
    source: IvtSource,
    desc: Option<Descriptor>,
    hdr: Option<HeaderLayout>,
) -> Result<IvtMetadata, IvtError> {
    let hdr = match hdr {
        Some(h) => h,
        None => read_header(data).map_err(IvtError::Parse)?,
    };
    if hdr.signature != [0x04, 0x00, 0x20, 0x00] && hdr.signature != [0x02, 0x00, 0x20, 0x00] {
        return Err(IvtError::InvalidSignature(hdr.signature));
    }
    let desc = match desc {
        Some(d) => d,
        None => parse_descriptor(data).ok_or(IvtError::DescriptorNotFound)?,
    };
    let master_entries = master_dir_entries(data, 32);
    let info = parse_table_identity(data);
    let (notes_en, notes_fr, notes_pid) = parse_identity_notes(data);
    let product_id = info.get("product_id").cloned().flatten().or(notes_pid);
    let keywords = parse_keywords(data, product_id.as_deref());
    let number_of_datapoints = if desc.dims.is_empty() {
        0
    } else {
        desc.dims.iter().fold(1i64, |a, d| a.saturating_mul(d.count))
    };
    let searchable = latin1(data);
    let (frequency, start_date, end_date) = parse_time_series(
        &[data],
        &searchable,
        &desc.dims,
        notes_en.as_deref(),
        notes_fr.as_deref(),
    )
    .map(TimeSeriesInfo::into_fields)
    .unwrap_or((None, None, None));
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
        frequency,
        start_date,
        end_date,
        read_only: Some(read_only_flag(data)),
        source,
        desc,
        master_entries,
        dimensions: None,
        geographies: None,
        footnotes: None,
        missing_values: None,
    })
}

fn build_metadata_selective(
    source: IvtSource,
    desc: Option<Descriptor>,
    hdr: Option<HeaderLayout>,
) -> Result<IvtMetadata, IvtError> {
    let file_len = source.len()?;
    let prefix_len = (HDR_DIM_SLOT0 + DIM_SLOT_STRIDE * 32).max(2048);
    let prefix = source.read_prefix(prefix_len.min(file_len as usize))?;
    let hdr = match hdr {
        Some(h) => h,
        None => read_header_ex(&prefix, file_len as usize).map_err(IvtError::Parse)?,
    };
    if hdr.signature != [0x04, 0x00, 0x20, 0x00] && hdr.signature != [0x02, 0x00, 0x20, 0x00] {
        return Err(IvtError::InvalidSignature(hdr.signature));
    }

    let master = u32(&prefix, HDR_MASTER_PTR) as u64;
    let mut ranges = vec![
        ByteRange::new(0, prefix.len()),
        ByteRange::new(master, 256),
    ];
    let d_off = u32(&prefix, HDR_DESCRIPTOR_PTR) as u64;
    if d_off > 0 && d_off < file_len {
        let wlen = 4000u64.min(file_len - d_off) as usize;
        ranges.push(ByteRange::new(d_off, wlen));
    }
    if hdr.title_en != 0 && (hdr.title_en as u64) < file_len {
        let len = 512usize.min((file_len - hdr.title_en as u64) as usize);
        ranges.push(ByteRange::new(hdr.title_en as u64, len));
    }
    if hdr.title_fr != 0 && (hdr.title_fr as u64) < file_len {
        let len = 512usize.min((file_len - hdr.title_fr as u64) as usize);
        ranges.push(ByteRange::new(hdr.title_fr as u64, len));
    }

    // Peek master directory to plan block reads.
    let master_dir = source.read_range(master, 256.min((file_len - master) as usize))?;
    let master_entries = master_dir_entries_from_buf(&prefix, &master_dir, master, file_len as usize);
    for &(off, len) in &master_entries {
        ranges.push(ByteRange::new(off as u64, len as usize));
    }

    let loaded = source.load_ranges(&ranges, DEFAULT_MERGE_GAP)?;
    let head = slice_or_read(&source, &loaded, 0, prefix.len().min(8000))?;
    let block_bufs: Vec<&[u8]> = master_entries
        .iter()
        .filter_map(|&(off, len)| find_loaded(&loaded, off as u64, len as usize))
        .collect();

    let desc = match desc {
        Some(d) => d,
        None => match parse_descriptor_selective(&source, &prefix, &loaded, d_off, &block_bufs)
        {
            Ok(d) => d,
            Err(_) => {
                // Demo tables sometimes need a full-file marker scan for the dim chain.
                source.set_fallback_used();
                let all = source.read_all()?;
                parse_descriptor(&all).ok_or(IvtError::DescriptorNotFound)?
            }
        },
    };

    let mut facet = parse_facet_summary_bufs(&block_bufs);
    let mut info = parse_table_identity_selective(&head, &block_bufs, &facet);
    let (notes_en, notes_fr, notes_pid) = parse_identity_notes_bufs(&block_bufs);
    let mut product_id = info.get("product_id").cloned().flatten().or(notes_pid);
    let keywords = parse_keywords_bufs(&block_bufs, product_id.as_deref());

    // Fallback: full-file FACET/identity only when summary fields are missing.
    let need_fallback = info.get("title_en").and_then(|t| t.as_ref()).is_none()
        && info.get("title_fr").and_then(|t| t.as_ref()).is_none()
        && facet.get("title_en").and_then(|t| t.as_ref()).is_none()
        && facet.get("title_fr").and_then(|t| t.as_ref()).is_none();
    let (searchable, marker_owned): (String, Option<Vec<u8>>) = if need_fallback {
        source.set_fallback_used();
        let all = source.read_all()?;
        facet = parse_facet_summary(&all);
        info = parse_table_identity(&all);
        product_id = info
            .get("product_id")
            .cloned()
            .flatten()
            .or(product_id);
        (latin1(&all), Some(all))
    } else {
        let mut parts = Vec::new();
        parts.push(latin1(&head));
        for b in &block_bufs {
            parts.push(latin1(b));
        }
        if let (Some(ne), Some(nf)) = (notes_en.as_ref(), notes_fr.as_ref()) {
            parts.push(ne.clone());
            parts.push(nf.clone());
        } else if let Some(ne) = notes_en.as_ref() {
            parts.push(ne.clone());
        } else if let Some(nf) = notes_fr.as_ref() {
            parts.push(nf.clone());
        }
        (parts.join("\n"), None)
    };

    let marker_refs: Vec<&[u8]> = if let Some(ref all) = marker_owned {
        vec![all.as_slice()]
    } else {
        block_bufs.clone()
    };

    let number_of_datapoints = if desc.dims.is_empty() {
        0
    } else {
        desc.dims.iter().fold(1i64, |a, d| a.saturating_mul(d.count))
    };
    let mut time_series = parse_time_series(
        &marker_refs,
        &searchable,
        &desc.dims,
        notes_en.as_deref(),
        notes_fr.as_deref(),
    );
    // Frequency/range strings may live outside master blocks. Full-text fallback
    // is only used when the file is still modest; large network files keep the
    // bounded searchable text to avoid a complete transfer.
    const TS_FALLBACK_MAX: u64 = 2 * 1024 * 1024;
    if let Some(ts) = &time_series {
        if (ts.start.is_none() || ts.end.is_none() || ts.frequency.is_none())
            && file_len <= TS_FALLBACK_MAX
        {
            source.set_fallback_used();
            let all = if let Some(ref owned) = marker_owned {
                owned.clone()
            } else {
                source.read_all()?
            };
            let full_text = latin1(&all);
            time_series = parse_time_series(
                &[&all],
                &full_text,
                &desc.dims,
                notes_en.as_deref(),
                notes_fr.as_deref(),
            );
        }
    }
    let (frequency, start_date, end_date) = time_series
        .map(TimeSeriesInfo::into_fields)
        .unwrap_or((None, None, None));
    let read_only = read_only_flag_bufs(&block_bufs);

    // Prefer FACET titles/authors when present (same as parse_table_identity).
    let title_en = facet
        .get("title_en")
        .cloned()
        .flatten()
        .or_else(|| info.get("title_en").cloned().flatten());
    let title_fr = facet
        .get("title_fr")
        .cloned()
        .flatten()
        .or_else(|| info.get("title_fr").cloned().flatten());

    Ok(IvtMetadata {
        product_id,
        title: LocalizedText {
            en: title_en,
            fr: title_fr,
        },
        universe: info.get("universe").cloned().flatten(),
        notes: LocalizedText {
            en: notes_en,
            fr: notes_fr,
        },
        version: hdr.version,
        signature: signature_hex(&hdr.signature),
        author: LocalizedText {
            en: facet
                .get("author_en")
                .cloned()
                .flatten()
                .or_else(|| info.get("author_en").cloned().flatten()),
            fr: facet
                .get("author_fr")
                .cloned()
                .flatten()
                .or_else(|| info.get("author_fr").cloned().flatten()),
        },
        category: LocalizedText {
            en: facet
                .get("category_en")
                .cloned()
                .flatten()
                .or_else(|| info.get("category_en").cloned().flatten()),
            fr: facet
                .get("category_fr")
                .cloned()
                .flatten()
                .or_else(|| info.get("category_fr").cloned().flatten()),
        },
        keywords,
        number_of_datapoints: Some(number_of_datapoints),
        frequency,
        start_date,
        end_date,
        read_only: Some(read_only),
        source,
        desc,
        master_entries,
        dimensions: None,
        geographies: None,
        footnotes: None,
        missing_values: None,
    })
}

fn find_loaded<'a>(loaded: &'a [LoadedRange], offset: u64, length: usize) -> Option<&'a [u8]> {
    for lr in loaded {
        if let Some(s) = lr.slice_at(offset, length) {
            return Some(s);
        }
    }
    None
}

fn slice_or_read(
    source: &IvtSource,
    loaded: &[LoadedRange],
    offset: u64,
    length: usize,
) -> Result<Vec<u8>, IvtError> {
    if let Some(s) = find_loaded(loaded, offset, length) {
        return Ok(s.to_vec());
    }
    source.read_range(offset, length)
}

fn master_dir_entries_from_buf(
    prefix: &[u8],
    master_dir: &[u8],
    master_off: u64,
    file_len: usize,
) -> Vec<(u32, u16)> {
    let _ = (prefix, master_off);
    let mut out = Vec::new();
    let mut o = 0usize;
    for _ in 0..32 {
        if o + 8 > master_dir.len() {
            break;
        }
        let off = u32(master_dir, o);
        let s1 = u16(master_dir, o + 4);
        o += 8;
        if off == 0 && s1 == 0 {
            continue;
        }
        if s1 == 0 || (off as usize) + (s1 as usize) > file_len {
            continue;
        }
        out.push((off, s1));
    }
    out
}

fn parse_descriptor_selective(
    source: &IvtSource,
    prefix: &[u8],
    loaded: &[LoadedRange],
    d_off: u64,
    block_bufs: &[&[u8]],
) -> Result<Descriptor, IvtError> {
    if !prefix.is_empty() && prefix[0] == 0x02 {
        return Err(IvtError::DescriptorNotFound);
    }
    let file_len = source.len()?;
    // Contiguous prefix covering the descriptor window matches oracle `walk_at`
    // (including FACET04 / dir-anchor truncation) without reading value pages.
    // Small files: full-buffer descriptor parse (includes marker-window fallback).
    // Large files: walk the header pointer window; marker scan uses master blocks
    // then a measured full-file fallback if needed.
    const SMALL_FILE: u64 = 2 * 1024 * 1024;
    if file_len <= SMALL_FILE {
        let all = source.read_all()?;
        return parse_descriptor(&all).ok_or(IvtError::DescriptorNotFound);
    }

    const MAX_PREFIX: u64 = 8 * 1024 * 1024;
    let need = (d_off.saturating_add(4000)).min(file_len);
    let mut best: Option<(u32, Vec<DimDesc>, i64)> = None;
    let consider = |best: &mut Option<(u32, Vec<DimDesc>, i64)>, off: u32, dims: Vec<DimDesc>| {
        let score = if dims.len() < 2 || dims.len() > 16 {
            -1
        } else {
            let prod = dims.iter().fold(1i64, |a, d| a.saturating_mul(d.count));
            (dims.len() as i64) * 1_000_000 + prod.min(999_999)
        };
        if score < 0 {
            return;
        }
        if best.as_ref().map(|(_, _, s)| *s).unwrap_or(-1) < score {
            *best = Some((off, dims, score));
        }
    };

    if need > d_off && need <= MAX_PREFIX {
        let buf = slice_or_read(source, loaded, 0, need as usize)?;
        let d_off_usize = d_off as usize;
        if d_off_usize + 18 <= buf.len() {
            let anchor = crate::header::dir_anchor_header(&buf);
            if let Some(dims) =
                walk_at_window(&buf[d_off_usize..], d_off_usize, anchor, 16)
            {
                consider(&mut best, d_off as u32, dims);
            }
        }
    }
    if let Some((off, dims)) = scan_dim_marker_in_buffers(block_bufs) {
        consider(&mut best, off as u32, dims);
    }
    if let Some((offset, dims, _)) = best {
        return Ok(Descriptor { offset, dims });
    }
    Err(IvtError::DescriptorNotFound)
}

fn parse_table_identity_selective(
    head: &[u8],
    block_bufs: &[&[u8]],
    facet: &HashMap<&'static str, Option<String>>,
) -> HashMap<&'static str, Option<String>> {
    let head_text = latin1(head);
    let grab = |re: &Regex| {
        re.captures(&head_text)
            .map(|m| m.get(1).unwrap().as_str().trim().to_string())
    };
    let mut info: HashMap<&'static str, Option<String>> = HashMap::new();
    info.insert("product_id", grab(&PRODUCT_ID_HEAD));
    info.insert("title_en", grab(&TITLE_EN_HEAD));
    info.insert("title_fr", grab(&TITLE_FR_HEAD));
    info.insert("universe", grab(&UNIVERSE_HEAD));

    let mut blobs = Vec::new();
    for blk in block_bufs {
        if blk.len() >= 2 && &blk[..2] == b"\x01\x01" && blk.len() > 8 {
            let payload = &blk[4..];
            let mut i = 0;
            while i < payload.len() && payload[i] < 0x20 {
                i += 1;
            }
            let text = latin1(&payload[i..]).trim().to_string();
            if !text.is_empty() {
                blobs.push(text);
            }
        } else if !blk.is_empty() && (blk[0] == b' ' || blk[0].is_ascii_alphabetic()) {
            let text = latin1(blk).trim().to_string();
            if text.contains("98-") || text.contains("Title") {
                blobs.push(text);
            }
        }
    }

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
        if let Some(m) = PID_FALLBACK.captures(&head_text) {
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

fn parse_identity_notes_bufs(
    block_bufs: &[&[u8]],
) -> (Option<String>, Option<String>, Option<String>) {
    let mut notes_en = None;
    let mut notes_fr = None;
    let mut product_id = None;
    let mut long_blobs = Vec::new();
    for blk in block_bufs {
        if blk.len() < 2 || &blk[..2] != b"\x01\x01" || blk.len() < 40 {
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

fn parse_keywords_bufs(block_bufs: &[&[u8]], product_id: Option<&str>) -> LocalizedText {
    let mut candidates = Vec::new();
    for blk in block_bufs {
        let length = blk.len() as u16;
        if !(8..=40).contains(&length) {
            continue;
        }
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
    // Reuse keyword assembly via a temporary in-memory parse path.
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

fn read_only_flag_bufs(block_bufs: &[&[u8]]) -> bool {
    for blk in block_bufs {
        if flag_block_read_only(blk) {
            return true;
        }
    }
    false
}

fn footnotes_from_source(source: &IvtSource) -> Result<Vec<Footnote>, IvtError> {
    if let Some(data) = source.as_memory_slice() {
        return Ok(footnotes(data));
    }
    let file_len = source.len()? as usize;
    // Match legacy region selection: for files ≤ 200 KiB the "tail" is the whole file.
    let tail_start = file_len.saturating_sub(200_000);
    if tail_start == 0 {
        let data = source.read_range(0, file_len)?;
        return Ok(footnotes(&data));
    }
    let head_len = file_len.min(12_000);
    let ranges = [
        ByteRange::new(0, head_len),
        ByteRange::new(tail_start as u64, file_len - tail_start),
    ];
    let loaded = source.load_ranges_default(&ranges)?;
    let head = find_loaded(&loaded, 0, head_len)
        .map(|s| s.to_vec())
        .unwrap_or_default();
    let tail = find_loaded(&loaded, tail_start as u64, file_len - tail_start)
        .map(|s| s.to_vec())
        .unwrap_or_default();
    Ok(footnotes_regions(&head, &tail))
}

fn missing_values_from_source(source: &IvtSource) -> Result<Vec<MissingValue>, IvtError> {
    if let Some(data) = source.as_memory_slice() {
        return Ok(missing_values(data));
    }
    let file_len = source.len()? as usize;
    if file_len < HDR_STATUS_LEGEND + 14 {
        return Ok(Vec::new());
    }
    let hdr = source.read_range(HDR_STATUS_LEGEND as u64, 14)?;
    let dir_ptr = u32(&hdr, 0);
    let n_entries = u32(&hdr, 8);
    if dir_ptr == 0 || n_entries < 1 {
        return Ok(Vec::new());
    }
    let dir_bytes = source.read_range(
        dir_ptr as u64,
        (n_entries as usize).saturating_mul(8).saturating_add(64),
    )?;
    let entries = status_legend_dir_bytes(&dir_bytes, n_entries, file_len);
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let (idx_off, idx_len) = entries[0];
    if idx_off == 0 || idx_len < 6 {
        return Ok(Vec::new());
    }
    let mut ranges = vec![ByteRange::new(idx_off as u64, idx_len as usize)];
    for &(off, len) in entries.iter().skip(1) {
        if off != 0 && len > 0 {
            ranges.push(ByteRange::new(off as u64, len as usize));
        }
    }
    let loaded = source.load_ranges_default(&ranges)?;
    let index = find_loaded(&loaded, idx_off as u64, idx_len as usize)
        .ok_or(IvtError::InvalidDirectory("status legend index"))?;
    if index[..2] != *b"\x04\x02" {
        return Ok(Vec::new());
    }
    let n_codes = u16(index, 2) as usize;
    let per_code = u16(index, 4) as usize;
    if n_codes < 1 || per_code < 1 {
        return Ok(Vec::new());
    }
    let need = 6 + n_codes * per_code * 4;
    if index.len() < need {
        return Ok(Vec::new());
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
            let Some(blob) = find_loaded(&loaded, off as u64, length as usize) else {
                labels.push(None);
                continue;
            };
            match parse_status_legend_record(blob) {
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
    Ok(out)
}

fn status_legend_dir_bytes(dir: &[u8], n_entries: u32, file_len: usize) -> Vec<(u32, u16)> {
    let mut entries = Vec::new();
    let mut o = 0usize;
    for _ in 0..n_entries {
        if o + 8 > dir.len() {
            break;
        }
        let off = u32(dir, o);
        let s1 = u16(dir, o + 4);
        let s2 = u16(dir, o + 6);
        o += 8;
        if off == 0 && s1 == 0 {
            entries.push((0, 0));
            continue;
        }
        let ok = s1 == s2 || s2 == 4 * ((s1 + 3) / 4) || s2 >= s1;
        if !ok || s1 == 0 || (off as usize) + (s1 as usize) > file_len {
            break;
        }
        entries.push((off, s1));
    }
    entries
}

fn build_dimensions_from_source(
    source: &IvtSource,
    desc: &Descriptor,
) -> Result<Vec<Dimension>, IvtError> {
    if let Some(data) = source.as_memory_slice() {
        return Ok(build_dimensions(data, desc));
    }
    let gd = geo_dim_index(&desc.dims);
    let mut dims = Vec::new();
    for (i, d) in desc.dims.iter().enumerate() {
        let is_geo = (i + 1) == gd;
        let (members_en, members_fr) = if !is_geo {
            dim_dir_labels_src(source, d, i, false)?
        } else {
            (None, None)
        };
        let (short, title_name_fr, description_en, description_fr) = dim_title_src(source, i)?;
        let name_en = short
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| d.name.clone());
        let description_en = description_en.or_else(|| Some(name_en.clone()));
        let mut name_fr = title_name_fr.or_else(|| description_fr.clone());
        if is_geo && name_en.eq_ignore_ascii_case("Geography") && name_fr.is_none() {
            name_fr = Some("Géographie".into());
        }
        dims.push(Dimension {
            name_en,
            name_fr,
            count: d.count,
            type_code: d.type_code,
            is_geography: is_geo,
            description_en,
            description_fr,
            type_label: type_label_for(d),
            members_en,
            members_fr,
            ordinal: None,
        });
    }
    Ok(dims)
}

fn geo_from_source(
    source: &IvtSource,
    dim: &DimDesc,
    slot_index: usize,
) -> Result<Geographies, IvtError> {
    if let Some(data) = source.as_memory_slice() {
        return Ok(geo_from_dir(data, dim, slot_index));
    }
    let (en, fr) = dim_dir_labels_src(source, dim, slot_index, true)?;
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
    Ok(geo)
}

fn dim_slot_from_source(source: &IvtSource, slot_index: usize) -> Result<Option<crate::header::DimSlot>, IvtError> {
    let base = HDR_DIM_SLOT0 + DIM_SLOT_STRIDE * slot_index;
    let file_len = source.len()? as usize;
    if base + DIM_SLOT_STRIDE > file_len {
        return Ok(None);
    }
    let bytes = source.read_range(base as u64, DIM_SLOT_STRIDE)?;
    let dir_ptr = u32(&bytes, 0);
    let alloc = u32(&bytes, 4);
    let n_entries = u32(&bytes, 8);
    let flag = u16(&bytes, 12);
    if dir_ptr == 0 && alloc == 0 && n_entries == 0 {
        return Ok(None);
    }
    Ok(Some(crate::header::DimSlot {
        index: slot_index,
        dir_ptr,
        alloc,
        n_entries,
        flag,
    }))
}

fn dim_title_src(
    source: &IvtSource,
    slot_index: usize,
) -> Result<
    (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ),
    IvtError,
> {
    if let Some(data) = source.as_memory_slice() {
        return Ok(dim_title(data, slot_index));
    }
    let Some(slot) = dim_slot_from_source(source, slot_index)? else {
        return Ok((None, None, None, None));
    };
    let file_len = source.len()? as usize;
    let dir_bytes = source.read_range(
        slot.dir_ptr as u64,
        (slot.n_entries as usize + 8).saturating_mul(8),
    )?;
    let entries = read_block_dir_bytes(&dir_bytes, slot.n_entries, file_len);
    let mut ranges = Vec::new();
    for &(off, length) in &entries {
        if length >= 16 {
            ranges.push(ByteRange::new(off as u64, length as usize));
        }
    }
    let loaded = source.load_ranges_default(&ranges)?;
    for &(off, length) in &entries {
        if length < 16 {
            continue;
        }
        if let Some(block) = find_loaded(&loaded, off as u64, length as usize) {
            let parsed = parse_dim_title(block);
            if parsed.0.is_some() || parsed.2.is_some() {
                return Ok(parsed);
            }
        }
    }
    Ok((None, None, None, None))
}

fn dim_dir_labels_src(
    source: &IvtSource,
    dim: &DimDesc,
    slot_index: usize,
    geography: bool,
) -> Result<(Option<Vec<String>>, Option<Vec<String>>), IvtError> {
    if let Some(data) = source.as_memory_slice() {
        return Ok(dim_dir_labels(data, dim, slot_index, geography));
    }
    let Some(slot) = dim_slot_from_source(source, slot_index)? else {
        return Ok((None, None));
    };
    let file_len = source.len()? as usize;
    let dir_bytes = source.read_range(
        slot.dir_ptr as u64,
        (slot.n_entries as usize + 8).saturating_mul(8),
    )?;
    let entries = read_block_dir_bytes(&dir_bytes, slot.n_entries, file_len);
    let mut ranges = Vec::new();
    for &(off, length) in &entries {
        if (length as i64) >= dim.count + 4 {
            ranges.push(ByteRange::new(off as u64, length as usize));
        }
    }
    let loaded = source.load_ranges_default(&ranges)?;
    let mut arrays = Vec::new();
    for &(off, length) in &entries {
        if (length as i64) < dim.count + 4 {
            continue;
        }
        let Some(block) = find_loaded(&loaded, off as u64, length as usize) else {
            continue;
        };
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
        return Ok((None, None));
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
    Ok((Some(en), fr))
}
