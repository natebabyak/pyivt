//! File summary, dimension members, geographies, footnotes.

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;

use crate::bytes::{latin1, u16, u32};
use crate::descriptor::{geo_dim_index, parse_descriptor, Descriptor, DimDesc};
use crate::error::IvtError;
use crate::header::{
    dim_slots, read_header, read_header_ex, HeaderLayout, DIM_SLOT_STRIDE, HDR_DESCRIPTOR_PTR,
    HDR_DIM_SLOT0, HDR_MASTER_PTR, HDR_STATUS_LEGEND,
};
use crate::io::IvtSource;

static TIME_SERIES_NAME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)timeseries|time\s*series|time\s*period|\bdate\b").unwrap());
static FR_CHARS: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[àâäéèêëïîôùûüçœæÀÂÄÉÈÊËÏÎÔÙÛÜÇ]").unwrap());
static FR_WORDS: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(de|des|les|une|pour|dans|recensement|année|âge)\b").unwrap());
static PID_FIRST: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\w.-]+$").unwrap());
static PID_FULL: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\dA-Za-z][\w./-]*$").unwrap());
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
static PRODUCT_ID_HEAD: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"Product ID:\s*([^\r\n]+)").unwrap());
static TITLE_EN_HEAD: Lazy<Regex> = Lazy::new(|| Regex::new(r"Title:\s*([^\r\n]+)").unwrap());
static TITLE_FR_HEAD: Lazy<Regex> = Lazy::new(|| Regex::new(r"Titre\s*:\s*([^\r\n]+)").unwrap());
static UNIVERSE_HEAD: Lazy<Regex> = Lazy::new(|| Regex::new(r"Universe:\s*([^\r\n]+)").unwrap());
static PID_FALLBACK: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(\d{2}-\d{3}-[A-Z]\d{7,}|\d{8})\b").unwrap());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionType {
    Geographic,
    Normal,
    TimeSeries,
}

impl DimensionType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Geographic => "geographic",
            Self::Normal => "normal",
            Self::TimeSeries => "time_series",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Dimension {
    pub name_en: String,
    pub name_fr: Option<String>,
    /// Beyond File Summary "Items" (map member list when longer than the cube).
    pub count: i64,
    pub dimension_type: DimensionType,
    /// Internal: geo dim flag for tidy export (not exposed to Python).
    pub is_geography: bool,
    pub description_en: Option<String>,
    pub description_fr: Option<String>,
    pub members_en: Option<Vec<String>>,
    pub members_fr: Option<Vec<String>>,
    /// Dimension-level notes referenced by the summary language rows.
    pub notes_en: Option<String>,
    pub notes_fr: Option<String>,
    pub ordinal: Option<Vec<i64>>,
}

