//! Member lookup table built directly as Arrow arrays.
//!
//! Selection, column naming, display-label choice and unsigned `Sort`
//! widening follow the original pyarrow implementation exactly, so the output
//! schema, metadata and values are unchanged.

use crate::compact::AxisProjection;
use crate::error::{corrupt_record, IvtError};
use crate::members::MemberValues;
use arrow::array::{
    new_null_array, ArrayRef, Date32Array, DictionaryArray, Float64Array, Int32Array, Int64Array,
    PrimitiveArray, StringArray, UInt16Array, UInt32Array, UInt64Array, UInt8Array,
};
use arrow::datatypes::{
    ArrowPrimitiveType, DataType, Field, Int32Type, Schema, SchemaRef, UInt16Type, UInt32Type,
    UInt64Type, UInt8Type,
};
use arrow::record_batch::RecordBatch;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Member fields and display hints for one axis, as read from the catalog.
pub struct AxisMembers {
    pub fields: Vec<(String, MemberValues)>,
    pub is_time: bool,
    /// Dimension titles by language (`en`, `fr`).
    pub names: [Option<String>; 2],
    /// Display field position by language.
    pub references: [Option<usize>; 2],
}

/// Inputs shared by every axis of one export.
pub struct MemberTableInput<'a> {
    pub slugs: &'a [String],
    pub counts: &'a [i64],
    pub projection: &'a [AxisProjection],
    /// 0 for English, 1 for French.
    pub language: usize,
    pub legacy: bool,
    pub table_id: &'a str,
    pub axes: &'a [AxisMembers],
}

/// Schema (without metadata), one batch per included axis, and the ordered
/// schema metadata.
pub struct MemberTable {
    pub schema: SchemaRef,
    pub batches: Vec<RecordBatch>,
    pub metadata: Vec<(String, String)>,
}

const DISPLAY_NAMES: [&[&str]; 2] = [&["english desc"], &["desc français", "french desc"]];
const RESERVED: [&str; 3] = ["dimension", "code", "label"];

/// Python `str.casefold()` for the Latin-1 names IVT member fields use.
fn casefold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            'ß' => out.push_str("ss"),
            'µ' => out.push('μ'),
            _ => out.extend(c.to_lowercase()),
        }
    }
    out
}

fn unsigned_type(max_value: u64) -> DataType {
    if max_value <= u8::MAX as u64 {
        DataType::UInt8
    } else if max_value <= u16::MAX as u64 {
        DataType::UInt16
    } else if max_value <= u32::MAX as u64 {
        DataType::UInt32
    } else {
        DataType::UInt64
    }
}

/// Order used to merge integer `Sort` widths: signed beats unsigned, then
/// wider beats narrower.
fn integer_rank(dtype: &DataType) -> (bool, u32) {
    match dtype {
        DataType::UInt8 => (false, 8),
        DataType::UInt16 => (false, 16),
        DataType::UInt32 => (false, 32),
        DataType::UInt64 => (false, 64),
        DataType::Int64 => (true, 64),
        _ => (false, 0),
    }
}

/// Days since 1970-01-01 to `YYYY-MM-DD`, matching Arrow's Date32 to string cast.
fn format_date(days: i32) -> String {
    // Howard Hinnant's civil_from_days.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    if year < 0 {
        format!("-{:04}-{month:02}-{day:02}", -year)
    } else {
        format!("{year:04}-{month:02}-{day:02}")
    }
}

/// Rows of one axis in output order, as 0-based member positions.
enum Rows<'a> {
    All(usize),
    Some(&'a [usize]),
}

impl Rows<'_> {
    fn len(&self) -> usize {
        match self {
            Rows::All(count) => *count,
            Rows::Some(indices) => indices.len(),
        }
    }

    fn pick<'v, T>(&'v self, values: &'v [T]) -> impl Iterator<Item = &'v T> + 'v {
        let (all, some) = match self {
            Rows::All(_) => (Some(values.iter()), None),
            Rows::Some(indices) => (None, Some(indices.iter().map(|&index| &values[index]))),
        };
        all.into_iter().flatten().chain(some.into_iter().flatten())
    }
}

