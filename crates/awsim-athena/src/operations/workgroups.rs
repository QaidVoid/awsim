use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{AthenaState, WorkGroup};

fn now_str() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

// ---------------------------------------------------------------------------
// CreateWorkGroup
// ---------------------------------------------------------------------------

pub fn create_workgroup(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "Name is required"))?;

    if state.workgroups.contains_key(name) {
        return Err(AwsError::conflict(
            "InvalidRequestException",
            format!("WorkGroup already exists: {name}"),
        ));
    }

    let description = input["Description"].as_str().map(|s| s.to_string());
    let output_location = input["Configuration"]["ResultConfiguration"]["OutputLocation"]
        .as_str()
        .map(|s| s.to_string());

    let wg = WorkGroup {
        name: name.to_string(),
        description,
        state: "ENABLED".to_string(),
        output_location,
        created_at: now_str(),
    };

    info!(name = %name, "Created Athena workgroup");
    state.workgroups.insert(name.to_string(), wg);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DeleteWorkGroup
// ---------------------------------------------------------------------------

pub fn delete_workgroup(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["WorkGroup"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "WorkGroup is required"))?;

    state.workgroups.remove(name).ok_or_else(|| {
        AwsError::not_found(
            "InvalidRequestException",
            format!("WorkGroup not found: {name}"),
        )
    })?;

    info!(name = %name, "Deleted Athena workgroup");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// GetWorkGroup
// ---------------------------------------------------------------------------

pub fn get_workgroup(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["WorkGroup"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "WorkGroup is required"))?;

    let wg = state.workgroups.get(name).ok_or_else(|| {
        AwsError::not_found(
            "InvalidRequestException",
            format!("WorkGroup not found: {name}"),
        )
    })?;

    Ok(json!({
        "WorkGroup": workgroup_to_value(&wg)
    }))
}

// ---------------------------------------------------------------------------
// ListWorkGroups
// ---------------------------------------------------------------------------

pub fn list_workgroups(
    state: &AthenaState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let workgroups: Vec<Value> = state
        .workgroups
        .iter()
        .map(|e| {
            json!({
                "Name": e.value().name,
                "State": e.value().state,
                "Description": e.value().description,
                "CreationTime": e.value().created_at,
            })
        })
        .collect();

    Ok(json!({ "WorkGroups": workgroups }))
}

// ---------------------------------------------------------------------------
// UpdateWorkGroup
// ---------------------------------------------------------------------------

pub fn update_workgroup(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["WorkGroup"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "WorkGroup is required"))?;

    let mut wg = state.workgroups.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "InvalidRequestException",
            format!("WorkGroup not found: {name}"),
        )
    })?;

    if let Some(desc) = input["Description"].as_str() {
        wg.description = Some(desc.to_string());
    }

    if let Some(output) =
        input["ConfigurationUpdates"]["ResultConfigurationUpdates"]["OutputLocation"].as_str()
    {
        wg.output_location = Some(output.to_string());
    }

    if let Some(state_val) = input["State"].as_str() {
        wg.state = state_val.to_string();
    }

    info!(name = %name, "Updated Athena workgroup");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn workgroup_to_value(wg: &WorkGroup) -> Value {
    json!({
        "Name": wg.name,
        "State": wg.state,
        "Description": wg.description,
        "CreationTime": wg.created_at,
        "Configuration": {
            "ResultConfiguration": {
                "OutputLocation": wg.output_location
            }
        }
    })
}