#[derive(Debug, Clone)]
pub struct Footnote {
    /// Beyond File Summary marker, e.g. `(1)` or `(§)`.
    pub symbol: String,
    pub number: i64,
    pub description: LocalizedText,
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
    pub description: LocalizedText,
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
    metadata_image: Option<Vec<u8>>,
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
            self.footnotes = Some(if let Some(data) = &self.metadata_image {
                footnotes(data)
            } else {
                footnotes_from_source(&self.source).map_err(|e| e.to_string())?
            });
        }
        Ok(self.footnotes.as_ref().unwrap())
    }

    pub fn missing_values(&mut self) -> Result<&Vec<MissingValue>, String> {
        if self.missing_values.is_none() {
            self.missing_values = Some(if let Some(data) = &self.metadata_image {
                missing_values(data)
            } else {
                missing_values_from_source(&self.source).map_err(|e| e.to_string())?
            });
        }
        Ok(self.missing_values.as_ref().unwrap())
    }

    fn ensure_dimensions(&mut self) -> Result<(), IvtError> {
        if self.dimensions.is_some() {
            return Ok(());
        }
        let dims = if let Some(data) = &self.metadata_image {
            build_dimensions(data, &self.desc)
        } else {
            build_dimensions_from_source(&self.source, &self.desc)?
        };
        self.dimensions = Some(dims);
        Ok(())
    }

    fn ensure_geographies(&mut self) -> Result<(), IvtError> {
        if self.geographies.is_some() {
            return Ok(());
        }
        let gd = geo_dim_index(&self.desc.dims);
        let geos = if let Some(data) = &self.metadata_image {
            geo_from_dir(data, &self.desc.dims[gd - 1], gd - 1)
        } else {
            geo_from_source(&self.source, &self.desc.dims[gd - 1], gd - 1)?
        };
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
    let mut out = Vec::new();
    let Ok(directory) = crate::metadata_directory::MetadataDirectory::new(data, HDR_MASTER_PTR)
    else {
        return out;
    };
    for index in 0..max_entries.min(directory.count) {
        let Ok(raw) = directory.entry(data, index) else {
            break;
        };
        let off = u32(raw, 0);
        let s1 = u16(raw, 4);
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

/// Indexed master entries retain holes: language references are entry ordinals.
fn master_record(data: &[u8], index: usize) -> Option<&[u8]> {
    crate::metadata_directory::record(data, HDR_MASTER_PTR, index)
}

/// Language matrix rows: language ID, FACET index, notes index, keywords index.
fn summary_language_record(data: &[u8], language: u32, column: usize) -> Option<&[u8]> {
    if let Some(raw) = master_record(data, 7) {
        let decoded = crate::decode::decompress_component(raw).ok()?;
        if decoded.get(..2) != Some(&[1, 2]) || decoded.len() < 6 || u16(&decoded, 4) != 32 {
            return None;
        }
        for row in decoded[6..].as_chunks::<32>().0 {
            if u32(row, 0) == language {
                let index = u32(row, column) as usize;
                // Zero is the default FACET record, but means absent for notes/keywords.
                return if column != 4 && index == 0 {
                    None
                } else {
                    master_record(data, index)
                };
            }
        }
        return None;
    }
    if language == 1 {
        master_record(
            data,
            match column {
                4 => 0,
                8 => 3,
                12 => 4,
                _ => return None,
            },
        )
    } else {
        None
    }
}

fn parse_facet_summary(data: &[u8]) -> HashMap<&'static str, Option<String>> {
    let mut out = HashMap::new();
    for (language, keys) in [
        (1, ["title_en", "author_en", "category_en"]),
        (2, ["title_fr", "author_fr", "category_fr"]),
    ] {
        let fields = summary_language_record(data, language, 4).and_then(|raw| {
            let decoded = crate::decode::decompress_component(raw).ok()?;
            if decoded.get(4..9) != Some(b"FACET") {
                return None;
            }
            let layout = match u16(&decoded, 2) {
                0x304 | 0x3cc => [(11, 255), (266, 255), (521, 255)],
                0xc3 => [(11, 60), (71, 64), (135, 64)],
                _ => return None,
            };
            let mut fields = Vec::with_capacity(layout.len());
            for (offset, length) in layout {
                let raw = decoded.get(offset..offset + length)?;
                let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
                let text = latin1(&raw[..end]).trim().to_string();
                fields.push((!text.is_empty()).then_some(text));
            }
            Some(fields)
        });
        for (i, key) in keys.into_iter().enumerate() {
            out.insert(key, fields.as_ref().and_then(|fields| fields[i].clone()));
        }
    }
    out
}

fn summary_text(data: &[u8], language: u32, column: usize) -> Option<String> {
    let decoded =
        crate::decode::decompress_component(summary_language_record(data, language, column)?)
            .ok()?;
    if decoded.get(..2) != Some(&[1, 1]) {
        return None;
    }
    let raw = decoded.get(4..)?;
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    Some(normalize_notes_text(&latin1(&raw[..end])))
}

fn identity_from_blob(text: &str) -> (Option<String>, Option<String>) {
    let text = text.trim_start();
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized.splitn(2, '\n');
    let pid = lines
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let title = lines
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(ref p) = pid {
        let first = p.split_whitespace().next().unwrap_or("");
        if !PID_FIRST.is_match(first) {
            return (None, Some(text.trim().to_string()));
        }
    }
    (pid, title)
}

fn parse_identity_notes(data: &[u8]) -> (Option<String>, Option<String>, Option<String>) {
    let en = summary_text(data, 1, 8);
    let fr = summary_text(data, 2, 8);
    let product_id = en.as_deref().and_then(|text| {
        PRODUCT_ID_HEAD
            .captures(text)
            .map(|m| m[1].trim().to_string())
            .or_else(|| {
                let (pid, _) = identity_from_blob(text);
                pid.filter(|p| PID_FULL.is_match(p))
            })
    });
    (en, fr, product_id)
}

fn keyword_text(data: &[u8], language: u32) -> Option<String> {
    let decoded =
        crate::decode::decompress_component(summary_language_record(data, language, 12)?).ok()?;
    // Keywords can be a NUL-separated byte vector or a matrix of fixed-width
    // strings. They are not notes: stopping at the first NUL loses keywords.
    let keywords: Vec<String> = match decoded.get(..2)? {
        [1, 1] => decoded.get(4..)?.split(|&b| b == 0).map(latin1).collect(),
        [1, 2] => {
            let width = u16(&decoded, 4) as usize;
            if width == 0 {
                return None;
            }
            decoded
                .get(6..)?
                .chunks_exact(width)
                .map(|row| {
                    let end = row.iter().position(|&b| b == 0).unwrap_or(row.len());
                    latin1(&row[..end])
                })
                .collect()
        }
        _ => return None,
    };
    let text = keywords
        .iter()
        .map(|word| word.trim())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    (!text.is_empty()).then_some(text)
}

fn parse_keywords(data: &[u8]) -> LocalizedText {
    LocalizedText {
        en: keyword_text(data, 1),
        fr: keyword_text(data, 2),
    }
}

/// Normalize identity notes toward Beyond HTML (CRLF→LF, trim line tails/indents).
fn normalize_notes_text(text: &str) -> String {
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<String> = unified
        .lines()
        .map(|line| {
            // Beyond File Summary HTML collapses leading indentation on note lines.
            line.trim_end().trim_start().to_string()
        })
        .collect();
    lines.join("\n").trim().to_string()
}

fn parse_time_series(data: &[u8]) -> Option<TimeSeriesInfo> {
    let decoded = crate::decode::decompress_component(master_record(data, 2)?).ok()?;
    if decoded.get(..4) != Some(&[1, 1, 32, 0]) {
        return None;
    }
    let rate = u16(&decoded, 10);
    let frequency = match rate {
        0 => Some("No".into()),
        1 => Some("Annual".into()),
        2 => Some("Semi-annual".into()),
        4 => Some("Quarterly".into()),
        12 => Some("Monthly".into()),
        _ => None,
    };
    let date = |offset| {
        let serial = u32(&decoded, offset);
        (rate != 0 && serial != 0).then(|| crate::members::iso_date(serial as i64 - 719_469))
    };
    Some(TimeSeriesInfo {
        frequency,
        start: date(12),
        end: date(16),
    })
}

fn dimension_type_for(d: &DimDesc) -> DimensionType {
    if d.type_code == 6 || TIME_SERIES_NAME_RE.is_match(&d.name) {
        return DimensionType::TimeSeries;
    }
    if d.double01 || d.type_code == 0x12 {
        return DimensionType::Geographic;
    }
    DimensionType::Normal
}

fn read_only_flag(data: &[u8]) -> bool {
    master_record(data, 2)
        .and_then(|raw| crate::decode::decompress_component(raw).ok())
        .is_some_and(|record| record.get(..4) == Some(&[1, 1, 32, 0]) && u16(&record, 6) & 1 != 0)
}

fn parse_table_identity(data: &[u8]) -> HashMap<&'static str, Option<String>> {
    let head = latin1(&data[..data.len().min(8000)]);
    let grab = |re: &Regex| {
        re.captures(&head)
            .map(|m| m.get(1).unwrap().as_str().trim().to_string())
    };
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
        } else if !blk.is_empty() && (blk[0] == b' ' || blk[0].is_ascii_alphabetic()) {
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

fn dimension_record(data: &[u8], slot_index: usize, entry_index: usize) -> Option<&[u8]> {
    let base = HDR_DIM_SLOT0.checked_add(DIM_SLOT_STRIDE.checked_mul(slot_index)?)?;
    crate::metadata_directory::record(data, base, entry_index)
}

fn dimension_summary_rows(data: &[u8], slot_index: usize) -> Option<Vec<u8>> {
    let raw = dimension_record(data, slot_index, 3)?;
    let decoded = crate::decode::decompress_component(raw).ok()?;
    if decoded.get(..2) != Some(&[1, 2])
        || decoded.len() < 6
        || u16(&decoded, 4) != 86
        || !(1..=32).contains(&u16(&decoded, 2))
    {
        return None;
    }
    Some(decoded)
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
    let Some(decoded) = dimension_summary_rows(data, slot_index) else {
        return (None, None, None, None);
    };
    let field = |raw: &[u8]| {
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Some(latin1(&raw[..end]).trim().to_string())
    };
    let row = &decoded[6..92];
    let (name_en, description_en) = (field(&row[..18]), field(&row[34..]));
    let (name_fr, description_fr) = if u16(&decoded, 2) >= 2 {
        let row = &decoded[92..178];
        (field(&row[..18]), field(&row[34..]))
    } else {
        (None, None)
    };
    (name_en, name_fr, description_en, description_fr)
}

fn dim_dir_labels(
    data: &[u8],
    dim: &DimDesc,
    slot_index: usize,
    _geography: bool,
) -> (Option<Vec<String>>, Option<Vec<String>>) {
    let Some(rows) = dimension_summary_rows(data, slot_index) else {
        return (None, None);
    };
    // Summary row +18 identifies its display member field. Do not choose a
    // field by English/French-looking labels or by the longest text array.
    // Time axes have native Date32 records rather than description strings.
    if dim.type_code == 6 {
        return (None, None);
    }
    let Ok(fields) = crate::members::read_members(data, slot_index, dim.count as usize) else {
        return (None, None);
    };
    let labels = |row: &[u8]| {
        let index = u16(row, 18) as usize;
        fields.get(index).map(|(_, values)| {
            values
                .iter()
                .map(|value| value.clone().unwrap_or_default())
                .collect()
        })
    };
    let en = labels(&rows[6..92]);
    let fr = if u16(&rows, 2) >= 2 {
        labels(&rows[92..178])
    } else {
        None
    };
    (en, fr)
}

fn dim_slot_notes(data: &[u8], slot_index: usize) -> (Option<String>, Option<String>) {
    let Some(decoded) = dimension_summary_rows(data, slot_index) else {
        return (None, None);
    };
    let note = |row: &[u8]| {
        let index = u16(row, 22) as usize;
        if index == 0 {
            return None;
        }
        let raw = dimension_record(data, slot_index, index)?;
        let decoded = crate::decode::decompress_component(raw).ok()?;
        if decoded.get(..2) != Some(&[1, 1]) {
            return None;
        }
        let raw = &decoded[4..];
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Some(normalize_notes_text(&latin1(&raw[..end])))
    };
    let en = note(&decoded[6..92]);
    let fr = if u16(&decoded, 2) >= 2 {
        note(&decoded[92..178])
    } else {
        None
    };
    (en, fr)
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
    // The table annotation legend follows the missing-value legend in the
    // fixed header. Its index specifies display order and language columns.
    // Numbered footnotes embedded in table/dimension notes are separate data.
    let mut out = Vec::new();
    for row in legend_values(data, HDR_STATUS_LEGEND + DIM_SLOT_STRIDE, false) {
        let Some(symbol) = beyond_footnote_symbol(&row.symbol) else {
            continue;
        };
        out.push(Footnote {
            symbol,
            number: row.code.unwrap_or(0),
            description: row.description,
            scope: None,
            dimension: None,
            member_id: None,
        });
    }
    out
}

fn symbol_chars_ok(symbol: &str) -> bool {
    !symbol.is_empty()
        && symbol.chars().all(|c| {
            let u = c as u32;
            // ASCII graphic + space (Beyond `- - -`) + Latin-1 § etc.
            (32..127).contains(&u) || (160..256).contains(&u)
        })
}

fn parse_status_legend_record(blob: &[u8]) -> Option<(String, String)> {
    if blob.len() < 4 || !matches!(blob[0], 2 | 0x82) || blob[1] != 1 {
        return None;
    }
    let decoded = crate::decode::decompress_component(blob).ok()?;
    let body = decoded.get(4..)?;
    // Reserved byte, symbol byte length, symbol (including NUL), then a
    // little-endian word length and description (including NUL).
    let n = *body.get(1)? as usize;
    if n < 1 {
        return None;
    }
    let symbol = body.get(2..2 + n)?;
    if symbol.last() != Some(&0) || body.len() < 4 + n {
        return None;
    }
    let text_len = u16(body, 2 + n) as usize;
    let text = body.get(4 + n..4 + n + text_len)?;
    let symbol = latin1(&symbol[..n - 1]);
    if !symbol_chars_ok(&symbol) {
        return None;
    }
    let end = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    Some((symbol, latin1(&text[..end]).trim().to_string()))
}

fn beyond_footnote_symbol(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    if s.starts_with('(') && s.ends_with(')') {
        return Some(s.to_string());
    }
    Some(format!("({})", s))
}

fn status_legend_dir(data: &[u8], slot: usize) -> Vec<(u32, u16)> {
    let mut entries = Vec::new();
    let Ok(directory) = crate::metadata_directory::MetadataDirectory::new(data, slot) else {
        return entries;
    };
    for index in 0..directory.count {
        let Ok(raw) = directory.entry(data, index) else {
            break;
        };
        let off = u32(raw, 0);
        let s1 = u16(raw, 4);
        let s2 = u16(raw, 6);
        if off == 0 && s1 == 0 {
            entries.push((0, 0));
            continue;
        }
        let ok = s1 == s2 || s2 == 4 * s1.div_ceil(4) || s2 >= s1;
        if !ok || s1 == 0 || (off as usize) + (s1 as usize) > data.len() {
            break;
        }
        entries.push((off, s1));
    }
    entries
}

fn missing_values(data: &[u8]) -> Vec<MissingValue> {
    legend_values(data, HDR_STATUS_LEGEND, true)
}

fn legend_values(data: &[u8], slot: usize, trim_short_period: bool) -> Vec<MissingValue> {
    if data.len() < slot + 14 {
        return Vec::new();
    }
    let dir_ptr = u32(data, slot);
    let n_entries = u32(data, slot + 8);
    if dir_ptr == 0 || n_entries < 1 {
        return Vec::new();
    }
    let entries = status_legend_dir(data, slot);
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
            match parse_status_legend_record(&data[off as usize..off as usize + length as usize]) {
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
        // Beyond short EN legend labels omit a trailing period; longer prose keeps it.
        let en = en.map(|mut t| {
            if trim_short_period
                && t.ends_with('.')
                && !t.ends_with("...")
                && t.chars().count() < 40
            {
                t.pop();
            }
            t
        });
        out.push(MissingValue {
            symbol: symbol.unwrap(),
            description: LocalizedText { en, fr },
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
        let (members_en, members_fr) = dim_dir_labels(data, d, i, is_geo);
        let items = d.count;
        let (short, title_name_fr, description_en, description_fr) = dim_title(data, i);
        // Prefer dim-title code (COOKIE TYPE) over descriptor name when present.
        let name_en = short
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| d.name.clone());
        let description_en = description_en.or_else(|| Some(name_en.clone()));
        let mut name_fr = title_name_fr.or_else(|| description_fr.clone());
        if is_geo && name_en.eq_ignore_ascii_case("Geography") && name_fr.is_none() {
            name_fr = Some("Géographie".into());
        }
        let (notes_en, notes_fr) = dim_slot_notes(data, i);
        dims.push(Dimension {
            name_en,
            name_fr,
            count: items,
            dimension_type: dimension_type_for(d),
            is_geography: is_geo,
            description_en,
            description_fr,
            members_en,
            members_fr,
            notes_en,
            notes_fr,
            ordinal: None,
        });
    }
    dims
}

/// File Summary datapoints are the product of the declared axis item counts.
fn summary_datapoints(desc: &Descriptor) -> i64 {
    if desc.dims.is_empty() {
        return 0;
    }
    desc.dims
        .iter()
        .fold(1i64, |acc, d| acc.saturating_mul(d.count))
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
    let keywords = parse_keywords(data);
    let number_of_datapoints = summary_datapoints(&desc);
    let (frequency, start_date, end_date) = parse_time_series(data)
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
        metadata_image: None,
        dimensions: None,
        geographies: None,
        footnotes: None,
        missing_values: None,
    })
}

