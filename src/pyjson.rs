//! Conversions between Python objects and JSON values (via the json module)

use pyo3::prelude::*;
use serde_json::Value as JsonValue;

/// Convert a JSON-compatible Python object to a JSON value
pub fn py_to_json(value: &Bound<'_, PyAny>) -> PyResult<JsonValue> {
    let py = value.py();
    let text: String = py
        .import("json")?
        .call_method1("dumps", (value,))?
        .extract()?;
    serde_json::from_str(&text)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid value: {}", e)))
}

/// Convert a JSON value to a Python object
pub fn json_to_py(py: Python<'_>, value: &JsonValue) -> PyResult<Py<PyAny>> {
    Ok(py
        .import("json")?
        .call_method1("loads", (value.to_string(),))?
        .unbind())
}
