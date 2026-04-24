use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{Connection, GlueState};

fn now_str() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

// ---------------------------------------------------------------------------
// CreateConnection
// ---------------------------------------------------------------------------

pub fn create_connection(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let conn_input = &input["ConnectionInput"];
    let name = conn_input["Name"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidInputException", "ConnectionInput.Name is required")
    })?;
    let connection_type = conn_input["ConnectionType"]
        .as_str()
        .unwrap_or("JDBC")
        .to_string();

    if state.connections.contains_key(name) {
        return Err(AwsError::conflict(
            "AlreadyExistsException",
            format!("Connection already exists: {name}"),
        ));
    }

    let connection_properties: HashMap<String, String> = conn_input["ConnectionProperties"]
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let description = conn_input["Description"].as_str().map(|s| s.to_string());

    let conn = Connection {
        name: name.to_string(),
        connection_type,
        connection_properties,
        description,
        created_at: now_str(),
    };

    info!(name = %name, "Created Glue connection");
    state.connections.insert(name.to_string(), conn);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// GetConnections
// ---------------------------------------------------------------------------

pub fn get_connections(
    state: &GlueState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list: Vec<Value> = state
        .connections
        .iter()
        .map(|e| connection_to_value(e.value()))
        .collect();

    Ok(json!({ "ConnectionList": list }))
}

// ---------------------------------------------------------------------------
// DeleteConnection
// ---------------------------------------------------------------------------

pub fn delete_connection(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ConnectionName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidInputException", "ConnectionName is required")
    })?;

    state.connections.remove(name).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Connection not found: {name}"),
        )
    })?;

    info!(name = %name, "Deleted Glue connection");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn connection_to_value(c: &Connection) -> Value {
    let props: serde_json::Map<String, Value> = c
        .connection_properties
        .iter()
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect();

    json!({
        "Name": c.name,
        "ConnectionType": c.connection_type,
        "ConnectionProperties": props,
        "Description": c.description,
        "CreationTime": c.created_at,
    })
}
