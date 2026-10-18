//! PyO3 wrappers for the public Python API.

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes, PyDict, PyString};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::arrow_export::columns_to_pyarrow;
use crate::decode::{build_layout, complete_grid, decode_store, Layout};
use crate::descriptor::parse_descriptor;
use crate::header::read_header;
use crate::metadata::{
    self, Dimension as CoreDimension, Footnote as CoreFootnote, Geographies,
    IvtMetadata as CoreMeta, LocalizedText as CoreLocalized, MissingValue as CoreMissing,
};
use crate::tidy::{tidy_arrays, Column};

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

fn geographies_to_dict(py: Python<'_>, g: &Geographies) -> PyResult<Py<PyDict>> {
    let d = PyDict::new(py);
    d.set_item("member_id", g.member_id.clone())?;
    if let Some(v) = &g.geo_label {
        d.set_item("geo_label", v.clone())?;
    }
    if let Some(v) = &g.geo_name {
        d.set_item("geo_name", v.clone())?;
    }
    if let Some(v) = &g.geo_uid {
        d.set_item("geo_uid", v.clone())?;
    }
    if let Some(v) = &g.geo_label_fr {
        d.set_item("geo_label_fr", v.clone())?;
    }
    if let Some(v) = &g.dqf_code {
        d.set_item("dqf_code", v.clone())?;
    }
    if let Some(v) = &g.tnr_short_form {
        d.set_item("tnr_short_form", v.clone())?;
    }
    Ok(d.into())
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
    fn time_series(&self) -> Option<String> {
        self.inner.lock().unwrap().time_series.clone()
    }

    #[getter]
    fn read_only(&self) -> Option<bool> {
        self.inner.lock().unwrap().read_only
    }

    #[getter]
    fn dimensions(&self) -> Vec<Dimension> {
        self.inner
            .lock()
            .unwrap()
            .dimensions()
            .iter()
            .cloned()
            .map(Dimension::from)
            .collect()
    }

    #[getter]
    fn geographies(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let g = self.inner.lock().unwrap().geographies().clone();
        geographies_to_dict(py, &g)
    }

    #[getter]
    fn footnotes(&self) -> Vec<Footnote> {
        self.inner
            .lock()
            .unwrap()
            .footnotes()
            .iter()
            .cloned()
            .map(Footnote::from)
            .collect()
    }

    #[getter]
    fn missing_values(&self) -> Vec<MissingValue> {
        self.inner
            .lock()
            .unwrap()
            .missing_values()
            .iter()
            .cloned()
            .map(MissingValue::from)
            .collect()
    }

    fn _loaded(&self, name: &str) -> bool {
        self.inner.lock().unwrap().loaded(name)
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
    data: Arc<Vec<u8>>,
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

        let data = Arc::clone(&self.data);
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

fn read_source(_py: Python<'_>, source: Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    if let Ok(b) = source.downcast::<PyBytes>() {
        return Ok(b.as_bytes().to_vec());
    }
    if let Ok(ba) = source.downcast::<PyByteArray>() {
        return Ok(ba.to_vec());
    }
    if source.hasattr("read")? {
        let data = source.call_method0("read")?;
        if let Ok(b) = data.downcast::<PyBytes>() {
            return Ok(b.as_bytes().to_vec());
        }
        // bytearray / memoryview
        if let Ok(bytes) = data.call_method0("__bytes__") {
            if let Ok(b) = bytes.downcast::<PyBytes>() {
                return Ok(b.as_bytes().to_vec());
            }
        }
        return Err(PyTypeError::new_err(format!(
            "file-like source must return bytes; got {}",
            data.get_type().name()?
        )));
    }
    if let Ok(s) = source.downcast::<PyString>() {
        let path = PathBuf::from(s.to_str()?);
        return std::fs::read(&path).map_err(|e| {
            PyValueError::new_err(format!("failed to read {}: {e}", path.display()))
        });
    }
    // pathlib.Path
    if source.hasattr("__fspath__")? {
        let p = source.call_method0("__fspath__")?;
        let path = PathBuf::from(p.extract::<&str>()?);
        return std::fs::read(&path).map_err(|e| {
            PyValueError::new_err(format!("failed to read {}: {e}", path.display()))
        });
    }
    // bytearray / memoryview
    if let Ok(bytes) = source.call_method0("__bytes__") {
        if let Ok(b) = bytes.downcast::<PyBytes>() {
            return Ok(b.as_bytes().to_vec());
        }
    }
    Err(PyTypeError::new_err(format!(
        "IVT source must be a path, bytes-like, or binary file object; got {}",
        source.get_type().name()?
    )))
}

#[pyfunction]
#[pyo3(name = "open")]
pub fn open_ivt(py: Python<'_>, source: Bound<'_, PyAny>) -> PyResult<Ivt> {
    let data = read_source(py, source)?;
    let hdr = read_header(&data).map_err(PyValueError::new_err)?;
    let desc = parse_descriptor(&data)
        .ok_or_else(|| PyValueError::new_err("could not parse IVT dimension descriptor"))?;
    let layout = build_layout(&desc.dims).map_err(PyValueError::new_err)?;
    let py_layout = PyLayout::from(&layout);
    let meta = metadata::build_metadata(data.clone(), Some(desc), Some(hdr))
        .map_err(PyValueError::new_err)?;
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
        data: Arc::new(data),
        cache: Mutex::new(None),
    })
}
