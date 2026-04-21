use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{GlueDatabase, GlueState};

fn now_str() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

// ---------------------------------------------------------------------------
// CreateDatabase
// ---------------------------------------------------------------------------

pub fn create_database(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DatabaseInput"]["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseInput.Name is required"))?;

    if state.databases.contains_key(name) {
        return Err(AwsError::conflict(
            "AlreadyExistsException",
            format!("Database already exists: {name}"),
        ));
    }

    let description = input["DatabaseInput"]["Description"]
        .as_str()
        .map(|s| s.to_string());

    let db = GlueDatabase {
        name: name.to_string(),
        description,
        created_at: now_str(),
    };

    info!(name = %name, "Created Glue database");
    state.databases.insert(name.to_string(), db);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// GetDatabase
// ---------------------------------------------------------------------------

pub fn get_database(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    let db = state.databases.get(name).ok_or_else(|| {
        AwsError::not_found("EntityNotFoundException", format!("Database not found: {name}"))
    })?;

    Ok(json!({
        "Database": {
            "Name": db.name,
            "Description": db.description,
            "CreateTime": db.created_at,
        }
    }))
}

// ---------------------------------------------------------------------------
// GetDatabases
// ---------------------------------------------------------------------------

pub fn get_databases(
    state: &GlueState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list: Vec<Value> = state
        .databases
        .iter()
        .map(|e| {
            json!({
                "Name": e.value().name,
                "Description": e.value().description,
                "CreateTime": e.value().created_at,
            })
        })
        .collect();

    Ok(json!({ "DatabaseList": list }))
}

// ---------------------------------------------------------------------------
// DeleteDatabase
// ---------------------------------------------------------------------------

pub fn delete_database(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    state.databases.remove(name).ok_or_else(|| {
        AwsError::not_found("EntityNotFoundException", format!("Database not found: {name}"))
    })?;

    info!(name = %name, "Deleted Glue database");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UpdateDatabase
// ---------------------------------------------------------------------------

pub fn update_database(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    let mut db = state.databases.get_mut(name).ok_or_else(|| {
        AwsError::not_found("EntityNotFoundException", format!("Database not found: {name}"))
    })?;

    if let Some(desc) = input["DatabaseInput"]["Description"].as_str() {
        db.description = Some(desc.to_string());
    }

    Ok(json!({}))
}