/// Materialize only metadata directories and their declared records. Relocating
/// their offsets keeps the bytes and path parsers identical without allocating
/// a buffer as large as the source file or touching the cube's value pages.
fn metadata_catalog(
    source: &IvtSource,
    master: bool,
    dimensions: bool,
    legends: &[usize],
    descriptor: bool,
) -> Result<Vec<u8>, IvtError> {
    let file_len = source.len()? as usize;
    let header_len = (HDR_DIM_SLOT0 + DIM_SLOT_STRIDE * 32).min(file_len);
    let mut catalog = source.read_prefix(header_len)?;
    if catalog.len() < 576 {
        return Err(IvtError::Parse(
            "IVT file too small to contain a header".into(),
        ));
    }
    let header = catalog.clone();
    let mut copied = HashMap::<(u32, u16), u32>::new();
    let mut slots = Vec::new();
    if master {
        slots.push((HDR_MASTER_PTR, u32(&header, HDR_MASTER_PTR + 8) as usize));
    }
    if dimensions {
        for slot in dim_slots(&header, 32) {
            slots.push((
                HDR_DIM_SLOT0 + slot.index * DIM_SLOT_STRIDE,
                slot.n_entries as usize,
            ));
        }
    }
    for &slot in legends {
        if slot + 14 <= header.len() {
            slots.push((slot, u32(&header, slot + 8) as usize));
        }
    }
    for (slot, count) in slots {
        let ptr = u32(&header, slot) as usize;
        if ptr == 0 || count == 0 {
            continue;
        }
        let mut directory =
            crate::metadata_directory::MetadataDirectory::new(&header, slot)?.read(source)?;
        for i in (0..directory.len().saturating_sub(7)).step_by(8) {
            let off = u32(&directory, i);
            let length = u16(&directory, i + 4);
            if off == 0 || length == 0 {
                continue;
            }
            if off as usize + length as usize > file_len {
                return Err(IvtError::InvalidDirectory("metadata record range"));
            }
            let relocated = if let Some(&at) = copied.get(&(off, length)) {
                at
            } else {
                let at = u32::try_from(catalog.len())
                    .map_err(|_| IvtError::InvalidDirectory("metadata catalog size"))?;
                catalog.extend_from_slice(&source.read_range(off as u64, length as usize)?);
                copied.insert((off, length), at);
                at
            };
            directory[i..i + 4].copy_from_slice(&relocated.to_le_bytes());
        }
        let at = u32::try_from(catalog.len())
            .map_err(|_| IvtError::InvalidDirectory("metadata catalog size"))?;
        catalog.extend_from_slice(&directory);
        // Old directory readers allow up to eight trailing blank entries.
        catalog.resize(catalog.len() + 64, 0);
        catalog[slot..slot + 4].copy_from_slice(&at.to_le_bytes());
        // The relocated catalog stores a flat directory, even when the source
        // uses a page-pointer table. Preserve unrelated native flag bits.
        let flags = u16(&header, slot + 12) & !1;
        catalog[slot + 12..slot + 14].copy_from_slice(&flags.to_le_bytes());
    }
    if descriptor {
        if crate::descriptor::structured_descriptor(&catalog).is_none() {
            return Err(IvtError::DescriptorNotFound);
        }
        catalog[HDR_DESCRIPTOR_PTR..HDR_DESCRIPTOR_PTR + 4].fill(0);
    }
    // Value-directory offsets do not apply to this relocated metadata image.
    catalog[crate::header::HDR_DIR_PTR..crate::header::HDR_DIR_PTR + 4].fill(0);
    Ok(catalog)
}

