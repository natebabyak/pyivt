//! PyO3 wrappers for the public Python API.

use pyo3::exceptions::{PyKeyError, PyOSError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes, PyDict, PyList, PyString, PyTuple};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::arrow_export::stream_capsule;
use crate::compact::{AxisProjection, CompactReader, Plan};
use crate::decode::{build_descriptor_layout, Layout};
use crate::error::{ErrorKind, IvtError};
use crate::io::{SharedBytes, WeakSharedBytes};
use crate::metadata::{
    self, Dimension as CoreDimension, Footnote as CoreFootnote, Geographies as CoreGeographies,
    IvtMetadata as CoreMeta, LocalizedText as CoreLocalized, MissingValue as CoreMissing,
};
use crate::stream::{preflight, BatchReader};
use crate::tidy::TidySchema;
use arrow::record_batch::{RecordBatch, RecordBatchIterator};

/// Raise the `pyivt` exception class for the error's kind, with its
/// structured fields as keyword attributes. The kind decides the class; the
/// message text is never inspected.
fn map_ivt_err(e: IvtError) -> PyErr {
    if let IvtError::Io(io) = e {
        return PyOSError::new_err(io.to_string());
    }
    Python::with_gil(|py| typed_error(py, &e).unwrap_or_else(|err| err))
}

fn typed_error(py: Python<'_>, e: &IvtError) -> PyResult<PyErr> {
    let class = match e.kind() {
        ErrorKind::Corrupt => "CorruptFileError",
        ErrorKind::Unsupported => "UnsupportedFormatError",
        ErrorKind::ResourceLimit => "ResourceLimitError",
        ErrorKind::Io | ErrorKind::Other => "IvtError",
    };
    let kwargs = PyDict::new(py);
    let context = e.context();
    for (name, value) in [
        ("offset", context.offset),
        ("length", context.length),
        ("file_len", context.file_len),
        ("slot", context.slot.map(|v| v as u64)),
        ("entry", context.entry.map(|v| v as u64)),
        ("dimension", context.dimension.map(|v| v as u64)),
    ] {
        if let Some(value) = value {
            kwargs.set_item(name, value)?;
        }
    }
    if let IvtError::ResourceLimit {
        estimate, limit, ..
    } = e
    {
        kwargs.set_item("estimate", *estimate)?;
        kwargs.set_item("limit", *limit)?;
    }
    let value = py
        .import("pyivt._errors")?
        .getattr(class)?
        .call((e.to_string(),), Some(&kwargs))?;
    Ok(PyErr::from_value(value))
}

fn resource_limit(message: &str, estimate: Option<u64>, limit: Option<u64>) -> PyErr {
    map_ivt_err(IvtError::resource_limit(message, estimate, limit))
}

/// English and French versions of a metadata text field.
///
/// Both constructor arguments default to `None`. Translations are read-only:
/// use `.en` / `.fr` or `["en"]` / `["fr"]`. A missing translation is `None`,
/// with no automatic fallback. Unsupported language keys raise `KeyError`.
///
/// ```python
/// text = LocalizedText(en="Population", fr="Population")
/// english = text.en
/// french = text["fr"]
/// preferred = text.fr or text.en
/// ```
#[pyclass(name = "LocalizedText", module = "pyivt")]
#[derive(Clone)]
pub struct LocalizedText {
    /// English text, or None when unavailable. No automatic translation fallback.
    #[pyo3(get)]
    pub en: Option<String>,
    /// French text, or None when unavailable. No automatic translation fallback.
    #[pyo3(get)]
    pub fr: Option<String>,
}

impl From<CoreLocalized> for LocalizedText {
    fn from(v: CoreLocalized) -> Self {
        Self { en: v.en, fr: v.fr }
    }
}

#[pymethods]
impl LocalizedText {
    #[new]
    #[pyo3(signature = (en=None, fr=None))]
    fn new(en: Option<String>, fr: Option<String>) -> Self {
        Self { en, fr }
    }

    fn __getitem__(&self, language: &str) -> PyResult<Option<String>> {
        match language {
            "en" => Ok(self.en.clone()),
            "fr" => Ok(self.fr.clone()),
            _ => Err(PyKeyError::new_err(language.to_owned())),
        }
    }

    fn __repr__(&self) -> String {
        format!("LocalizedText(en={:?}, fr={:?})", self.en, self.fr)
    }
}

/// English and French metadata label lists in member ID order.
///
/// Read-only translations support .en / .fr and ["en"] / ["fr"]. Missing
/// translations are None; unsupported keys raise KeyError. Lists are copies.
///
/// ```python
/// labels = ivt.metadata.dimensions[0].members
/// for member_id, label in enumerate(labels.en or [], start=1):
///     print(member_id, label)
/// ```
#[pyclass(name = "LocalizedLabels", module = "pyivt")]
#[derive(Clone)]
pub struct LocalizedLabels {
    /// English labels, or None when unavailable.
    #[pyo3(get)]
    pub en: Option<Vec<String>>,
    /// French labels, or None when unavailable.
    #[pyo3(get)]
    pub fr: Option<Vec<String>>,
}

#[pymethods]
impl LocalizedLabels {
    #[new]
    #[pyo3(signature = (en=None, fr=None))]
    fn new(en: Option<Vec<String>>, fr: Option<Vec<String>>) -> Self {
        Self { en, fr }
    }

