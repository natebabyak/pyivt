//! Structured dimension fields, including chunked member arrays.
//!
//! Dimension-directory entry zero is a matrix of 34-byte field definitions.
//! Each definition supplies its storage type and the directory entry containing
//! its chunk index. No choice of member field depends on its label or contents.

use crate::bytes::{latin1, u16, u32};
use crate::header::{DIM_SLOT_STRIDE, HDR_DIM_SLOT0};
use crate::metadata_directory::MetadataDirectory;

pub use crate::decode::decompress_component as decompress_record;

/// Native member-attribute storage types. Codes always remain strings, including
/// codes containing only digits; numeric attributes retain their storage type.
#[derive(Debug, Clone)]
pub enum MemberValues {
    Utf8(Vec<Option<String>>),
    Int64(Vec<Option<i64>>),
    Float64(Vec<Option<f64>>),
    Date32(Vec<Option<i32>>),
}

impl MemberValues {
    pub fn len(&self) -> usize {
        match self {
            Self::Utf8(v) => v.len(),
            Self::Int64(v) => v.len(),
            Self::Float64(v) => v.len(),
            Self::Date32(v) => v.len(),
        }
    }

    fn append(&mut self, other: Self) -> Result<(), String> {
        match (self, other) {
            (Self::Utf8(a), Self::Utf8(b)) => a.extend(b),
            (Self::Int64(a), Self::Int64(b)) => a.extend(b),
            (Self::Float64(a), Self::Float64(b)) => a.extend(b),
            (Self::Date32(a), Self::Date32(b)) => a.extend(b),
            _ => return Err("member field changes storage type between chunks".into()),
        }
        Ok(())
    }

    fn truncate(&mut self, count: usize) {
        match self {
            Self::Utf8(v) => v.truncate(count),
            Self::Int64(v) => v.truncate(count),
            Self::Float64(v) => v.truncate(count),
            Self::Date32(v) => v.truncate(count),
        }
    }

    fn into_strings(self) -> Vec<Option<String>> {
        match self {
            Self::Utf8(v) => v,
            Self::Int64(v) => v.into_iter().map(|v| v.map(|x| x.to_string())).collect(),
            Self::Float64(v) => v.into_iter().map(|v| v.map(|x| x.to_string())).collect(),
            Self::Date32(v) => v
                .into_iter()
                .map(|v| v.map(|x| iso_date(x as i64)))
                .collect(),
        }
    }
}

struct Directory<'a> {
    data: &'a [u8],
    directory: MetadataDirectory,
    entries: usize,
}

impl<'a> Directory<'a> {
    fn new(data: &'a [u8], dimension: usize) -> Result<Self, String> {
        let slot = dimension
            .checked_mul(DIM_SLOT_STRIDE)
            .and_then(|n| HDR_DIM_SLOT0.checked_add(n))
            .ok_or("dimension slot overflow")?;
        if slot + DIM_SLOT_STRIDE > data.len() {
            return Err(format!("missing directory for dimension {dimension}"));
        }
        let directory = MetadataDirectory::new(data, slot).map_err(|e| e.to_string())?;
        let entries = directory.count;
        if entries == 0 {
            return Err(format!("invalid directory for dimension {dimension}"));
        }
        directory
            .entry(data, entries - 1)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            data,
            directory,
            entries,
        })
    }

    fn block(&self, index: usize) -> Result<Option<&'a [u8]>, String> {
        if index >= self.entries {
            return Err(format!(
                "member directory index {index} exceeds {} entries",
                self.entries
            ));
        }
        let entry = self
            .directory
            .entry(self.data, index)
            .map_err(|e| e.to_string())?;
        let offset = u32(entry, 0) as usize;
        let length = u16(entry, 4) as usize;
        if offset == 0 && length == 0 {
            return Ok(None);
        }
        self.data
            .get(
                offset
                    ..offset
                        .checked_add(length)
                        .ok_or("member block size overflow")?,
            )
            .map(Some)
            .ok_or_else(|| format!("member block {index} is outside the IVT"))
    }

    fn record(&self, index: usize) -> Result<Option<Vec<u8>>, String> {
        self.block(index)?.map(decompress_record).transpose()
    }

    fn chunks(&self, index: usize) -> Result<Vec<usize>, String> {
        let Some(record) = self.record(index)? else {
            return Ok(Vec::new());
        };
        if record[0] & 15 != 4 || record[1] != 1 {
            return Err(format!(
                "member chunk index {index} is not an integer vector"
            ));
        }
        Ok(record[4..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|v| u32(v, 0) as usize)
            .collect())
    }
}

fn optional_text(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        None
    } else {
        Some(latin1(bytes))
    }
}

