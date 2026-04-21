use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{GlueState, GlueTable};

fn now_str() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn table_key(db: &str, table: &str) -> String {
    format!("{db}.{table}")
}

// ---------------------------------------------------------------------------
// CreateTable
// ---------------------------------------------------------------------------

pub fn create_table(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;
    let table_name = input["TableInput"]["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "TableInput.Name is required"))?;

    let key = table_key(db_name, table_name);
    if state.tables.contains_key(&key) {
        return Err(AwsError::conflict(
            "AlreadyExistsException",
            format!("Table already exists: {db_name}.{table_name}"),
        ));
    }

    let now = now_str();
    let storage_descriptor = input["TableInput"]["StorageDescriptor"].clone();

    let table = GlueTable {
        database_name: db_name.to_string(),
        name: table_name.to_string(),
        storage_descriptor: if storage_descriptor.is_null() { None } else { Some(storage_descriptor) },
        description: input["TableInput"]["Description"].as_str().map(|s| s.to_string()),
        created_at: now.clone(),
        updated_at: now,
    };

    info!(db = %db_name, table = %table_name, "Created Glue table");
    state.tables.insert(key, table);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// GetTable
// ---------------------------------------------------------------------------

pub fn get_table(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;
    let table_name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    let key = table_key(db_name, table_name);
    let table = state.tables.get(&key).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Table not found: {db_name}.{table_name}"),
        )
    })?;

    Ok(json!({ "Table": table_to_value(&table) }))
}

// ---------------------------------------------------------------------------
// GetTables
// ---------------------------------------------------------------------------

pub fn get_tables(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;

    let list: Vec<Value> = state
        .tables
        .iter()
        .filter(|e| e.value().database_name == db_name)
        .map(|e| table_to_value(e.value()))
        .collect();

    Ok(json!({ "TableList": list }))
}

// ---------------------------------------------------------------------------
// DeleteTable
// ---------------------------------------------------------------------------

pub fn delete_table(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;
    let table_name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    let key = table_key(db_name, table_name);
    state.tables.remove(&key).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Table not found: {db_name}.{table_name}"),
        )
    })?;

    info!(db = %db_name, table = %table_name, "Deleted Glue table");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UpdateTable
// ---------------------------------------------------------------------------

pub fn update_table(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;
    let table_name = input["TableInput"]["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "TableInput.Name is required"))?;

    let key = table_key(db_name, table_name);
    let mut table = state.tables.get_mut(&key).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Table not found: {db_name}.{table_name}"),
        )
    })?;

    if let Some(sd) = input["TableInput"].get("StorageDescriptor") {
        if !sd.is_null() {
            table.storage_descriptor = Some(sd.clone());
        }
    }
    if let Some(desc) = input["TableInput"]["Description"].as_str() {
        table.description = Some(desc.to_string());
    }
    table.updated_at = now_str();

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn table_to_value(t: &GlueTable) -> Value {
    json!({
        "DatabaseName": t.database_name,
        "Name": t.name,
        "StorageDescriptor": t.storage_descriptor,
        "Description": t.description,
        "CreateTime": t.created_at,
        "UpdateTime": t.updated_at,
    })
}