    fn __getitem__(&self, language: &str) -> PyResult<Option<Vec<String>>> {
        match language {
            "en" => Ok(self.en.clone()),
            "fr" => Ok(self.fr.clone()),
            _ => Err(PyKeyError::new_err(language.to_owned())),
        }
    }

    fn __repr__(&self) -> String {
        format!("LocalizedLabels(en={:?}, fr={:?})", self.en, self.fr)
    }
}

/// One table axis, obtained from IvtMetadata.dimensions in file order.
///
/// All attributes are read-only. Member label lists, when available, follow
/// file order. Labels may retain leading
/// spaces used by Beyond 20/20 to express hierarchy. None means unavailable.
/// Use the slug as the Arrow column name and selection keyword.
///
/// ```python
/// for dim in ivt.metadata.dimensions:
///     print(dim.name.en, dim.type, dim.items)
///     if dim.type == "time_series":
///         print(dim.members.en)
/// ```
#[pyclass(name = "Dimension", module = "pyivt")]
#[derive(Clone)]
pub struct Dimension {
    /// Localized short name; English falls back to the descriptor name.
    #[pyo3(get)]
    pub name: LocalizedText,
    /// Dimension key for Arrow columns and selection.
    #[pyo3(get)]
    pub slug: String,
    /// Declared number of members on this axis.
    #[pyo3(get)]
    pub items: i64,
    /// Axis classification: `"geographic"`, `"normal"`, or `"time_series"`.
    ///
    /// ```python
    /// time_axes = [dim for dim in ivt.metadata.dimensions if dim.type == "time_series"]
    /// ```
    #[pyo3(get, name = "type")]
    pub dimension_type: &'static str,
    /// Localized description; English falls back to name.en when unavailable.
    #[pyo3(get)]
    pub description: LocalizedText,
    /// Localized member labels in member ID order.
    #[pyo3(get)]
    pub members: LocalizedLabels,
    /// Localized dimension notes with normalized whitespace; blank notes are None.
    #[pyo3(get)]
    pub notes: LocalizedText,
}

impl Dimension {
    fn from_core(d: CoreDimension, slug: String) -> Self {
        Self {
            slug,
            name: LocalizedText {
                en: Some(d.name_en),
                fr: d.name_fr,
            },
            items: d.count,
            dimension_type: d.dimension_type.as_str(),
            description: LocalizedText {
                en: d.description_en,
                fr: d.description_fr,
            },
            members: LocalizedLabels {
                en: d.members_en,
                fr: d.members_fr,
            },
            notes: LocalizedText {
                en: d.notes_en,
                fr: d.notes_fr,
            },
        }
    }
}

#[pymethods]
impl Dimension {
    fn __repr__(&self) -> String {
        format!("Dimension(name={:?}, items={})", self.name.en, self.items)
    }
}

/// One annotation legend entry with English and French descriptions.
///
/// Obtain entries from `IvtMetadata.footnotes`, in legend order. Each entry
/// contains both translations in `description`; a missing translation is `None`.
/// Numbered notes embedded in table or dimension notes are separate from this
/// legend. Attributes are read-only; instances cannot be constructed directly.
///
/// ```python
/// for note in ivt.metadata.footnotes:
///     print(note.symbol, note.description.en, note.description["fr"])
/// ```
#[pyclass(name = "Footnote", module = "pyivt")]
#[derive(Clone)]
pub struct Footnote {
    /// Display symbol identifying this annotation in the table legend.
    #[pyo3(get)]
    pub symbol: String,
    /// Numeric legend entry code, or 0 if the code is unavailable.
    #[pyo3(get)]
    pub number: i64,
    /// Localized annotation description; either translation may be `None`.
    #[pyo3(get)]
    pub description: LocalizedText,
}

impl From<CoreFootnote> for Footnote {
    fn from(f: CoreFootnote) -> Self {
        Self {
            symbol: f.symbol,
            number: f.number,
            description: f.description.into(),
        }
    }
}

/// A missing-value or status legend entry from IvtMetadata.missing_values.
///
/// The optional numeric code links this entry to Arrow's status_code column.
/// The symbol and localized description explain the file-defined status. Attributes
/// are read-only; instances cannot be constructed directly.
///
/// ```python
/// for missing in ivt.metadata.missing_values:
///     print(missing.code, missing.symbol, missing.description.en)
/// ```
#[pyclass(name = "MissingValue", module = "pyivt")]
#[derive(Clone)]
pub struct MissingValue {
    /// File-defined display symbol for this status.
    #[pyo3(get)]
    pub symbol: String,
    /// English and French descriptions; either translation may be None.
    #[pyo3(get)]
    pub description: LocalizedText,
    /// Numeric status code used by Arrow output, or None when unavailable.
    #[pyo3(get)]
    pub code: Option<i64>,
}

impl From<CoreMissing> for MissingValue {
    fn from(m: CoreMissing) -> Self {
        Self {
            symbol: m.symbol,
            description: m.description.into(),
            code: m.code,
        }
    }
}

