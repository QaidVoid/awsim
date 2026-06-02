use awsim_core::tags::{TagOpts, validate_aws_tags};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{AthenaState, DataCatalog};

fn catalog_to_value(c: &DataCatalog) -> Value {
    json!({
        "CatalogName": c.name,
        "Type": c.catalog_type,
        "Description": c.description,
        "Parameters": c.parameters,
    })
}

// ---------------------------------------------------------------------------
// ListDataCatalogs
// ---------------------------------------------------------------------------

pub fn list_data_catalogs(
    state: &AthenaState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    state.ensure_default_catalog();

    let catalogs: Vec<Value> = state
        .data_catalogs
        .iter()
        .map(|e| {
            json!({
                "CatalogName": e.value().name,
                "Type": e.value().catalog_type,
            })
        })
        .collect();

    Ok(json!({ "DataCatalogsSummary": catalogs }))
}

// ---------------------------------------------------------------------------
// GetDataCatalog
// ---------------------------------------------------------------------------

pub fn get_data_catalog(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    state.ensure_default_catalog();

    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "Name is required"))?;

    let catalog = state.data_catalogs.get(name).ok_or_else(|| {
        AwsError::not_found(
            "InvalidRequestException",
            format!("DataCatalog not found: {name}"),
        )
    })?;

    Ok(json!({ "DataCatalog": catalog_to_value(&catalog) }))
}

// ---------------------------------------------------------------------------
// CreateDataCatalog
// ---------------------------------------------------------------------------

pub fn create_data_catalog(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "Name is required"))?;
    let catalog_type = input["Type"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "Type is required"))?;

    if state.data_catalogs.contains_key(name) {
        return Err(AwsError::conflict(
            "InvalidRequestException",
            format!("DataCatalog already exists: {name}"),
        ));
    }

    validate_aws_tags(&input["Tags"], &TagOpts::aws_default())?;

    let description = input["Description"].as_str().map(|s| s.to_string());
    let parameters = input.get("Parameters").cloned().unwrap_or(json!({}));

    let catalog = DataCatalog {
        name: name.to_string(),
        catalog_type: catalog_type.to_string(),
        description,
        parameters,
    };

    info!(name = %name, "Created Athena data catalog");
    state.data_catalogs.insert(name.to_string(), catalog);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DeleteDataCatalog
// ---------------------------------------------------------------------------

pub fn delete_data_catalog(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "Name is required"))?;

    state.data_catalogs.remove(name).ok_or_else(|| {
        AwsError::not_found(
            "InvalidRequestException",
            format!("DataCatalog not found: {name}"),
        )
    })?;

    info!(name = %name, "Deleted Athena data catalog");
    Ok(json!({}))
}
