//! Zero-copy Arrow C stream export. The capsule owns the reader until consumed.
use arrow::ffi_stream::FFI_ArrowArrayStream;
use arrow::record_batch::RecordBatchReader;
use pyo3::prelude::*;
use pyo3::types::PyCapsule;
use std::ffi::CString;

pub fn stream_capsule<'py>(
    py: Python<'py>,
    reader: Box<dyn RecordBatchReader + Send>,
) -> PyResult<Bound<'py, PyCapsule>> {
    PyCapsule::new(
        py,
        FFI_ArrowArrayStream::new(reader),
        Some(CString::new("arrow_array_stream").unwrap()),
    )
}