/// Parallel geography member lists from IvtMetadata.geographies.
///
/// Position i in each available list describes member_id[i]. A list may be None
/// when the field is unavailable; optional entries within a list may also be
/// None. Names, UIDs and quality fields are parsed from English member labels,
/// rather than looked up in an external geography database. Attributes are
/// read-only; instances cannot be constructed directly.
#[pyclass(name = "Geographies", module = "pyivt")]
#[derive(Clone)]
pub struct Geographies {
    /// One-based geography member IDs in file order.
    #[pyo3(get)]
    pub member_id: Vec<i64>,
    /// Original geography labels in English and French.
    #[pyo3(get)]
    pub geo_label: LocalizedLabels,
    /// Names parsed from English labels, or None when labels are unavailable.
    #[pyo3(get)]
    pub geo_name: Option<Vec<Option<String>>>,
    /// Geographic identifiers parsed from labels; strings preserve leading zeros.
    #[pyo3(get)]
    pub geo_uid: Option<Vec<Option<String>>>,
    /// Data quality flag codes parsed from labels; unavailable fields are None.
    #[pyo3(get)]
    pub dqf_code: Option<Vec<Option<String>>>,
    /// Short-form total non-response rates as strings with decimal points, or None.
    #[pyo3(get)]
    pub tnr_short_form: Option<Vec<Option<String>>>,
}

impl From<CoreGeographies> for Geographies {
    fn from(g: CoreGeographies) -> Self {
        Self {
            member_id: g.member_id,
            geo_label: LocalizedLabels {
                en: g.geo_label,
                fr: g.geo_label_fr,
            },
            geo_name: g.geo_name,
            geo_uid: g.geo_uid,
            dqf_code: g.dqf_code,
            tnr_short_form: g.tnr_short_form,
        }
    }
}

#[pymethods]
impl Geographies {
    fn __repr__(&self) -> String {
        format!(
            "Geographies(member_id_len={}, geo_label={})",
            self.member_id.len(),
            self.geo_label.en.is_some()
        )
    }
}

/// Read-only table metadata, obtained from Ivt.metadata.
///
/// Summary properties are available after open(). Dimensions, geographies,
/// footnotes and missing_values are loaded on first access and cached. Accessing
/// these lazy properties may raise pyivt.IvtError if metadata cannot be read or
/// parsed. Returned lists and nested objects are snapshots; modifying a Python
/// list does not update the reader or source file. Optional fields use None
/// when unavailable. Instances cannot be constructed directly.
///
/// ```python
/// import pyivt
///
/// meta = pyivt.open("table.ivt").metadata
/// title = meta.title.en or meta.title.fr
/// axes = [(dim.name.en, dim.items) for dim in meta.dimensions]
/// ```
#[pyclass(name = "IvtMetadata", module = "pyivt")]
pub struct IvtMetadata {
    inner: Mutex<CoreMeta>,
    slugs: Vec<String>,
    _source_owner: Option<Py<PyAny>>,
}

#[pymethods]
impl IvtMetadata {
    /// Product identifier extracted from table identity or notes, or None.
    #[getter]
    fn product_id(&self) -> Option<String> {
        self.inner.lock().unwrap().product_id.clone()
    }

    #[getter]
    /// Table title in English and French; either translation may be None.
    fn title(&self) -> LocalizedText {
        self.inner.lock().unwrap().title.clone().into()
    }

    #[getter]
    /// Population or universe described by the table, or None when unavailable.
    fn universe(&self) -> Option<String> {
        self.inner.lock().unwrap().universe.clone()
    }

    #[getter]
    /// Table notes with normalized line endings and whitespace; blank notes are None.
    fn notes(&self) -> LocalizedText {
        self.inner.lock().unwrap().notes.clone().into()
    }

    #[getter]
    /// IVT format version from the file header, not the pyivt package version.
    fn version(&self) -> String {
        self.inner.lock().unwrap().version.clone()
    }

    #[getter]
    /// Four-byte file signature as eight lowercase hexadecimal digits.
    fn signature(&self) -> String {
        self.inner.lock().unwrap().signature.clone()
    }

    #[getter]
    /// Author or producing organization; blank or missing translations are `None`.
    fn author(&self) -> LocalizedText {
        self.inner.lock().unwrap().author.clone().into()
    }

    #[getter]
    /// File-defined category; blank or missing translations are `None`.
    fn category(&self) -> LocalizedText {
        self.inner.lock().unwrap().category.clone().into()
    }

    #[getter]
    /// Keyword text in each language, or `None` when absent or blank.
    ///
    /// Multiple stored keyword strings are joined with `", "` in file order.
    /// No translation fallback is applied.
    ///
    /// ```python
    /// keywords = ivt.metadata.keywords.en
    /// terms = [term.strip() for term in keywords.split(",")] if keywords else []
    /// ```
    fn keywords(&self) -> LocalizedText {
        self.inner.lock().unwrap().keywords.clone().into()
    }

    #[getter]
    /// Product of declared dimension counts, or None when unavailable.
    ///
    /// Includes all member combinations, rather than counting only numeric or
    /// non-missing observations. The summary product saturates at 2**63 - 1.
    fn number_of_datapoints(&self) -> Option<i64> {
        self.inner.lock().unwrap().number_of_datapoints
    }