fn fixed_strings(record: &[u8]) -> Result<Vec<Option<String>>, String> {
    if record[0] & 15 != 1 || record[1] != 2 {
        return Err("member code storage is not a byte matrix".into());
    }
    let stride = u16(record, 4) as usize;
    if stride == 0 {
        return Err("zero-width member code".into());
    }
    record[6..]
        .chunks_exact(stride)
        .map(|row| {
            let length = row[0] as usize;
            row.get(1..1 + length)
                .map(optional_text)
                .ok_or_else(|| "member code exceeds its row".into())
        })
        .collect()
}

fn variable_strings(record: &[u8]) -> Result<Vec<Option<String>>, String> {
    if record[0] & 15 != 1 || record[1] != 1 || record.len() < 6 {
        return Err("member text storage is not a byte vector".into());
    }
    let count = u16(record, 4) as usize;
    let mut cursor = 6;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let length = *record.get(cursor).ok_or("truncated member text length")? as usize;
        cursor += 1;
        let bytes = record
            .get(cursor..cursor + length)
            .ok_or("truncated member text")?;
        values.push(optional_text(bytes));
        cursor += length;
        if record.get(cursor) != Some(&0) {
            return Err("missing member text terminator".into());
        }
        cursor += 1;
    }
    Ok(values)
}

