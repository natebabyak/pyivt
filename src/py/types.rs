//! PyO3 wrappers for the public Python API.

use pyo3::exceptions::{PyKeyError, PyOSError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes, PyString};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::arrow_export::columns_to_pyarrow;
use crate::decode::{build_layout, complete_grid, decode_store, Layout};
use crate::descriptor::parse_descriptor;
use crate::error::IvtError;
use crate::header::read_header;
use crate::metadata::{
    self, Dimension as CoreDimension, Footnote as CoreFootnote, Geographies as CoreGeographies,
    IvtMetadata as CoreMeta, LocalizedText as CoreLocalized, MissingValue as CoreMissing,
};
use crate::tidy::{tidy_arrays, Column};

fn map_ivt_err(e: IvtError) -> PyErr {
    match e {
        IvtError::Io(io) => PyOSError::new_err(io.to_string()),
        other => PyValueError::new_err(other.to_string()),
    }
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
/// member ID order: position 0 describes member ID 1. Labels may retain leading
/// spaces used by Beyond 20/20 to express hierarchy. None means unavailable.
/// Use Ivt.layout.slugs for the corresponding Arrow column names; display names
/// in this object need not match those column names. Instances come from the
/// reader and cannot be constructed directly.
///
/// ```python
/// for dim in ivt.metadata.dimensions:
///     print(dim.name.en, dim.type, dim.count)
///     if dim.type == "time_series":
///         print(dim.members.en)
/// ```
#[pyclass(name = "Dimension", module = "pyivt")]
#[derive(Clone)]
pub struct Dimension {
    /// Localized short name; English falls back to the descriptor name.
    #[pyo3(get)]
    pub name: LocalizedText,
    /// Declared number of members on this axis; member IDs run from 1 to count.
    #[pyo3(get)]
    pub count: i64,
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
    /// Localized dimension notes with normalized whitespace.
    #[pyo3(get)]
    pub notes: LocalizedText,
    /// Optional member ordering information. Currently always None.
    #[pyo3(get)]
    pub ordinal: Option<Vec<i64>>,
}

impl From<CoreDimension> for Dimension {
    fn from(d: CoreDimension) -> Self {
        Self {
            name: LocalizedText {
                en: Some(d.name_en),
                fr: d.name_fr,
            },
            count: d.count,
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
            ordinal: d.ordinal,
        }
    }
}

#[pymethods]
impl Dimension {
    fn __repr__(&self) -> String {
        format!("Dimension(name={:?}, count={})", self.name.en, self.count)
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
    /// Optional scope of the annotation. Currently always None.
    #[pyo3(get)]
    pub scope: Option<String>,
    /// Optional dimension association. Currently always None.
    #[pyo3(get)]
    pub dimension: Option<String>,
    /// Optional member association. Currently always None.
    #[pyo3(get)]
    pub member_id: Option<i64>,
}

impl From<CoreFootnote> for Footnote {
    fn from(f: CoreFootnote) -> Self {
        Self {
            symbol: f.symbol,
            number: f.number,
            description: f.description.into(),
            scope: f.scope,
            dimension: f.dimension,
            member_id: f.member_id,
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
/// these lazy properties may raise ValueError if metadata cannot be read or
/// parsed. Returned lists and nested objects are snapshots; modifying a Python
/// list does not update the reader or source file. Optional fields use None
/// when unavailable. Instances cannot be constructed directly.
///
/// ```python
/// import pyivt
///
/// meta = pyivt.open("table.ivt").metadata
/// title = meta.title.en or meta.title.fr
/// axes = [(dim.name.en, dim.count) for dim in meta.dimensions]
/// ```
#[pyclass(name = "IvtMetadata", module = "pyivt")]
pub struct IvtMetadata {
    inner: Mutex<CoreMeta>,
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
    /// Table notes in both languages, with line endings and whitespace normalized.
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
    /// 'No', 'Annual', 'Semi-annual', 'Quarterly', or 'Monthly'; None if unknown.
    ///
    /// 'No' is the file's explicit non-time-series frequency, distinct from None.
    fn frequency(&self) -> Option<String> {
        self.inner.lock().unwrap().frequency.clone()
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
    /// Raises ValueError if the required metadata cannot be loaded or parsed.
    /// Position i corresponds to Ivt.layout.counts[i] and Ivt.layout.slugs[i].
    fn dimensions(&self) -> PyResult<Vec<Dimension>> {
        let mut guard = self.inner.lock().unwrap();
        let dims = guard.dimensions().map_err(PyValueError::new_err)?;
        Ok(dims.iter().cloned().map(Dimension::from).collect())
    }

    #[getter]
    /// Geography member lists, loaded on first access and cached.
    ///
    /// Returns Geographies for the axis selected by Ivt.layout.geo_dim.
    /// Unavailable fields are None. Raises ValueError on metadata loading errors.
    fn geographies(&self) -> PyResult<Geographies> {
        let mut guard = self.inner.lock().unwrap();
        Ok(guard
            .geographies()
            .map_err(PyValueError::new_err)?
            .clone()
            .into())
    }

    #[getter]
    /// Annotation legend entries, loaded on first access and cached.
    ///
    /// Returns one `Footnote` per legend entry, with a localized `description`,
    /// or an empty list when no entries are found. Raises `ValueError` on errors.
    ///
    /// ```python
    /// for note in ivt.metadata.footnotes:
    ///     print(note.symbol, note.description.en, note.description.fr)
    /// ```
    fn footnotes(&self) -> PyResult<Vec<Footnote>> {
        let mut guard = self.inner.lock().unwrap();
        let notes = guard.footnotes().map_err(PyValueError::new_err)?;
        Ok(notes.iter().cloned().map(Footnote::from).collect())
    }

    #[getter]
    /// Status legend entries, loaded on first access and cached.
    ///
    /// Returns MissingValue entries linking numeric codes to symbols and
    /// localized descriptions, or an empty list if none are found. Raises
    /// ValueError on loading errors. See Ivt.to_arrow for status column behavior.
    fn missing_values(&self) -> PyResult<Vec<MissingValue>> {
        let mut guard = self.inner.lock().unwrap();
        let vals = guard.missing_values().map_err(PyValueError::new_err)?;
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

/// Read-only decoding layout, obtained from Ivt.layout.
///
/// counts and slugs follow file dimension order and describe the exported grid.
/// geo_dim and straddle are one-based dimension indices; subtract 1 before
/// indexing a Python list. Paging fields describe IVT storage, not Arrow record
/// batches. List attributes are copies. Instances cannot be constructed directly.
#[pyclass(name = "Layout", module = "pyivt")]
#[derive(Clone)]
pub struct PyLayout {
    /// Member counts in file dimension order; their product is the Arrow row count.
    #[pyo3(get)]
    pub counts: Vec<i64>,
    /// Unique Arrow member-ID column names; the selected geography axis is 'geo'.
    #[pyo3(get)]
    pub slugs: Vec<String>,
    /// Number of dimensions; equals len(counts) and len(slugs).
    #[pyo3(get)]
    pub n_dim: usize,
    /// One-based index of the selected geography axis; may be a fallback axis.
    #[pyo3(get)]
    pub geo_dim: usize,
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

struct DecodeCache {
    cols: Vec<(String, Column)>,
}

/// An opened Beyond 20/20 table with metadata and deferred cell decoding.
///
/// Obtain an instance with pyivt.open(source); direct construction is not
/// supported. Inspect metadata and layout without exporting the cell grid.
/// to_arrow() reads and decodes the complete table on first use, then caches
/// decoded columns for subsequent exports. No context manager or close() call
/// is required. This reader does not modify the source file.
///
/// ```python
/// import pyivt
///
/// ivt = pyivt.open("table.ivt")
/// title = ivt.metadata.title.en
/// table = ivt.to_arrow()
/// ```
#[pyclass(name = "Ivt", module = "pyivt")]
pub struct Ivt {
    metadata: Py<IvtMetadata>,
    layout: Layout,
    py_layout: PyLayout,
    /// Full file bytes when opened from memory, or filled lazily for path opens on `to_arrow`.
    data: Mutex<Option<Arc<Vec<u8>>>>,
    path: Option<PathBuf>,
    cache: Mutex<Option<DecodeCache>>,
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
    fn layout(&self) -> PyLayout {
        self.py_layout.clone()
    }

    /// Export the complete table as a pyarrow.Table in long form.
    ///
    /// ### Returns
    ///
    /// A `pyarrow.Table` with one row per combination of dimension members,
    /// including combinations absent from storage. Row count is
    /// `math.prod(self.layout.counts)`. Rows follow file dimension order, with
    /// the last dimension varying fastest. Each dimension has an `int64` column
    /// named by `layout.slugs` containing one-based member IDs. File-defined
    /// member fields use `<slug>_<field>` names with collision suffixes where
    /// needed; types are `string`, `int64`, `float64` or `date32`.
    ///
    /// Additional columns:
    ///
    /// * `value` (`float64`): numeric value when `status_code` is 0, otherwise null.
    /// * `status_code` (`int64`): decoded status; 0 denotes a numeric cell.
    /// * `symbol` (`string`): matching status legend symbol, or null.
    /// * `status` (`string`): legend description, preferring English then French,
    ///   or null if no description is available.
    /// * `footnote_raw` (`list<uint16>`): raw cell annotation markers; an empty
    ///   list means no markers. These are not resolved footnote texts.
    ///
    /// ### Raises
    ///
    /// - `OSError`: the path cannot be read when the full file is first needed.
    /// - `ValueError`: decoding, member metadata or dimension validation fails.
    /// - `ImportError`: pyarrow is unavailable.
    ///
    /// ### Notes
    ///
    /// This materializes the entire grid in memory; there is no streaming or
    /// filtering option. Combinations absent from storage have value 0 and
    /// status_code 0. Nonzero statuses produce null values even if a numeric
    /// payload exists. Source precision and label indentation are preserved.
    /// First export caches the file bytes and decoded columns. Later calls
    /// construct another Arrow table from cached columns without re-decoding.
    ///
    /// ### Example
    ///
    /// ```python
    /// import pyarrow.parquet as pq
    /// import pyivt
    ///
    /// table = pyivt.open("table.ivt").to_arrow()
    /// pq.write_table(table, "table.parquet")
    /// ```
    fn to_arrow<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        {
            let cache = self.cache.lock().unwrap();
            if let Some(c) = cache.as_ref() {
                return columns_to_pyarrow(py, &c.cols);
            }
        }

        let data = self.ensure_data(py).map_err(map_ivt_err)?;
        let layout = self.layout.clone();
        let dims = {
            let meta = self.metadata.borrow(py);
            let guard = meta.inner.lock().unwrap();
            guard.desc().dims.clone()
        };

        let cells = py
            .allow_threads(|| -> Result<Vec<(String, Vec<f64>)>, String> {
                layout
                    .counts
                    .iter()
                    .try_fold(1usize, |n, &count| {
                        usize::try_from(count).ok().and_then(|c| n.checked_mul(c))
                    })
                    .ok_or("dimension product exceeds addressable memory")?;
                let store = decode_store(&data, &dims, &layout)?;
                Ok(complete_grid(&store, &layout))
            })
            .map_err(PyValueError::new_err)?;

        let tidy = {
            let meta = self.metadata.borrow(py);
            let mut inner = meta.inner.lock().unwrap();
            tidy_arrays(&data, &cells, &mut inner, &self.layout).map_err(PyValueError::new_err)?
        };

        *self.cache.lock().unwrap() = Some(DecodeCache { cols: tidy.clone() });

        columns_to_pyarrow(py, &tidy)
    }
}

enum OpenSource {
    Path(PathBuf),
    Bytes(Vec<u8>),
}

fn resolve_source(_py: Python<'_>, source: Bound<'_, PyAny>) -> PyResult<OpenSource> {
    if let Ok(b) = source.downcast::<PyBytes>() {
        return Ok(OpenSource::Bytes(b.as_bytes().to_vec()));
    }
    if let Ok(ba) = source.downcast::<PyByteArray>() {
        return Ok(OpenSource::Bytes(ba.to_vec()));
    }
    if source.hasattr("read")? {
        let data = source.call_method0("read")?;
        if let Ok(b) = data.downcast::<PyBytes>() {
            return Ok(OpenSource::Bytes(b.as_bytes().to_vec()));
        }
        if let Ok(bytes) = data.call_method0("__bytes__") {
            if let Ok(b) = bytes.downcast::<PyBytes>() {
                return Ok(OpenSource::Bytes(b.as_bytes().to_vec()));
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
            return Ok(OpenSource::Bytes(b.as_bytes().to_vec()));
        }
    }
    Err(PyTypeError::new_err(format!(
        "IVT source must be a path, bytes-like, or binary file object; got {}",
        source.get_type().name()?
    )))
}

impl Ivt {
    fn ensure_data(&self, _py: Python<'_>) -> Result<Arc<Vec<u8>>, IvtError> {
        {
            let guard = self.data.lock().unwrap();
            if let Some(d) = guard.as_ref() {
                return Ok(Arc::clone(d));
            }
        }
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| IvtError::Parse("IVT has no in-memory data or path".into()))?;
        let bytes = std::fs::read(path).map_err(IvtError::Io)?;
        let arc = Arc::new(bytes);
        *self.data.lock().unwrap() = Some(Arc::clone(&arc));
        Ok(arc)
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
/// An `Ivt` reader exposing `metadata`, `layout`, and `to_arrow()`. Summary
/// metadata and layout are parsed immediately; cell decoding waits until
/// `to_arrow()`.
///
/// ### Raises
///
/// - `TypeError`: unsupported source, path representation or `read()` result.
/// - `OSError`: the source path cannot be opened or read.
/// - `ValueError`: invalid or unsupported IVT header, descriptor or layout.
///
/// ### Notes
///
/// Paths use selective metadata reads. Keep the file available and unchanged:
/// the first to_arrow() reads its full contents, and lazy metadata properties
/// may read additional ranges. In-memory sources are copied when opened, so
/// later changes to a bytearray do not affect the reader. This function does
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
/// in_memory = pyivt.open(BytesIO(Path("table.ivt").read_bytes()))
/// ```
#[pyfunction]
#[pyo3(name = "open")]
pub fn open_ivt(py: Python<'_>, source: Bound<'_, PyAny>) -> PyResult<Ivt> {
    let src = resolve_source(py, source)?;
    match src {
        OpenSource::Path(path) => {
            let path_clone = path.clone();
            let (meta, layout) = py
                .allow_threads(|| -> Result<(CoreMeta, Layout), IvtError> {
                    let meta = metadata::open_metadata_path(&path_clone)?;
                    let layout = build_layout(&meta.desc().dims).map_err(IvtError::Parse)?;
                    Ok((meta, layout))
                })
                .map_err(map_ivt_err)?;
            let py_layout = PyLayout::from(&layout);
            let metadata = Py::new(
                py,
                IvtMetadata {
                    inner: Mutex::new(meta),
                },
            )?;
            Ok(Ivt {
                metadata,
                layout,
                py_layout,
                data: Mutex::new(None),
                path: Some(path),
                cache: Mutex::new(None),
            })
        }
        OpenSource::Bytes(data) => {
            let (meta, layout) = py
                .allow_threads(|| -> Result<(CoreMeta, Layout), IvtError> {
                    let hdr = read_header(&data).map_err(IvtError::Parse)?;
                    let desc = parse_descriptor(&data).ok_or(IvtError::DescriptorNotFound)?;
                    let layout = build_layout(&desc.dims).map_err(IvtError::Parse)?;
                    let meta = metadata::build_metadata(data.clone(), Some(desc), Some(hdr))
                        .map_err(IvtError::Parse)?;
                    Ok((meta, layout))
                })
                .map_err(map_ivt_err)?;
            let py_layout = PyLayout::from(&layout);
            let metadata = Py::new(
                py,
                IvtMetadata {
                    inner: Mutex::new(meta),
                },
            )?;
            Ok(Ivt {
                metadata,
                layout,
                py_layout,
                data: Mutex::new(Some(Arc::new(data))),
                path: None,
                cache: Mutex::new(None),
            })
        }
    }
}