    #[getter]
    /// Time-series frequency: monthly, quarterly, semi_annual, or annual.
    ///
    /// None when the table is not a time series or its rate is unknown.
    fn frequency(&self) -> Option<&'static str> {
        self.inner
            .lock()
            .unwrap()
            .frequency
            .map(|rate| rate.as_str())
    }

    #[getter]
    /// Time-series start date as an ISO date string, or None when unavailable.
    fn start_date(&self) -> Option<String> {
        self.inner.lock().unwrap().start_date.clone()
    }

    #[getter]
    /// Time-series end date as an ISO date string, or None when unavailable.
    fn end_date(&self) -> Option<String> {
        self.inner.lock().unwrap().end_date.clone()
    }

    #[getter]
    /// File's stored read-only flag, or None; does not report filesystem permissions.
    fn read_only(&self) -> Option<bool> {
        self.inner.lock().unwrap().read_only
    }

    #[getter]
    /// Dimensions in file order, loaded on first access and cached.
    ///
    /// Returns a list of Dimension snapshots with member labels and notes.
    /// Raises pyivt.IvtError if the required metadata cannot be loaded or parsed.
    fn dimensions(&self) -> PyResult<Vec<Dimension>> {
        let mut guard = self.inner.lock().unwrap();
        let dims = guard.dimensions().map_err(map_ivt_err)?;
        Ok(dims
            .iter()
            .cloned()
            .zip(self.slugs.iter().cloned())
            .map(|(dimension, slug)| Dimension::from_core(dimension, slug))
            .collect())
    }

    #[getter]
    /// Geography member lists, loaded on first access and cached.
    ///
    /// Returns Geographies for the axis selected by the geographic dimension.
    /// Unavailable fields are None. Raises pyivt.IvtError on metadata loading errors.
    fn geographies(&self) -> PyResult<Geographies> {
        let mut guard = self.inner.lock().unwrap();
        Ok(guard.geographies().map_err(map_ivt_err)?.clone().into())
    }

    #[getter]
    /// Annotation legend entries, loaded on first access and cached.
    ///
    /// Returns one `Footnote` per legend entry, with a localized `description`,
    /// or an empty list when no entries are found. Raises `pyivt.IvtError` on errors.
    ///
    /// ```python
    /// for note in ivt.metadata.footnotes:
    ///     print(note.symbol, note.description.en, note.description.fr)
    /// ```
    fn footnotes(&self) -> PyResult<Vec<Footnote>> {
        let mut guard = self.inner.lock().unwrap();
        let notes = guard.footnotes().map_err(map_ivt_err)?;
        Ok(notes.iter().cloned().map(Footnote::from).collect())
    }

    #[getter]
    /// Status legend entries, loaded on first access and cached.
    ///
    /// Returns MissingValue entries linking numeric codes to symbols and
    /// localized descriptions, or an empty list if none are found. Raises
    /// pyivt.IvtError on loading errors. Use the Arrow C stream for per-cell statuses.
    fn missing_values(&self) -> PyResult<Vec<MissingValue>> {
        let mut guard = self.inner.lock().unwrap();
        let vals = guard.missing_values().map_err(map_ivt_err)?;
        Ok(vals.iter().cloned().map(MissingValue::from).collect())
    }

    /// Internal diagnostic: whether a named lazy metadata field is cached.
    fn _loaded(&self, name: &str) -> bool {
        self.inner.lock().unwrap().loaded(name)
    }

    /// Internal: bytes read / read ops during metadata I/O (for benchmarks).
    fn _io_metrics(&self) -> (u64, u64, usize, bool) {
        let m = self.inner.lock().unwrap().io_metrics();
        (m.bytes_read, m.read_ops, m.largest_range, m.fallback_used)
    }
}

/// Read-only decoding layout, obtained from Ivt._layout.
///
/// counts and slugs follow file dimension order and describe the exported grid.
/// geo_dim and straddle are one-based dimension indices; subtract 1 before
/// indexing a Python list. Paging fields describe IVT storage, not Arrow record
/// batches. List attributes are copies. Instances cannot be constructed directly.
#[pyclass(name = "_Layout", module = "pyivt._native")]
#[derive(Clone)]
pub struct PyLayout {
    /// Member counts in file dimension order; their product is the Arrow row count.
    #[pyo3(get)]
    pub counts: Vec<i64>,
    /// Unique dimension slugs for Arrow columns and member selection.
    #[pyo3(get)]
    pub slugs: Vec<String>,
    /// Number of dimensions; equals len(counts) and len(slugs).
    #[pyo3(get)]
    pub n_dim: usize,
    /// One-based geography axis from the native kind or UID field; None when absent.
    #[pyo3(get)]
    pub geo_dim: Option<usize>,
    /// One-based index of the axis split across IVT page windows.
    #[pyo3(get)]
    pub straddle: usize,
    /// Number of page windows covering the straddle axis, not the total page count.
    #[pyo3(get)]
    pub window_count: i64,
}

impl From<&Layout> for PyLayout {
    fn from(l: &Layout) -> Self {
        Self {
            counts: l.counts.clone(),
            slugs: l.slugs.clone(),
            n_dim: l.n_dim,
            geo_dim: l.geo_dim,
            straddle: l.straddle,
            window_count: l.window_count,
        }
    }
}