fn signed_numbers(record: &[u8]) -> Result<MemberValues, String> {
    let width = (record[0] & 15) as usize;
    if record[1] != 1 {
        return Err("member numeric storage is not a vector".into());
    }
    if width == 8 {
        return Ok(MemberValues::Float64(
            record[4..]
                .chunks_exact(width)
                .map(|v| Some(f64::from_le_bytes(v.try_into().unwrap())))
                .collect(),
        ));
    }
    let values = record[4..]
        .chunks_exact(width)
        .map(|scalar| {
            let value = match width {
                1 => scalar[0] as i8 as i64,
                2 => i16::from_le_bytes(scalar.try_into().unwrap()) as i64,
                4 => i32::from_le_bytes(scalar.try_into().unwrap()) as i64,
                _ => return Err("unsupported member numeric width".into()),
            };
            Ok(Some(value))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(MemberValues::Int64(values))
}

fn referenced_strings(dir: &Directory<'_>, record: &[u8]) -> Result<Vec<Option<String>>, String> {
    if record[0] & 15 != 4 || record[1] != 1 {
        return Err("member text references are not an integer vector".into());
    }
    record[4..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|scalar| {
            let index = u32(scalar, 0) as usize;
            if index == 0 {
                return Ok(None);
            }
            let Some(text) = dir.record(index)? else {
                return Ok(None);
            };
            if text[0] & 15 != 1 || text[1] != 1 || text.len() < 5 {
                return Err("referenced member text is not a byte vector".into());
            }
            // Individual note records begin with the native text-format byte.
            Ok(optional_text(&text[5..]))
        })
        .collect()
}

fn time_members(record: &[u8]) -> Result<MemberValues, String> {
    if record[0] & 15 != 1 || record[1] != 2 || u16(record, 4) != 8 {
        return Err("time-series members are not eight-byte records".into());
    }
    let dates = record[6..]
        .as_chunks::<8>()
        .0
        .iter()
        .map(|row| {
            let serial = u32(row, 0);
            if serial == 0 {
                return Ok(None);
            }
            // Beyond's Gregorian day number is ordinal day + 306.
            i32::try_from(serial as i64 - 719_469)
                .map(Some)
                .map_err(|_| "time member exceeds Arrow Date32 range".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(MemberValues::Date32(dates))
}

/// Gregorian civil date from days since 1970-01-01 (400-year cycle arithmetic).
pub(crate) fn iso_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

pub type MemberField = (String, Vec<Option<String>>);

/// Read every declared field for an axis, in native member ordinal order.
///
/// Field names are the names recorded in the IVT, without label-based guessing.
/// Callers can qualify them with the dimension name to avoid name collisions.
/// Empty strings and absent member-note references become nulls. Numeric member
/// attributes are returned as text here; datapoint typing is handled separately.
pub fn read_members(
    data: &[u8],
    dimension: usize,
    count: usize,
) -> Result<Vec<MemberField>, String> {
    Ok(read_typed_members(data, dimension, count)?
        .into_iter()
        .map(|(name, values)| (name, values.into_strings()))
        .collect())
}

/// Typed counterpart to [`read_members`], used for Arrow construction.
/// Time-series fields additionally expose their original serial and auxiliary
/// word, so conversion to an Arrow date does not discard the native record.
pub fn read_typed_members(
    data: &[u8],
    dimension: usize,
    count: usize,
) -> Result<Vec<(String, MemberValues)>, String> {
    let dir = Directory::new(data, dimension)?;
    let definitions = dir.record(0)?.ok_or("missing member field definitions")?;
    if definitions[0] & 15 != 1 || definitions[1] != 2 || u16(&definitions, 4) != 34 {
        return Err("unsupported member field definition layout".into());
    }
    let mut fields = Vec::new();
    for field in definitions[6..].as_chunks::<34>().0 {
        let name_len = field[0] as usize;
        if name_len > 17 {
            return Err("member field name exceeds its definition".into());
        }
        let name = latin1(&field[1..1 + name_len]);
        let kind = u16(field, 18);
        let entry = u32(field, 26) as usize;
        let mut values: Option<MemberValues> = None;
        let mut time_serials = Vec::new();
        let mut time_aux = Vec::new();
        for index in dir.chunks(entry)? {
            if values.as_ref().is_some_and(|v| v.len() >= count) {
                break;
            }
            if index == 0 {
                return Err(format!("missing chunk for member field {name}"));
            }
            let record = dir
                .record(index)?
                .ok_or_else(|| format!("empty chunk for member field {name}"))?;
            let chunk = match kind {
                0x0140 => fixed_strings(&record).map(MemberValues::Utf8),
                0x0180 => time_members(&record),
                0x0001 => variable_strings(&record).map(MemberValues::Utf8),
                0x0304 => referenced_strings(&dir, &record).map(MemberValues::Utf8),
                0x0302 => signed_numbers(&record),
                _ => Err(format!("unsupported member field type 0x{kind:04x}")),
            }
            .map_err(|e| format!("dimension {dimension}, field {name}: {e}"))?;
            if kind == 0x0180 {
                for row in record[6..].as_chunks::<8>().0 {
                    time_serials.push(Some(u32(row, 0) as i64));
                    time_aux.push(Some(u32(row, 4) as i64));
                }
            }
            if let Some(ref mut values) = values {
                values.append(chunk)?;
            } else {
                values = Some(chunk);
            }
        }
        let mut values = match values {
            Some(v) => v,
            None => match kind {
                0x0140 | 0x0001 | 0x0304 => MemberValues::Utf8(vec![None; count]),
                0x0180 => MemberValues::Date32(vec![None; count]),
                0x0302 => MemberValues::Int64(vec![None; count]),
                _ => return Err(format!("unsupported member field type 0x{kind:04x}")),
            },
        };
        if values.len() < count {
            return Err(format!(
                "dimension {dimension}, field {name}: only {} of {count} members",
                values.len()
            ));
        }
        values.truncate(count);
        if kind == 0x0180 {
            time_serials.resize(count, None);
            time_aux.resize(count, None);
            fields.push((format!("{name}_serial"), MemberValues::Int64(time_serials)));
            fields.push((format!("{name}_aux"), MemberValues::Int64(time_aux)));
        }
        fields.push((name, values));
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_expansion_uses_word_msb_first_and_scalar_width() {
        let record = [0x82, 1, 3, 0, 0, 0xa0, 0x34, 0x12, 0x78, 0x56];
        assert_eq!(
            decompress_record(&record).unwrap(),
            [2, 1, 3, 0, 0x34, 0x12, 0, 0, 0x78, 0x56]
        );
        assert!(decompress_record(&record[..9]).is_err());
    }

    #[test]
    fn member_strings_keep_spaces_and_null_ordinals() {
        let record = [1, 1, 11, 0, 3, 0, 1, b'A', 0, 0, 0, 2, b' ', b'B', 0];
        assert_eq!(
            variable_strings(&record).unwrap(),
            vec![Some("A".into()), None, Some(" B".into())]
        );
    }

    #[test]
    fn time_serials_cover_leap_days_and_epoch() {
        assert_eq!(iso_date(0), "1970-01-01");
        assert_eq!(iso_date(727_139 - 719_469), "1991-01-01");
        assert_eq!(iso_date(11_016), "2000-02-29");
        let mut record = vec![1, 2, 1, 0, 8, 0];
        record.extend_from_slice(&727_139u32.to_le_bytes());
        record.extend_from_slice(&0u32.to_le_bytes());
        let MemberValues::Date32(dates) = time_members(&record).unwrap() else {
            panic!()
        };
        assert_eq!(dates, [Some(7_670)]);
    }

    #[test]
    fn numeric_attributes_do_not_pass_through_text() {
        let integer = [2, 1, 2, 0, 0xfe, 0xff, 42, 0];
        let MemberValues::Int64(values) = signed_numbers(&integer).unwrap() else {
            panic!()
        };
        assert_eq!(values, [Some(-2), Some(42)]);
        let mut float = vec![8, 1, 1, 0];
        float.extend_from_slice(&1.25f64.to_le_bytes());
        let MemberValues::Float64(values) = signed_numbers(&float).unwrap() else {
            panic!()
        };
        assert_eq!(values, [Some(1.25)]);
    }

    #[test]
    fn installed_fixture_members_are_complete_when_available() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        let cases = [
            ("0d9e", 1159, 13),
            ("4076", 5468, 13),
            ("4891", 7203, 7),
            ("5678", 307, 13),
            ("625f", 14, 11),
            ("9992", 307, 6),
            ("cf698", 63404, 5),
            ("d614", 352, 7),
            ("e506", 174, 13),
            ("f203", 90, 7),
        ];
        for (prefix, geographies, measures) in cases {
            let Some(path) = std::fs::read_dir(&root)
                .ok()
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|e| e.path())
                .find(|p| p.file_name().unwrap().to_string_lossy().starts_with(prefix))
            else {
                continue;
            };
            let data = std::fs::read(path).unwrap();
            for (axis, count) in [(0, geographies), (1, measures)] {
                let fields = read_members(&data, axis, count).unwrap();
                assert!(fields.len() >= 3);
                assert!(fields.iter().all(|(_, v)| v.len() == count));
                assert!(fields
                    .iter()
                    .find(|(k, _)| k == "Code")
                    .unwrap()
                    .1
                    .iter()
                    .all(Option::is_some));
                if axis == 0 && prefix != "cf698" {
                    let uids = &fields.iter().find(|(k, _)| k == "DGUID_EN").unwrap().1;
                    assert!(uids.iter().all(Option::is_some));
                    if prefix == "0d9e" {
                        assert_eq!(uids[0].as_deref(), Some("2021S0503932"));
                    }
                    if prefix == "625f" {
                        assert_eq!(uids[13].as_deref(), Some("2021A000262"));
                    }
                }
            }
        }
    }

    #[test]
    fn three_dimensional_fixture_keeps_the_singleton_axis() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        let Some(path) = std::fs::read_dir(&root)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("941238")
            })
        else {
            return;
        };
        let data = std::fs::read(path).unwrap();
        for (axis, count) in [(0, 1), (1, 718), (2, 2)] {
            let fields = read_typed_members(&data, axis, count).unwrap();
            assert!(fields.iter().all(|(_, v)| v.len() == count));
            if axis == 0 {
                let MemberValues::Utf8(uids) =
                    &fields.iter().find(|(k, _)| k == "DGUID_EN").unwrap().1
                else {
                    panic!()
                };
                assert_eq!(uids[0].as_deref(), Some("2021A000011124"));
            }
        }
    }

    #[test]
    fn demo_member_fields_and_dates_when_inputs_are_available() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        let cases: [(&str, &str, &[usize]); 4] = [
            ("3c69fe", "People.ivt", &[2, 12, 18, 39]),
            ("6268c7", "Sample.ivt", &[2, 12, 18, 39]),
            ("ba4e61", "Products.ivt", &[3, 6, 15, 39]),
            ("ed23e0", "Oecd.ivt", &[30, 19, 26]),
        ];
        for (prefix, filename, counts) in cases {
            let path = std::fs::read_dir(&root)
                .ok()
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|e| e.path())
                .find(|p| p.file_name().unwrap().to_string_lossy().starts_with(prefix))
                .or_else(|| {
                    std::env::var_os("ProgramFiles")
                        .map(std::path::PathBuf::from)
                        .map(|p| {
                            p.join("Beyond 2020/Professional Browser/Data")
                                .join(filename)
                        })
                        .filter(|p| p.is_file())
                });
            let Some(path) = path else { continue };
            let data = std::fs::read(path).unwrap();
            for (axis, &count) in counts.iter().enumerate() {
                let fields = read_typed_members(&data, axis, count).unwrap();
                assert!(fields.iter().all(|(_, v)| v.len() == count));
                if axis == counts.len() - 1 {
                    let MemberValues::Date32(dates) =
                        &fields.iter().find(|(k, _)| k == "Timeseries").unwrap().1
                    else {
                        panic!()
                    };
                    assert!(dates.iter().all(Option::is_some));
                    assert_eq!(
                        iso_date(dates[0].unwrap() as i64),
                        if prefix == "6268c7" {
                            "1997-01-01"
                        } else if prefix == "ed23e0" {
                            "1980-01-01"
                        } else {
                            "1991-01-01"
                        }
                    );
                }
            }
        }
    }
}
