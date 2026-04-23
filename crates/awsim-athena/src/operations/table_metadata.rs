use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::AthenaState;

// ---------------------------------------------------------------------------
// GetTableMetadata
// ---------------------------------------------------------------------------

pub fn get_table_metadata(
    _state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let catalog_name = input["CatalogName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "CatalogName is required"))?;
    let database_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "DatabaseName is required"))?;
    let table_name = input["TableName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "TableName is required"))?;

    Ok(json!({
        "TableMetadata": {
            "Name": table_name,
            "TableType": "EXTERNAL_TABLE",
            "Columns": [],
            "PartitionKeys": [],
            "Parameters": {
                "EXTERNAL": "TRUE",
                "CatalogName": catalog_name,
                "DatabaseName": database_name,
            },
        }
    }))
}

// ---------------------------------------------------------------------------
// ListTableMetadata
// ---------------------------------------------------------------------------

pub fn list_table_metadata(
    _state: &AthenaState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({
        "TableMetadataList": [],
        "NextToken": null,
    }))
}