fn build_metadata_selective(
    source: IvtSource,
    desc: Option<Descriptor>,
    hdr: Option<HeaderLayout>,
) -> Result<IvtMetadata, IvtError> {
    let catalog = metadata_catalog(
        &source,
        true,
        true,
        &[HDR_STATUS_LEGEND, HDR_STATUS_LEGEND + 14],
        true,
    )?;
    // Header identity uses the actual file length even though the parser sees
    // only the selected metadata records.
    let hdr = match hdr {
        Some(hdr) => hdr,
        None => read_header_ex(&catalog, source.len()? as usize).map_err(IvtError::Parse)?,
    };
    let mut metadata = build_metadata_memory(&catalog, source, desc, Some(hdr))?;
    metadata.metadata_image = Some(catalog);
    Ok(metadata)
}

fn footnotes_from_source(source: &IvtSource) -> Result<Vec<Footnote>, IvtError> {
    if let Some(data) = source.as_memory_slice() {
        return Ok(footnotes(data));
    }
    let catalog = metadata_catalog(source, false, false, &[HDR_STATUS_LEGEND + 14], false)?;
    Ok(footnotes(&catalog))
}

fn missing_values_from_source(source: &IvtSource) -> Result<Vec<MissingValue>, IvtError> {
    if let Some(data) = source.as_memory_slice() {
        return Ok(missing_values(data));
    }
    let catalog = metadata_catalog(source, false, false, &[HDR_STATUS_LEGEND], false)?;
    Ok(missing_values(&catalog))
}

