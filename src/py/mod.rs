mod types;

use pyo3::prelude::*;
use types::{
    open_ivt, Dimension, Footnote, Geographies, Ivt, IvtMetadata, LocalizedLabels, LocalizedText,
    MissingValue, PyLayout,
};

pub fn init_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<LocalizedText>()?;
    m.add_class::<LocalizedLabels>()?;
    m.add_class::<Dimension>()?;
    m.add_class::<Footnote>()?;
    m.add_class::<MissingValue>()?;
    m.add_class::<Geographies>()?;
    m.add_class::<IvtMetadata>()?;
    m.add_class::<PyLayout>()?;
    m.add_class::<Ivt>()?;
    m.add_function(wrap_pyfunction!(open_ivt, m)?)?;
    Ok(())
}