/// An opened Beyond 20/20 table with metadata and deferred cell decoding.
///
/// Obtain an instance with pyivt.open(source); direct construction is not
/// supported. Inspect metadata and members without decoding cells.
/// to_arrow() returns a new compact table on each call; the reader keeps no
/// reference to it, so dropping the table releases its buffers. iter_batches()
/// streams the same table in bounded memory. No context
/// manager or close() call is required. This reader does not modify the source file.
///
/// ```python
/// import pyivt
///
/// ivt = pyivt.open("table.ivt")
/// title = ivt.metadata.title.en
/// table = ivt.to_arrow()
/// members = ivt.to_member_table()
/// ```
#[pyclass(name = "Ivt", module = "pyivt")]
pub struct Ivt {
    metadata: Py<IvtMetadata>,
    layout: Layout,
    py_layout: PyLayout,
    /// Full file bytes when opened from memory; held for the reader's lifetime.
    data: Mutex<Option<SharedBytes>>,
    /// File bytes read for a path open. Held weakly, so they are freed once no
    /// export or stream uses them; a later export reads the file again.
    loaded: Mutex<Option<WeakSharedBytes>>,
    path: Option<PathBuf>,
    maps: Mutex<Option<Arc<TidySchema>>>,
    member_fields: Mutex<Option<Py<PyAny>>>,
    table_id: Mutex<Option<String>>,
}

#[pymethods]
impl Ivt {
    /// Shared metadata object; detailed member and legend fields load on demand.
    #[getter]
    fn metadata(&self, py: Python<'_>) -> Py<IvtMetadata> {
        self.metadata.clone_ref(py)
    }

    #[getter]
    /// Snapshot of dimension counts, Arrow column slugs and IVT paging details.
    fn _layout(&self) -> PyLayout {
        self.py_layout.clone()
    }

    /// Arrow PyCapsule stream protocol. Each invocation owns an independent
    /// native reader; consumers can release it early without retaining batches.
    #[pyo3(signature = (requested_schema=None))]
    fn _arrow_c_stream<'py>(
        &self,
        py: Python<'py>,
        requested_schema: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, pyo3::types::PyCapsule>> {
        if requested_schema.is_some_and(|s| !s.is_none()) {
            return Err(PyValueError::new_err(
                "requested_schema is not supported; cast the exported stream explicitly",
            ));
        }
        let reader = self.batch_reader(py, 1_073_741_824)?;
        stream_capsule(py, Box::new(reader))
    }