fn integers<T>(values: &[Option<i64>], rows: &Rows<'_>) -> ArrayRef
where
    T: ArrowPrimitiveType,
    T::Native: TryFrom<i64>,
    PrimitiveArray<T>: From<Vec<Option<T::Native>>>,
{
    // Widened types only hold values that fit; see `Sort` widening below.
    let values: Vec<Option<T::Native>> = rows
        .pick(values)
        .map(|value| value.and_then(|value| T::Native::try_from(value).ok()))
        .collect();
    Arc::new(PrimitiveArray::<T>::from(values))
}

/// Build one field's array at `dtype`.
fn field_array(values: &MemberValues, rows: &Rows<'_>, dtype: &DataType) -> ArrayRef {
    match values {
        MemberValues::Utf8(v) => Arc::new(
            rows.pick(v)
                .map(|value| value.as_deref())
                .collect::<StringArray>(),
        ),
        MemberValues::Float64(v) => Arc::new(rows.pick(v).copied().collect::<Float64Array>()),
        MemberValues::Date32(v) => Arc::new(rows.pick(v).copied().collect::<Date32Array>()),
        MemberValues::Int64(v) => match dtype {
            DataType::UInt8 => integers::<UInt8Type>(v, rows),
            DataType::UInt16 => integers::<UInt16Type>(v, rows),
            DataType::UInt32 => integers::<UInt32Type>(v, rows),
            DataType::UInt64 => integers::<UInt64Type>(v, rows),
            _ => Arc::new(rows.pick(v).copied().collect::<Int64Array>()),
        },
    }
}

fn kind_type(values: &MemberValues) -> DataType {
    match values {
        MemberValues::Utf8(_) => DataType::Utf8,
        MemberValues::Int64(_) => DataType::Int64,
        MemberValues::Float64(_) => DataType::Float64,
        MemberValues::Date32(_) => DataType::Date32,
    }
}

fn kind_name(values: &MemberValues) -> &'static str {
    match values {
        MemberValues::Utf8(_) => "text",
        MemberValues::Int64(_) => "integer",
        MemberValues::Float64(_) => "float",
        MemberValues::Date32(_) => "date",
    }
}

/// Where an axis's label column comes from.
enum Label {
    Field(usize),
    Date(usize),
    Null,
}

struct AxisPlan<'a> {
    group: usize,
    axis: usize,
    rows: Rows<'a>,
    /// Natural type and output column of each field.
    fields: Vec<(DataType, usize)>,
    label: Label,
}

