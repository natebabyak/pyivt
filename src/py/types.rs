//! PyO3 wrappers for the public Python API.

use pyo3::exceptions::{PyOSError, PyTypeError, PyValueError};
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
    self, Dimension as CoreDimension, Footnote as CoreFootnote,
    Geographies as CoreGeographies, IvtMetadata as CoreMeta, LocalizedText as CoreLocalized,
    MissingValue as CoreMissing, TimeSeriesInfo as CoreTimeSeries,
};
use crate::tidy::{tidy_arrays, Column};

fn map_ivt_err(e: IvtError) -> PyErr {
    match e {
        IvtError::Io(io) => PyOSError::new_err(io.to_string()),
        other => PyValueError::new_err(other.to_string()),
    }
}

#[pyclass(name = "LocalizedText", module = "pyivt")]
#[derive(Clone)]
pub struct LocalizedText {
    #[pyo3(get)]
    pub en: Option<String>,
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

    fn __repr__(&self) -> String {
        format!("LocalizedText(en={:?}, fr={:?})", self.en, self.fr)
    }
}

#[pyclass(name = "TimeSeries", module = "pyivt")]
#[derive(Clone)]
pub struct TimeSeries {
    #[pyo3(get)]
    pub frequency: Option<String>,
    #[pyo3(get)]
    pub start: Option<String>,
    #[pyo3(get)]
    pub end: Option<String>,
}

impl From<CoreTimeSeries> for TimeSeries {
    fn from(v: CoreTimeSeries) -> Self {
        Self {
            frequency: v.frequency,
            start: v.start,
            end: v.end,
        }
    }
}

#[pymethods]
impl TimeSeries {
    #[new]
    #[pyo3(signature = (frequency=None, start=None, end=None))]
    fn new(frequency: Option<String>, start: Option<String>, end: Option<String>) -> Self {
        Self {
            frequency,
            start,
            end,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "TimeSeries(frequency={:?}, start={:?}, end={:?})",
            self.frequency, self.start, self.end
        )
    }
}

#[pyclass(name = "Dimension", module = "pyivt")]
#[derive(Clone)]
pub struct Dimension {
    #[pyo3(get)]
    pub name: String,
    #[pyo3(get)]
    pub name_fr: Option<String>,
    #[pyo3(get)]
    pub count: i64,
    #[pyo3(get, name = "type")]
    pub type_code: i64,
    #[pyo3(get)]
    pub is_geography: bool,
    #[pyo3(get)]
    pub description: Option<String>,
    #[pyo3(get)]
    pub description_fr: Option<String>,
    #[pyo3(get)]
    pub type_label: String,
    #[pyo3(get)]
    pub members: Option<Vec<String>>,
    #[pyo3(get)]
    pub members_fr: Option<Vec<String>>,
    #[pyo3(get)]
    pub ordinal: Option<Vec<i64>>,
}

impl From<CoreDimension> for Dimension {
    fn from(d: CoreDimension) -> Self {
        Self {
            name: d.name,
            name_fr: d.name_fr,
            count: d.count,
            type_code: d.type_code,
            is_geography: d.is_geography,
            description: d.description,
            description_fr: d.description_fr,
            type_label: d.type_label,
            members: d.members,
            members_fr: d.members_fr,
            ordinal: d.ordinal,
        }
    }
}

#[pymethods]
impl Dimension {
    fn __repr__(&self) -> String {
        format!(
            "Dimension(name={:?}, count={}, is_geography={})",
            self.name, self.count, self.is_geography
        )
    }
}

#[pyclass(name = "Footnote", module = "pyivt")]
#[derive(Clone)]
pub struct Footnote {
    #[pyo3(get)]
    pub language: String,
    #[pyo3(get)]
    pub number: i64,
    #[pyo3(get)]
    pub text: String,
    #[pyo3(get)]
    pub scope: Option<String>,
    #[pyo3(get)]
    pub dimension: Option<String>,
    #[pyo3(get)]
    pub member_id: Option<i64>,
}

impl From<CoreFootnote> for Footnote {
    fn from(f: CoreFootnote) -> Self {
        Self {
            language: f.language,
            number: f.number,
            text: f.text,
            scope: f.scope,
            dimension: f.dimension,
            member_id: f.member_id,
        }
    }
}

#[pyclass(name = "MissingValue", module = "pyivt")]
#[derive(Clone)]
pub struct MissingValue {
    #[pyo3(get)]
    pub symbol: String,
    #[pyo3(get)]
    pub label: LocalizedText,
    #[pyo3(get)]
    pub code: Option<i64>,
}

impl From<CoreMissing> for MissingValue {
    fn from(m: CoreMissing) -> Self {
        Self {
            symbol: m.symbol,
            label: m.label.into(),
            code: m.code,
        }
    }
}

#[pyclass(name = "Geographies", module = "pyivt")]
#[derive(Clone)]
pub struct Geographies {
    #[pyo3(get)]
    pub member_id: Vec<i64>,
    #[pyo3(get)]
    pub geo_label: Option<Vec<String>>,
    #[pyo3(get)]
    pub geo_name: Option<Vec<Option<String>>>,
    #[pyo3(get)]
    pub geo_uid: Option<Vec<Option<String>>>,
    #[pyo3(get)]
    pub geo_label_fr: Option<Vec<String>>,
    #[pyo3(get)]
    pub dqf_code: Option<Vec<Option<String>>>,
    #[pyo3(get)]
    pub tnr_short_form: Option<Vec<Option<String>>>,
}