fn build_dimensions_from_source(
    source: &IvtSource,
    desc: &Descriptor,
) -> Result<Vec<Dimension>, IvtError> {
    if let Some(data) = source.as_memory_slice() {
        return Ok(build_dimensions(data, desc));
    }
    let catalog = metadata_catalog(source, false, true, &[], false)?;
    Ok(build_dimensions(&catalog, desc))
}

fn geo_from_source(
    source: &IvtSource,
    dim: &DimDesc,
    slot_index: usize,
) -> Result<Geographies, IvtError> {
    if let Some(data) = source.as_memory_slice() {
        return Ok(geo_from_dir(data, dim, slot_index));
    }
    let catalog = metadata_catalog(source, false, true, &[], false)?;
    Ok(geo_from_dir(&catalog, dim, slot_index))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn master_fixture(records: &[(usize, Vec<u8>)]) -> Vec<u8> {
        let count = records.iter().map(|(i, _)| i + 1).max().unwrap_or(0);
        let mut data = vec![0; 1024 + count * 8];
        data[HDR_MASTER_PTR..HDR_MASTER_PTR + 4].copy_from_slice(&1024u32.to_le_bytes());
        data[HDR_MASTER_PTR + 8..HDR_MASTER_PTR + 12]
            .copy_from_slice(&(count as u32).to_le_bytes());
        for (index, record) in records {
            let at = 1024 + index * 8;
            let offset = data.len() as u32;
            data[at..at + 4].copy_from_slice(&offset.to_le_bytes());
            data[at + 4..at + 6].copy_from_slice(&(record.len() as u16).to_le_bytes());
            data.extend_from_slice(record);
        }
        data
    }

    fn text_record(text: &[u8]) -> Vec<u8> {
        let mut record = vec![1, 1];
        record.extend_from_slice(&(text.len() as u16).to_le_bytes());
        record.extend_from_slice(text);
        record
    }

    #[test]
    fn keywords_preserve_all_terms_and_follow_language_references() {
        let mut matrix = vec![1, 2, 2, 0, 32, 0];
        for (language, keywords) in [(2u32, 11u32), (1, 4)] {
            let mut row = [0; 32];
            row[..4].copy_from_slice(&language.to_le_bytes());
            row[12..16].copy_from_slice(&keywords.to_le_bytes());
            matrix.extend_from_slice(&row);
        }
        let mut french = vec![1, 2, 2, 0, 12, 0];
        french.extend_from_slice(b"population\0\0recensement\0");
        let data = master_fixture(&[
            (4, text_record(b" population\0census\0\0")),
            (7, matrix),
            (11, french),
        ]);
        let keywords = parse_keywords(&data);
        assert_eq!(keywords.en.as_deref(), Some("population, census"));
        assert_eq!(keywords.fr.as_deref(), Some("population, recensement"));
    }

    #[test]
    fn keywords_absent_blank_or_malformed_are_none() {
        for record in [
            Vec::new(),
            text_record(b"\0 \t\0"),
            vec![1, 2, 0, 0, 0, 0],
            vec![1, 2, 2, 0, 12, 0, b'A'],
            vec![4, 1, 1, 0, 1, 0, 0, 0],
        ] {
            let keywords = parse_keywords(&master_fixture(&[(4, record)]));
            assert!(keywords.en.is_none());
            assert!(keywords.fr.is_none());
        }
    }

    #[test]
    fn facet_blank_fields_are_none_and_truncated_fields_do_not_panic() {
        let mut facet = text_record(&vec![0; 0x304]);
        facet[4..11].copy_from_slice(b"FACET04");
        facet[11..16].copy_from_slice(b"Title");
        facet[266..269].copy_from_slice(b" \t ");
        let parsed = parse_facet_summary(&master_fixture(&[(0, facet)]));
        assert_eq!(parsed["title_en"].as_deref(), Some("Title"));
        assert!(parsed["author_en"].is_none());
        assert!(parsed["category_en"].is_none());
        let truncated = text_record(b"FACET04");
        let parsed = parse_facet_summary(&master_fixture(&[(0, truncated)]));
        assert!(parsed["author_en"].is_none());
    }

    #[test]
    fn dimension_types_are_a_closed_set() {
        for (name, type_code, double01, expected) in [
            ("Region", 0x12, false, "geographic"),
            ("Region", 0, true, "geographic"),
            ("Items", 0, false, "normal"),
            ("TIME", 6, false, "time_series"),
            ("Time Period", 0, false, "time_series"),
        ] {
            let dim = DimDesc {
                name: name.into(),
                count: 2,
                type_code,
                double01,
            };
            assert_eq!(dimension_type_for(&dim).as_str(), expected);
        }
    }
}
