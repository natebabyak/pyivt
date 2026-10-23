//! Build Arrow arrays / tables and hand off to pyarrow via C Data Interface.

use arrow::array::{
    Array, ArrayRef, Date32Array, Float64Array, Int64Array, ListArray, StringArray,
};
use arrow::datatypes::{DataType, Field, Schema, UInt16Type};
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
            Column::NullableI64(v) => {
                fields.push(Field::new(name, DataType::Int64, true));
                arrays.push(Arc::new(Int64Array::from(v.clone())) as ArrayRef);
            }
            Column::F64(v) => {
                fields.push(Field::new(name, DataType::Float64, true));
                arrays.push(Arc::new(Float64Array::from(v.clone())) as ArrayRef);
            }
            Column::Date32(v) => {
                fields.push(Field::new(name, DataType::Date32, true));
                arrays.push(Arc::new(Date32Array::from(v.clone())) as ArrayRef);
            }
            Column::U16List(v) => {
                let array = ListArray::from_iter_primitive::<UInt16Type, _, _>(
                    v.iter().map(|row| Some(row.iter().copied().map(Some))),
                );
                fields.push(Field::new(name, array.data_type().clone(), false));
                arrays.push(Arc::new(array) as ArrayRef);
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

        // PyArrow moves the contents and clears their release callbacks. Keep
        // ownership of the outer structs so successful and failed imports free them.
        let mut ffi_array = Box::new(ffi_array);
        let mut ffi_schema = Box::new(ffi_schema);
        let array_ptr = (&mut *ffi_array) as *mut _ as usize;
        let schema_ptr = (&mut *ffi_schema) as *mut _ as usize;

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
    let batch = columns_to_record_batch(cols).map_err(PyValueError::new_err)?;
    record_batch_to_pyarrow(py, &batch)
}