    /// Materialize the batches as a chunked pyarrow.Table. Each call decodes
    /// afresh and the reader retains no output buffers.
    /// The conservative pre-flight limit includes output and working buffers.
    #[pyo3(signature = (*, max_memory_bytes=2_147_483_648))]
    fn _legacy_to_arrow<'py>(
        &self,
        py: Python<'py>,
        max_memory_bytes: usize,
    ) -> PyResult<Bound<'py, PyAny>> {
        let reader = self.batch_reader(py, max_memory_bytes)?;
        let fixed = reader
            .data
            .len()
            .saturating_mul(3)
            .saturating_add(reader.maps.dictionary_bytes);
        preflight(
            reader.total,
            reader.maps.bytes_per_row,
            fixed,
            max_memory_bytes,
        )
        .map_err(map_ivt_err)?;
        let schema = Arc::clone(&reader.maps.schema);
        let mut reader = reader;
        let batches = py
            .allow_threads(|| {
                std::iter::from_fn(|| reader.next_batch()).collect::<Result<Vec<_>, IvtError>>()
            })
            .map_err(map_ivt_err)?;
        let capsule = stream_capsule(
            py,
            Box::new(RecordBatchIterator::new(
                batches.into_iter().map(Ok),
                schema,
            )),
        )?;
        let _profile = crate::profile::Stage::new("arrow_table");
        py.import("pyarrow")?
            .getattr("RecordBatchReader")?
            .call_method1("_import_from_c_capsule", (capsule,))?
            .call_method0("read_all")
    }

    /// Export 1-based member positions and nullable float64 values in file order.
    /// Each call builds new buffers that the reader does not retain. Path opens
    /// read the file for the export and release it afterwards.
    #[pyo3(signature = (**dims))]
    fn to_arrow<'py>(
        slf: Bound<'py, Self>,
        py: Python<'py>,
        dims: Option<Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let (plan, data) = Self::compact_plan(&slf, py, dims)?;
        let this: &Self = &slf.borrow();
        let batch = py
            .allow_threads(|| -> Result<RecordBatch, IvtError> {
                crate::compact::preflight(
                    plan.rows,
                    plan.bytes_per_row(),
                    data.len(),
                    crate::compact::MEMORY_LIMIT,
                )?;
                plan.batch(&data, &this.layout, 0, plan.rows)
            })
            .map_err(map_ivt_err)?;
        drop(data);
        let schema = batch.schema();
        let capsule = stream_capsule(
            py,
            Box::new(RecordBatchIterator::new(
                vec![Ok(batch)].into_iter(),
                schema,
            )),
        )?;
        let _profile = crate::profile::Stage::new("arrow_table");
        py.import("pyarrow")?
            .getattr("RecordBatchReader")?
            .call_method1("_import_from_c_capsule", (capsule,))?
            .call_method0("read_all")
    }

    /// Stream the to_arrow() table as a pyarrow.RecordBatchReader of at most
    /// batch_size rows per batch, in file order, with the same schema and
    /// metadata. Batches are decoded on demand without the GIL; the stream
    /// keeps one reference to the file contents and no earlier batches.
    #[pyo3(signature = (*, batch_size=DEFAULT_BATCH_ROWS, **dims))]
    fn iter_batches<'py>(
        slf: Bound<'py, Self>,
        py: Python<'py>,
        batch_size: usize,
        dims: Option<Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        if batch_size == 0 {
            return Err(PyValueError::new_err("batch_size must be positive"));
        }
        let (plan, data) = Self::compact_plan(&slf, py, dims)?;
        let layout = slf.borrow().layout.clone();
        let reader = CompactReader::new(data, layout, plan, batch_size).map_err(map_ivt_err)?;
        let capsule = stream_capsule(py, Box::new(reader))?;
        py.import("pyarrow")?
            .getattr("RecordBatchReader")?
            .call_method1("_import_from_c_capsule", (capsule,))
    }

    /// Export one Arrow member lookup with a dictionary-encoded dimension column.
    #[pyo3(signature = (*, language="en", **dims))]
    fn to_member_table<'py>(
        slf: Bound<'py, Self>,
        py: Python<'py>,
        language: &str,
        dims: Option<Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        if !matches!(language, "en" | "fr") {
            return Err(PyValueError::new_err("language must be 'en' or 'fr'"));
        }
        let export = py.import("pyivt._export")?;
        let prepared = export
            .getattr("prepare")?
            .call1((&slf, dims.unwrap_or_else(|| PyDict::new(py))))?;
        export
            .getattr("build_member_table")?
            .call1((slf, prepared, language))
    }

    /// Private member-only bridge with Date32 day offsets and display references.
    fn _member_fields<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        if let Some(fields) = self.member_fields.lock().unwrap().as_ref() {
            return Ok(fields.bind(py).clone());
        }
        let member_estimate = self.layout.counts.iter().fold(0usize, |n, &count| {
            n.saturating_add((count as usize).saturating_mul(128))
        });
        compact_preflight(0, member_estimate)?;
        let metadata = self.metadata.borrow(py);
        let inner = &metadata.inner;
        let fields = py
            .allow_threads(|| {
                // Member records live in the metadata catalog built at open;
                // value pages are never needed here.
                let guard = inner.lock().unwrap();
                let data = guard.catalog()?;
                let axis_kinds = guard
                    .desc()
                    .dims
                    .iter()
                    .map(|dimension| dimension.type_code == 6)
                    .collect::<Vec<_>>();
                self.layout
                    .counts
                    .iter()
                    .enumerate()
                    .map(|(axis, &count)| {
                        let (fields, positions) =
                            crate::members::read_typed_members_with_positions(
                                data,
                                axis,
                                count as usize,
                            )?;
                        let (names, references) = metadata::member_display(data, axis, &positions);
                        let is_time = axis_kinds.get(axis).copied().unwrap_or(false);
                        Ok::<_, IvtError>((fields, (is_time, names, references)))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(map_ivt_err)?;
        drop(metadata);
        let estimated = fields
            .iter()
            .flat_map(|(fields, _)| fields)
            .fold(0usize, |n, (_, values)| {
                n.saturating_add(values.len().saturating_mul(128))
            });
        compact_preflight(0, estimated)?;
        let digest = self.table_id(py)?;
        let axes = PyList::empty(py);
        let displays = PyList::empty(py);
        for (axis, display) in fields {
            displays.append(display)?;
            let converted = PyList::empty(py);
            for (name, values) in axis {
                use crate::members::MemberValues;
                let (kind, values): (&str, Bound<'py, PyAny>) = match values {
                    MemberValues::Utf8(v) => ("text", v.into_pyobject(py)?.into_any()),
                    MemberValues::Int64(v) => ("integer", v.into_pyobject(py)?.into_any()),
                    MemberValues::Float64(v) => ("float", v.into_pyobject(py)?.into_any()),
                    MemberValues::Date32(v) => ("date", v.into_pyobject(py)?.into_any()),
                };
                converted.append((name, kind, values))?;
            }
            axes.append(converted)?;
        }
        let result = PyTuple::new(
            py,
            [
                digest.into_pyobject(py)?.into_any(),
                axes.into_any(),
                displays.into_any(),
            ],
        )?
        .into_any();
        *self.member_fields.lock().unwrap() = Some(result.clone().unbind());
        Ok(result)
    }
}

enum OpenSource {
    Path(PathBuf),
    Bytes(SharedBytes),
}

/// Keeps a Python `bytes` object alive and exposes its buffer without the GIL.
struct PyBytesOwner {
    _object: Py<PyBytes>,
    ptr: *const u8,
    len: usize,
}

// SAFETY: a `bytes` object is immutable and its buffer never moves or is freed
// while a strong reference (`_object`) is held, so the pointer stays valid and
// may be read from any thread without the GIL. `Py<T>` itself is Send + Sync.
unsafe impl Send for PyBytesOwner {}
unsafe impl Sync for PyBytesOwner {}

impl AsRef<[u8]> for PyBytesOwner {
    fn as_ref(&self) -> &[u8] {
        // SAFETY: see the Send/Sync justification above.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

/// Borrow an immutable `bytes` object's buffer instead of copying it.
fn borrow_bytes(b: &Bound<'_, PyBytes>) -> SharedBytes {
    let view = b.as_bytes();
    SharedBytes::from_owner(PyBytesOwner {
        ptr: view.as_ptr(),
        len: view.len(),
        _object: b.clone().unbind(),
    })
}

fn resolve_source(_py: Python<'_>, source: Bound<'_, PyAny>) -> PyResult<OpenSource> {
    if let Ok(b) = source.downcast::<PyBytes>() {
        return Ok(OpenSource::Bytes(borrow_bytes(b)));
    }
    if let Ok(ba) = source.downcast::<PyByteArray>() {
        // A bytearray can change after open(); snapshot it once.
        return Ok(OpenSource::Bytes(ba.to_vec().into()));
    }
    if source.hasattr("read")? {
        let data = source.call_method0("read")?;
        if let Ok(b) = data.downcast::<PyBytes>() {
            return Ok(OpenSource::Bytes(borrow_bytes(b)));
        }
        if let Ok(ba) = data.downcast::<PyByteArray>() {
            return Ok(OpenSource::Bytes(ba.to_vec().into()));
        }
        if let Ok(bytes) = data.call_method0("__bytes__") {
            if let Ok(b) = bytes.downcast::<PyBytes>() {
                return Ok(OpenSource::Bytes(borrow_bytes(b)));
            }
        }
        return Err(PyTypeError::new_err(format!(
            "file-like source must return bytes; got {}",
            data.get_type().name()?
        )));
    }
    if let Ok(s) = source.downcast::<PyString>() {
        return Ok(OpenSource::Path(PathBuf::from(s.to_str()?)));
    }
    if source.hasattr("__fspath__")? {
        let p = source.call_method0("__fspath__")?;
        return Ok(OpenSource::Path(PathBuf::from(p.extract::<&str>()?)));
    }
    if let Ok(bytes) = source.call_method0("__bytes__") {
        if let Ok(b) = bytes.downcast::<PyBytes>() {
            return Ok(OpenSource::Bytes(borrow_bytes(b)));
        }
    }
    Err(PyTypeError::new_err(format!(
        "IVT source must be a path, bytes-like, or binary file object; got {}",
        source.get_type().name()?
    )))
}

/// Rows per iter_batches() batch unless the caller chooses otherwise.
const DEFAULT_BATCH_ROWS: usize = 1_048_576;

/// Coarse check before reading a path open: one copy of the source plus `extra`.
fn compact_preflight(file_size: usize, extra: usize) -> PyResult<()> {
    let estimate = file_size.saturating_add(extra);
    if estimate > crate::compact::MEMORY_LIMIT {
        return Err(resource_limit(
            "memory estimate exceeds the internal 2 GiB safety limit",
            Some(estimate as u64),
            Some(crate::compact::MEMORY_LIMIT as u64),
        ));
    }
    Ok(())
}

impl Ivt {
    /// Validate `dims` and load the source for a compact export.
    fn compact_plan<'py>(
        slf: &Bound<'py, Self>,
        py: Python<'py>,
        dims: Option<Bound<'py, PyDict>>,
    ) -> PyResult<(Plan, SharedBytes)> {
        let projection: Vec<AxisProjection> = py
            .import("pyivt._export")?
            .getattr("prepare")?
            .call1((slf, dims.unwrap_or_else(|| PyDict::new(py))))?
            .extract()?;
        let this: &Self = &slf.borrow();
        let table_id = this.table_id(py)?;
        let metadata = std::collections::HashMap::from([
            ("pyivt.table_id".into(), table_id),
            ("pyivt.dimension_encoding".into(), "one_based_index".into()),
        ]);
        let plan = Plan::new(&this.layout, projection, metadata).map_err(map_ivt_err)?;
        let data = this.compact_data(py, 0)?;
        Ok((plan, data))
    }

    fn compact_data(&self, py: Python<'_>, extra: usize) -> PyResult<SharedBytes> {
        let size = self.file_size()?;
        compact_preflight(size, extra)?;
        py.allow_threads(|| self.ensure_data()).map_err(map_ivt_err)
    }

    /// `pyivt.table_id`, computed once from header and metadata bytes in Rust
    /// with the GIL released. Shared by `to_arrow()` and `to_member_table()`.
    fn table_id(&self, py: Python<'_>) -> PyResult<String> {
        if let Some(digest) = self.table_id.lock().unwrap().as_ref() {
            return Ok(digest.clone());
        }
        let metadata = self.metadata.borrow(py);
        let inner = &metadata.inner;
        let digest = py
            .allow_threads(|| inner.lock().unwrap().table_identity())
            .map_err(map_ivt_err)?;
        *self.table_id.lock().unwrap() = Some(digest.clone());
        Ok(digest)
    }

    fn file_size(&self) -> PyResult<usize> {
        if let Some(data) = self.data.lock().unwrap().as_ref() {
            return Ok(data.len());
        }
        let size = std::fs::metadata(self.path.as_ref().unwrap())
            .map_err(|e| PyOSError::new_err(e.to_string()))?
            .len();
        usize::try_from(size)
            .map_err(|_| resource_limit("file exceeds addressable memory", Some(size), None))
    }

    fn batch_reader(&self, py: Python<'_>, limit: usize) -> PyResult<BatchReader> {
        let file_len = self.file_size()?;
        preflight(0, 0, file_len.saturating_mul(4), limit).map_err(map_ivt_err)?;
        let data = py
            .allow_threads(|| self.ensure_data())
            .map_err(map_ivt_err)?;
        let legends = self
            .metadata
            .borrow(py)
            .inner
            .lock()
            .unwrap()
            .missing_values()
            .map_err(map_ivt_err)?
            .clone();
        let maps = py
            .allow_threads(|| {
                let mut maps = self.maps.lock().unwrap();
                if maps.is_none() {
                    *maps = Some(Arc::new(TidySchema::new(
                        &data,
                        &self.layout,
                        &legends,
                        limit,
                    )?));
                }
                Ok::<_, IvtError>(Arc::clone(maps.as_ref().unwrap()))
            })
            .map_err(map_ivt_err)?;
        BatchReader::new(data, self.layout.clone(), maps, limit).map_err(map_ivt_err)
    }

    fn ensure_data(&self) -> Result<SharedBytes, IvtError> {
        if let Some(d) = self.data.lock().unwrap().as_ref() {
            return Ok(d.clone());
        }
        // Hold the lock while reading so concurrent exports share one copy.
        let mut loaded = self.loaded.lock().unwrap();
        if let Some(d) = loaded.as_ref().and_then(WeakSharedBytes::upgrade) {
            return Ok(d);
        }
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| IvtError::Parse("IVT has no in-memory data or path".into()))?;
        let bytes = SharedBytes::from(std::fs::read(path).map_err(IvtError::Io)?);
        *loaded = Some(bytes.downgrade());
        Ok(bytes)
    }
}

/// Open a Beyond 20/20 IVT source without decoding its cell values.
///
/// ### Parameters
///
/// `source`: a local path (`str` or `PathLike[str]`), complete IVT contents
/// (`bytes`, `bytearray`, `memoryview`), or a binary file object. Bytes are
/// interpreted as file contents. File objects are read once from their current
/// position and are neither rewound nor closed. Objects implementing
/// `__bytes__()` are also accepted.
///
/// ### Returns
///
/// An `Ivt` reader exposing `metadata`, `to_member_table()`, `to_arrow()` and
/// `iter_batches()`. Summary metadata is parsed immediately. Cells are decoded
/// by `to_arrow()` or, in bounded batches, by `iter_batches()`.
///
/// ### Raises
///
/// - `TypeError`: unsupported source, path representation or `read()` result.
/// - `OSError`: the source path cannot be opened or read.
/// - `pyivt.CorruptFileError`: truncated file, bad signature or header, or a
///   record or directory outside the file.
/// - `pyivt.UnsupportedFormatError`: an IVT container version, descriptor or
///   layout pyivt cannot read.
/// - `pyivt.ResourceLimitError`: a memory estimate exceeds a safety limit.
///
/// All three subclass `pyivt.IvtError`, a `ValueError`.
///
/// ### Notes
///
/// Paths use selective metadata reads. Keep the file available and unchanged:
/// the first export reads its full contents. Lazy metadata properties
/// may read additional ranges. Immutable `bytes` are shared with the reader,
/// not copied. Mutable contents such as a bytearray are copied once when
/// opened, so later changes do not affect the reader. This function does
/// not fetch URLs or decompress archives. Exceptions raised by a supplied
/// read(), __fspath__() or __bytes__() method may also propagate.
///
/// ### Examples
///
/// ```python
/// from io import BytesIO
/// from pathlib import Path
/// import pyivt
///
/// ivt = pyivt.open(Path("table.ivt"))
/// title = ivt.metadata.title.en or ivt.metadata.title.fr
/// table = ivt.to_arrow()
/// members = ivt.to_member_table()
/// in_memory = pyivt.open(BytesIO(Path("table.ivt").read_bytes()))
/// ```
#[pyfunction]
#[pyo3(name = "open")]
#[pyo3(signature = (source, *, _source_owner=None))]
pub fn open_ivt(
    py: Python<'_>,
    source: Bound<'_, PyAny>,
    _source_owner: Option<Py<PyAny>>,
) -> PyResult<Ivt> {
    let src = resolve_source(py, source)?;
    match src {
        OpenSource::Path(path) => {
            let path_clone = path.clone();
            let (meta, layout) = py
                .allow_threads(|| -> Result<(CoreMeta, Layout), IvtError> {
                    let meta = metadata::open_metadata_path(&path_clone)?;
                    let layout = build_descriptor_layout(meta.desc())?;
                    Ok((meta, layout))
                })
                .map_err(map_ivt_err)?;
            let py_layout = PyLayout::from(&layout);
            let metadata = Py::new(
                py,
                IvtMetadata {
                    inner: Mutex::new(meta),
                    slugs: layout.slugs.clone(),
                    _source_owner,
                },
            )?;
            Ok(Ivt {
                metadata,
                layout,
                py_layout,
                data: Mutex::new(None),
                loaded: Mutex::new(None),
                path: Some(path),
                maps: Mutex::new(None),
                member_fields: Mutex::new(None),
                table_id: Mutex::new(None),
            })
        }
        OpenSource::Bytes(data) => {
            let (meta, layout) = py
                .allow_threads(|| -> Result<(CoreMeta, Layout), IvtError> {
                    let meta = metadata::build_metadata(data.clone(), None, None)?;
                    let layout = build_descriptor_layout(meta.desc())?;
                    Ok((meta, layout))
                })
                .map_err(map_ivt_err)?;
            let py_layout = PyLayout::from(&layout);
            let metadata = Py::new(
                py,
                IvtMetadata {
                    inner: Mutex::new(meta),
                    slugs: layout.slugs.clone(),
                    _source_owner,
                },
            )?;
            Ok(Ivt {
                metadata,
                layout,
                py_layout,
                data: Mutex::new(Some(data)),
                loaded: Mutex::new(None),
                path: None,
                maps: Mutex::new(None),
                member_fields: Mutex::new(None),
                table_id: Mutex::new(None),
            })
        }
    }
}
