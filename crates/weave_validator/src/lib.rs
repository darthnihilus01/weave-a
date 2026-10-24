use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

pub mod kotlin;
pub mod sql;

use kotlin::KotlinValidator;

#[pyfunction]
#[pyo3(signature = (content, expected_package))]
fn validate_kotlin_source(content: &str, expected_package: &str) -> PyResult<()> {
    let mut validator = KotlinValidator::new();
    validator
        .validate_file(content, expected_package)
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
#[pyo3(signature = (content))]
fn has_genesis_app_entry(content: &str) -> PyResult<bool> {
    let mut validator = KotlinValidator::new();
    Ok(validator.has_genesis_app_entry(content))
}

#[pyfunction]
#[pyo3(signature = (content))]
fn has_design_kit_import(content: &str) -> PyResult<bool> {
    let validator = KotlinValidator::new();
    Ok(validator.has_design_kit_import(content))
}

#[pyfunction]
#[pyo3(signature = (raw_sql, max_limit=100, default_limit=50))]
fn validate_sql(raw_sql: &str, max_limit: u64, default_limit: u64) -> PyResult<String> {
    sql::validate_and_normalize_sql(raw_sql, max_limit, default_limit)
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pymodule]
mod weave_validator {
    use super::*;

    #[pymodule_export]
    use super::validate_kotlin_source;

    #[pymodule_export]
    use super::has_genesis_app_entry;

    #[pymodule_export]
    use super::has_design_kit_import;

    #[pymodule_export]
    use super::validate_sql;
}