impl From<CoreGeographies> for Geographies {
    fn from(g: CoreGeographies) -> Self {
        Self {
            member_id: g.member_id,
            geo_label: g.geo_label,
            geo_name: g.geo_name,
            geo_uid: g.geo_uid,
            geo_label_fr: g.geo_label_fr,
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
            self.geo_label.is_some()
        )
    }
}

#[pyclass(name = "IvtMetadata", module = "pyivt")]
pub struct IvtMetadata {
    inner: Mutex<CoreMeta>,
}

#[pymethods]
impl IvtMetadata {
    #[getter]
    fn product_id(&self) -> Option<String> {
        self.inner.lock().unwrap().product_id.clone()
    }

    #[getter]
    fn title(&self) -> LocalizedText {
        self.inner.lock().unwrap().title.clone().into()
    }

    #[getter]
    fn universe(&self) -> Option<String> {
        self.inner.lock().unwrap().universe.clone()
    }

    #[getter]
    fn notes(&self) -> LocalizedText {
        self.inner.lock().unwrap().notes.clone().into()
    }

    #[getter]
    fn version(&self) -> String {
        self.inner.lock().unwrap().version.clone()
    }

    #[getter]
    fn signature(&self) -> String {
        self.inner.lock().unwrap().signature.clone()
    }

    #[getter]
    fn author(&self) -> LocalizedText {
        self.inner.lock().unwrap().author.clone().into()
    }

    #[getter]
    fn category(&self) -> LocalizedText {
        self.inner.lock().unwrap().category.clone().into()
    }

    #[getter]
    fn keywords(&self) -> LocalizedText {
        self.inner.lock().unwrap().keywords.clone().into()
    }

    #[getter]
    fn number_of_datapoints(&self) -> Option<i64> {
        self.inner.lock().unwrap().number_of_datapoints
    }

    #[getter]
    fn time_series(&self) -> Option<TimeSeries> {
        self.inner
            .lock()
            .unwrap()
            .time_series
            .clone()
            .map(TimeSeries::from)
    }

    #[getter]
    fn read_only(&self) -> Option<bool> {
        self.inner.lock().unwrap().read_only
    }

    #[getter]
    fn dimensions(&self) -> PyResult<Vec<Dimension>> {
        let mut guard = self.inner.lock().unwrap();
        let dims = guard.dimensions().map_err(PyValueError::new_err)?;
        Ok(dims.iter().cloned().map(Dimension::from).collect())
    }

    #[getter]
    fn geographies(&self) -> PyResult<Geographies> {
        let mut guard = self.inner.lock().unwrap();
        Ok(guard.geographies().map_err(PyValueError::new_err)?.clone().into())
    }

    #[getter]
    fn footnotes(&self) -> PyResult<Vec<Footnote>> {
        let mut guard = self.inner.lock().unwrap();
        let notes = guard.footnotes().map_err(PyValueError::new_err)?;
        Ok(notes.iter().cloned().map(Footnote::from).collect())
    }

    #[getter]
    fn missing_values(&self) -> PyResult<Vec<MissingValue>> {
        let mut guard = self.inner.lock().unwrap();
        let vals = guard.missing_values().map_err(PyValueError::new_err)?;
        Ok(vals.iter().cloned().map(MissingValue::from).collect())
    }

    fn _loaded(&self, name: &str) -> bool {
        self.inner.lock().unwrap().loaded(name)
    }

    /// Internal: bytes read / read ops during metadata I/O (for benchmarks).
    fn _io_metrics(&self) -> (u64, u64, usize, bool) {
        let m = self.inner.lock().unwrap().io_metrics();
        (m.bytes_read, m.read_ops, m.largest_range, m.fallback_used)
    }
}

#[pyclass(name = "Layout", module = "pyivt")]
#[derive(Clone)]
pub struct PyLayout {
    #[pyo3(get)]
    pub counts: Vec<i64>,
    #[pyo3(get)]
    pub slugs: Vec<String>,
    #[pyo3(get)]
    pub n_dim: usize,
    #[pyo3(get)]
    pub geo_dim: usize,
    #[pyo3(get)]
    pub straddle: usize,
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
    complete: bool,
    cols: Vec<(String, Column)>,
}

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
    #[getter]
    fn metadata(&self, py: Python<'_>) -> Py<IvtMetadata> {
        self.metadata.clone_ref(py)
    }

    #[getter]
    fn layout(&self) -> PyLayout {
        self.py_layout.clone()
    }

    #[pyo3(signature = (*, complete=true, max_cells=100_000_000, labels=true, language="en"))]
    fn to_arrow<'py>(
        &self,
        py: Python<'py>,
        complete: bool,
        max_cells: i64,
        labels: bool,
        language: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        {
            let cache = self.cache.lock().unwrap();
            if let Some(c) = cache.as_ref() {
                if c.complete == complete {
                    return columns_to_pyarrow(py, &c.cols);
                }
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
                let store = decode_store(&data, &dims, &layout)?;
                if complete {
                    let n = layout.counts.iter().fold(1i64, |a, &c| a.saturating_mul(c));
                    if n > max_cells {
                        return Err(format!(
                            "published grid has {n} cells; raise max_cells or use complete=False"
                        ));
                    }
                    Ok(complete_grid(&store, &layout))
                } else {
                    Ok(store)
                }
            })
            .map_err(PyValueError::new_err)?;

        let tidy = {
            let meta = self.metadata.borrow(py);
            let mut inner = meta.inner.lock().unwrap();
            tidy_arrays(&cells, &mut inner, &self.layout, labels, language)
                .map_err(PyValueError::new_err)?
        };

        *self.cache.lock().unwrap() = Some(DecodeCache {
            complete,
            cols: tidy.clone(),
        });

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
