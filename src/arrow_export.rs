//! Build Arrow arrays / tables and hand off to pyarrow via C Data Interface.

use arrow::array::{
    ArrayRef, Float64Array, Int64Array, StringArray,
};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::ffi::to_ffi;
use arrow::record_batch::RecordBatch;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyList;
use std::sync::Arc;

use crate::tidy::Column;

pub fn columns_to_record_batch(cols: &[(String, Column)]) -> Result<RecordBatch, String> {
    if cols.is_empty() {
        return Err("no columns".into());
    }
    let mut fields = Vec::with_capacity(cols.len());
    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(cols.len());
    for (name, col) in cols {
        match col {
            Column::I64(v) => {
                fields.push(Field::new(name, DataType::Int64, true));
                arrays.push(Arc::new(Int64Array::from(v.clone())) as ArrayRef);
            }
            Column::F64(v) => {
                fields.push(Field::new(name, DataType::Float64, true));
                arrays.push(Arc::new(Float64Array::from(v.clone())) as ArrayRef);
            }
            Column::Utf8(v) => {
                fields.push(Field::new(name, DataType::Utf8, true));
                let opts: Vec<Option<&str>> = v.iter().map(|s| s.as_deref()).collect();
                arrays.push(Arc::new(StringArray::from(opts)) as ArrayRef);
            }
        }
    }
    let schema = Arc::new(Schema::new(fields));
    RecordBatch::try_new(schema, arrays).map_err(|e| e.to_string())
}

/// Convert a RecordBatch into a pyarrow.Table via Arrow C Data Interface.
pub fn record_batch_to_pyarrow<'py>(
    py: Python<'py>,
    batch: &RecordBatch,
) -> PyResult<Bound<'py, PyAny>> {
    let pa = py.import("pyarrow")?;
    let mut arrays = Vec::with_capacity(batch.num_columns());
    let mut names = Vec::with_capacity(batch.num_columns());

    for (i, field) in batch.schema().fields().iter().enumerate() {
        names.push(field.name().clone());
        let array = batch.column(i);
        let (ffi_array, ffi_schema) =
            to_ffi(&array.to_data()).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        // Leak boxed FFI structs; pyarrow takes ownership via _import_from_c
        let array_ptr = Box::into_raw(Box::new(ffi_array)) as usize;
        let schema_ptr = Box::into_raw(Box::new(ffi_schema)) as usize;

        let py_array = pa
            .getattr("Array")?
            .call_method1("_import_from_c", (array_ptr, schema_ptr))?;
        arrays.push(py_array);
    }

    let names_list = PyList::new(py, &names)?;
    let arrays_list = PyList::new(py, &arrays)?;
    pa.call_method1("table", (arrays_list, names_list))
}

pub fn columns_to_pyarrow<'py>(
    py: Python<'py>,
    cols: &[(String, Column)],
) -> PyResult<Bound<'py, PyAny>> {
    let batch = columns_to_record_batch(cols)
        .map_err(PyValueError::new_err)?;
    record_batch_to_pyarrow(py, &batch)
}
