use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{Command, SsmState};

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn new_command_id() -> String {
    Uuid::new_v4().to_string()
}

// ---------------------------------------------------------------------------
// PutInventory (stub)
// ---------------------------------------------------------------------------

pub fn put_inventory(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // Accept any inventory data and return success
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// GetInventory (stub)
// ---------------------------------------------------------------------------

pub fn get_inventory(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "Entities": [] }))
}

// ---------------------------------------------------------------------------
// GetInventorySchema (stub)
// ---------------------------------------------------------------------------

pub fn get_inventory_schema(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "Schemas": [] }))
}

// ---------------------------------------------------------------------------
// SendCommand
// ---------------------------------------------------------------------------

pub fn send_command(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let document_name = input["DocumentName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidDocument", "DocumentName is required"))?;

    let targets = input["Targets"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let command_id = new_command_id();
    let now = now_epoch_secs();

    let command = Command {
        command_id: command_id.clone(),
        document_name: document_name.to_string(),
        targets: targets.clone(),
        status: "Pending".to_string(),
        created_time: now,
    };

    info!(command_id = %command_id, document_name, "SendCommand (stub)");
    state.commands.insert(command_id.clone(), command);

    Ok(json!({
        "Command": {
            "CommandId": command_id,
            "DocumentName": document_name,
            "Targets": targets,
            "Status": "Pending",
            "CreatedDate": now,
        }
    }))
}

// ---------------------------------------------------------------------------
// ListCommands
// ---------------------------------------------------------------------------

pub fn list_commands(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_command_id = input["CommandId"].as_str();
    let max_results = input["MaxResults"].as_u64().unwrap_or(50) as usize;

    let commands: Vec<Value> = state
        .commands
        .iter()
        .filter(|e| {
            filter_command_id.map_or(true, |id| e.command_id == id)
        })
        .map(|e| {
            json!({
                "CommandId": e.command_id,
                "DocumentName": e.document_name,
                "Targets": e.targets,
                "Status": e.status,
                "CreatedDate": e.created_time,
            })
        })
        .take(max_results)
        .collect();

    Ok(json!({ "Commands": commands }))
}

// ---------------------------------------------------------------------------
// GetCommandInvocation
// ---------------------------------------------------------------------------

pub fn get_command_invocation(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let command_id = input["CommandId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidCommandId", "CommandId is required"))?;

    let command = state.commands.get(command_id).ok_or_else(|| {
        AwsError::not_found(
            "InvalidCommandId",
            format!("Command {command_id} not found"),
        )
    })?;

    let instance_id = input["InstanceId"].as_str().unwrap_or("i-00000000");

    Ok(json!({
        "CommandId": command.command_id,
        "InstanceId": instance_id,
        "DocumentName": command.document_name,
        "Status": "Success",
        "StatusDetails": "Success",
        "StandardOutputContent": "",
        "StandardErrorContent": "",
    }))
}
