use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

/// ListDatabases — returns a stub `default` database for any catalog.
pub fn list_databases(
    _state: &crate::state::AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let _catalog = input["CatalogName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "CatalogName is required"))?;

    Ok(json!({
        "DatabaseList": [
            {
                "Name": "default",
                "Description": "Default database"
            }
        ]
    }))
}

/// GetDatabase — returns a stub database object.
pub fn get_database(
    _state: &crate::state::AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let _catalog = input["CatalogName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "CatalogName is required"))?;
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "DatabaseName is required"))?;

    Ok(json!({
        "Database": {
            "Name": db_name,
            "Description": ""
        }
    }))
}
