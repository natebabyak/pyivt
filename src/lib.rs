mod arrow_export;
mod bytes;
mod decode;
mod descriptor;
mod error;
mod header;
mod io;
mod metadata;
mod metadata_directory;
mod members;
mod py;
mod tidy;

use pyo3::prelude::*;

/// Python extension module `pyivt._native`.
#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    py::init_module(m)
}
