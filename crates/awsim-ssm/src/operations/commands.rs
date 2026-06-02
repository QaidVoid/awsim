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

    let targets = input["Targets"].as_array().cloned().unwrap_or_default();

    // Resolve the target instances: explicit InstanceIds plus any
    // `instanceids` target Values.
    let mut instance_ids: Vec<String> = input["InstanceIds"]
        .as_array()
        .map(|ids| {
            ids.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    for target in &targets {
        let is_instance_key = target["Key"]
            .as_str()
            .is_some_and(|k| k.eq_ignore_ascii_case("instanceids"));
        if is_instance_key && let Some(values) = target["Values"].as_array() {
            instance_ids.extend(values.iter().filter_map(|v| v.as_str().map(String::from)));
        }
    }

    let command_id = new_command_id();
    let now = now_epoch_secs();

    let command = Command {
        command_id: command_id.clone(),
        document_name: document_name.to_string(),
        targets: targets.clone(),
        instance_ids,
        status: "Pending".to_string(),
        created_time: now,
        std_out: String::new(),
        std_err: String::new(),
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
        .filter(|e| filter_command_id.is_none_or(|id| e.command_id == id))
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
        AwsError::bad_request(
            "InvalidCommandId",
            format!("Command {command_id} not found"),
        )
    })?;

    let instance_id = input["InstanceId"]
        .as_str()
        .map(String::from)
        .or_else(|| command.instance_ids.first().cloned())
        .unwrap_or_else(|| "i-00000000".to_string());

    Ok(json!({
        "CommandId": command.command_id,
        "InstanceId": instance_id,
        "DocumentName": command.document_name,
        "Status": command.status,
        "StatusDetails": command.status,
        "StandardOutputContent": command.std_out,
        "StandardErrorContent": command.std_err,
    }))
}