pub fn build(input: MemberTableInput<'_>) -> Result<MemberTable, IvtError> {
    let MemberTableInput {
        slugs,
        counts,
        projection,
        language,
        legacy,
        table_id,
        axes,
    } = input;
    if projection.len() != slugs.len() || counts.len() != slugs.len() {
        return Err(IvtError::Parse(
            "selection does not match the layout".into(),
        ));
    }
    let included: Vec<usize> = (0..slugs.len()).filter(|&a| projection[a].0).collect();
    let dimension_type = DataType::Dictionary(Box::new(DataType::Int32), Box::new(DataType::Utf8));
    let code_type = unsigned_type(counts.iter().copied().max().unwrap_or(0).max(0) as u64);
    let mut columns: Vec<(String, DataType)> = vec![
        ("dimension".into(), dimension_type),
        ("code".into(), code_type.clone()),
        ("label".into(), DataType::Utf8),
    ];
    let mut metadata = vec![(
        "pyivt.dimension_encoding".to_string(),
        "one_based_index".to_string(),
    )];
    if included.is_empty() {
        return Ok(MemberTable {
            schema: schema(&columns),
            batches: Vec::new(),
            metadata,
        });
    }
    if axes.len() != slugs.len() {
        return Err(IvtError::Parse(
            "member fields do not match the layout".into(),
        ));
    }
    metadata.push(("pyivt.table_id".into(), table_id.into()));

    // Pass 1: decide rows, column names, merged types and labels.
    let mut native_names: HashMap<(String, &'static str, usize), usize> = HashMap::new();
    let mut used: HashSet<String> = RESERVED.iter().map(|name| name.to_string()).collect();
    let mut plans = Vec::with_capacity(included.len());
    for (group, &axis) in included.iter().enumerate() {
        let count = counts[axis] as usize;
        let members = &axes[axis];
        let rows = match &projection[axis].1 {
            None => Rows::All(count),
            Some(indices) => {
                if let Some(&bad) = indices.iter().find(|&&index| index >= count) {
                    return Err(IvtError::Parse(format!(
                        "dimension {}: unknown member index {bad}",
                        slugs[axis]
                    )));
                }
                Rows::Some(indices)
            }
        };
        let mut fields = Vec::with_capacity(members.fields.len());
        let mut occurrences: HashMap<(String, &'static str), usize> = HashMap::new();
        for (name, values) in &members.fields {
            if values.len() != count {
                return Err(corrupt_record(format!(
                    "dimension {}, field {name}: incomplete member map",
                    slugs[axis]
                )));
            }
            let folded = casefold(name);
            let kind = kind_name(values);
            let mut dtype = kind_type(values);
            if let (true, MemberValues::Int64(v)) = (folded == "sort", values) {
                let mut selected = rows.pick(v).flatten().copied().peekable();
                let (min, max) = if selected.peek().is_none() {
                    (0, 0)
                } else {
                    selected.fold((i64::MAX, i64::MIN), |(lo, hi), value| {
                        (lo.min(value), hi.max(value))
                    })
                };
                if min >= 0 {
                    dtype = unsigned_type(max as u64);
                }
            }
            let occurrence = occurrences.entry((folded.clone(), kind)).or_insert(0);
            let key = (folded.clone(), kind, *occurrence);
            *occurrence += 1;
            let column = match native_names.get(&key) {
                Some(&column) => {
                    let current = &mut columns[column].1;
                    if *current != dtype && integer_rank(&dtype) > integer_rank(current) {
                        *current = dtype.clone();
                    }
                    column
                }
                None => {
                    let base = if RESERVED.contains(&folded.as_str()) {
                        format!("native_{name}")
                    } else {
                        name.clone()
                    };
                    let mut column = base.clone();
                    let mut suffix = 2;
                    while used.contains(&casefold(&column)) {
                        column = format!("{base}_{suffix}");
                        suffix += 1;
                    }
                    used.insert(casefold(&column));
                    columns.push((column, dtype.clone()));
                    native_names.insert(key, columns.len() - 1);
                    columns.len() - 1
                }
            };
            fields.push((dtype, column));
        }
        let label = choose_label(members, language, legacy, &fields);
        if let Some(title) = &members.names[language] {
            metadata.push((format!("pyivt.label.{}", slugs[axis]), title.clone()));
        }
        plans.push(AxisPlan {
            group,
            axis,
            rows,
            fields,
            label,
        });
    }

    // Pass 2: build each axis's arrays at their final column types.
    let schema = schema(&columns);
    let dictionary = Arc::new(StringArray::from_iter_values(
        included.iter().map(|&axis| slugs[axis].as_str()),
    ));
    let mut batches = Vec::with_capacity(plans.len());
    for plan in plans {
        let length = plan.rows.len();
        let members = &axes[plan.axis];
        let mut arrays: Vec<Option<ArrayRef>> = vec![None; columns.len()];
        let keys = Int32Array::from(vec![plan.group as i32; length]);
        arrays[0] = Some(Arc::new(
            DictionaryArray::<Int32Type>::try_new(keys, dictionary.clone())
                .map_err(|e| IvtError::Parse(e.to_string()))?,
        ));
        arrays[1] = Some(codes(&plan.rows, &code_type));
        for ((_, values), (_, column)) in members.fields.iter().zip(&plan.fields) {
            arrays[*column] = Some(field_array(values, &plan.rows, &columns[*column].1));
        }
        arrays[2] = Some(match plan.label {
            Label::Field(field) => {
                let column = plan.fields[field].1;
                arrays[column].clone().unwrap()
            }
            Label::Date(field) => {
                let MemberValues::Date32(v) = &members.fields[field].1 else {
                    unreachable!("date labels come from date fields")
                };
                Arc::new(
                    plan.rows
                        .pick(v)
                        .map(|value| value.map(format_date))
                        .collect::<StringArray>(),
                )
            }
            Label::Null => new_null_array(&DataType::Utf8, length),
        });
        let arrays = arrays
            .into_iter()
            .zip(&columns)
            .map(|(array, (_, dtype))| array.unwrap_or_else(|| new_null_array(dtype, length)))
            .collect();
        batches.push(
            RecordBatch::try_new(schema.clone(), arrays)
                .map_err(|e| IvtError::Parse(e.to_string()))?,
        );
    }
    Ok(MemberTable {
        schema,
        batches,
        metadata,
    })
}

fn choose_label(
    members: &AxisMembers,
    language: usize,
    legacy: bool,
    fields: &[(DataType, usize)],
) -> Label {
    if members.is_time {
        return fields
            .iter()
            .position(|(dtype, _)| *dtype == DataType::Date32)
            .map_or(Label::Null, Label::Date);
    }
    if let Some(reference) = members.references[language] {
        if let Some((name, values)) = members.fields.get(reference) {
            let folded = casefold(name);
            if matches!(values, MemberValues::Utf8(_))
                && !DISPLAY_NAMES[1 - language].contains(&folded.as_str())
                && (legacy || folded != "code")
            {
                return Label::Field(reference);
            }
        }
    }
    // The last text field with a given name wins, as in a dict keyed by name.
    for wanted in DISPLAY_NAMES[language] {
        if let Some(field) = members.fields.iter().rposition(|(name, values)| {
            matches!(values, MemberValues::Utf8(_)) && casefold(name) == *wanted
        }) {
            return Label::Field(field);
        }
    }
    Label::Null
}

fn codes(rows: &Rows<'_>, dtype: &DataType) -> ArrayRef {
    macro_rules! build {
        ($array:ty, $native:ty) => {
            Arc::new(match rows {
                Rows::All(count) => (1..=*count as $native).collect::<$array>(),
                Rows::Some(indices) => indices
                    .iter()
                    .map(|&index| (index + 1) as $native)
                    .collect::<$array>(),
            }) as ArrayRef
        };
    }
    match dtype {
        DataType::UInt8 => build!(UInt8Array, u8),
        DataType::UInt16 => build!(UInt16Array, u16),
        DataType::UInt32 => build!(UInt32Array, u32),
        _ => build!(UInt64Array, u64),
    }
}

fn schema(columns: &[(String, DataType)]) -> SchemaRef {
    Arc::new(Schema::new(
        columns
            .iter()
            .map(|(name, dtype)| Field::new(name, dtype.clone(), true))
            .collect::<Vec<_>>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_format_like_arrow() {
        assert_eq!(format_date(0), "1970-01-01");
        assert_eq!(format_date(-1), "1969-12-31");
        assert_eq!(format_date(11_016), "2000-02-29");
        assert_eq!(format_date(-719_162), "0001-01-01");
        assert_eq!(format_date(2_932_896), "9999-12-31");
    }

    #[test]
    fn casefold_matches_python_for_latin1() {
        assert_eq!(casefold("Desc Français"), "desc français");
        assert_eq!(casefold("STRAßE"), "strasse");
        assert_eq!(casefold("µ"), "μ");
    }

    fn axis(fields: Vec<(&str, MemberValues)>) -> AxisMembers {
        AxisMembers {
            fields: fields
                .into_iter()
                .map(|(name, values)| (name.to_string(), values))
                .collect(),
            is_time: false,
            names: [Some("Axis".into()), None],
            references: [None, None],
        }
    }

    #[test]
    fn sort_widens_to_the_largest_type_across_axes() {
        let slugs = vec!["a".to_string(), "b".to_string()];
        let axes = vec![
            axis(vec![(
                "Sort",
                MemberValues::Int64(vec![Some(1), Some(300)]),
            )]),
            axis(vec![("SORT", MemberValues::Int64(vec![Some(2), None]))]),
        ];
        let table = build(MemberTableInput {
            slugs: &slugs,
            counts: &[2, 2],
            projection: &[(true, None), (true, None)],
            language: 0,
            legacy: false,
            table_id: "id",
            axes: &axes,
        })
        .unwrap();
        assert_eq!(table.schema.field(3).name(), "Sort");
        assert_eq!(table.schema.field(3).data_type(), &DataType::UInt16);
        assert_eq!(table.batches[1].column(3).data_type(), &DataType::UInt16);
    }

    #[test]
    fn reserved_and_duplicate_names_get_unique_columns() {
        let slugs = vec!["a".to_string()];
        let text = || MemberValues::Utf8(vec![Some("x".into())]);
        let axes = vec![axis(vec![
            ("Code", text()),
            ("English Desc", text()),
            ("english desc", text()),
            ("native_code", text()),
        ])];
        let table = build(MemberTableInput {
            slugs: &slugs,
            counts: &[1],
            projection: &[(true, None)],
            language: 0,
            legacy: false,
            table_id: "id",
            axes: &axes,
        })
        .unwrap();
        let names: Vec<_> = table
            .schema
            .fields()
            .iter()
            .map(|f| f.name().clone())
            .collect();
        assert_eq!(
            names,
            [
                "dimension",
                "code",
                "label",
                "native_Code",
                "English Desc",
                "english desc_2",
                "native_code_2"
            ]
        );
    }
}
