use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{StateMachine, StepFunctionsState};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn now_iso8601() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn build_sm_arn(ctx: &RequestContext, name: &str) -> String {
    format!(
        "arn:aws:states:{}:{}:stateMachine:{}",
        ctx.region, ctx.account_id, name
    )
}

fn sm_to_value(sm: &StateMachine) -> Value {
    json!({
        "stateMachineArn": sm.arn,
        "name": sm.name,
        "status": sm.status,
        "definition": sm.definition,
        "roleArn": sm.role_arn,
        "type": sm.machine_type,
        "creationDate": sm.creation_date,
    })
}

// ---------------------------------------------------------------------------
// CreateStateMachine
// ---------------------------------------------------------------------------

pub fn create_state_machine(
    state: &StepFunctionsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "name is required"))?;

    let definition = input["definition"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "definition is required"))?;

    // Validate definition is valid JSON
    serde_json::from_str::<Value>(definition).map_err(|e| {
        AwsError::bad_request(
            "InvalidDefinition",
            format!("definition is not valid JSON: {e}"),
        )
    })?;

    let role_arn = input["roleArn"].as_str().unwrap_or("").to_string();
    let machine_type = input["type"].as_str().unwrap_or("STANDARD").to_string();

    match machine_type.as_str() {
        "STANDARD" | "EXPRESS" => {}
        _ => {
            return Err(AwsError::bad_request(
                "InvalidParameter",
                format!("Invalid type: {machine_type}. Must be STANDARD or EXPRESS"),
            ));
        }
    }

    let arn = build_sm_arn(ctx, name);

    if state.state_machines.contains_key(&arn) {
        return Err(AwsError::conflict(
            "StateMachineAlreadyExists",
            format!("State machine already exists: {arn}"),
        ));
    }

    let creation_date = now_iso8601();
    let sm = StateMachine {
        name: name.to_string(),
        arn: arn.clone(),
        definition: definition.to_string(),
        role_arn,
        machine_type,
        status: "ACTIVE".to_string(),
        creation_date: creation_date.clone(),
    };

    info!(name, arn = %arn, "Created state machine");
    state.state_machines.insert(arn.clone(), sm);

    Ok(json!({
        "stateMachineArn": arn,
        "creationDate": creation_date,
    }))
}

// ---------------------------------------------------------------------------
// DeleteStateMachine
// ---------------------------------------------------------------------------

pub fn delete_state_machine(
    state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["stateMachineArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "stateMachineArn is required"))?;

    state.state_machines.remove(arn).ok_or_else(|| {
        AwsError::not_found(
            "StateMachineDoesNotExist",
            format!("State machine not found: {arn}"),
        )
    })?;

    info!(arn, "Deleted state machine");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DescribeStateMachine
// ---------------------------------------------------------------------------

pub fn describe_state_machine(
    state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["stateMachineArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "stateMachineArn is required"))?;

    let sm = state.state_machines.get(arn).ok_or_else(|| {
        AwsError::not_found(
            "StateMachineDoesNotExist",
            format!("State machine not found: {arn}"),
        )
    })?;

    Ok(sm_to_value(&sm))
}

// ---------------------------------------------------------------------------
// ListStateMachines
// ---------------------------------------------------------------------------

pub fn list_state_machines(
    state: &StepFunctionsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let mut machines: Vec<Value> = state
        .state_machines
        .iter()
        .map(|entry| {
            let sm = entry.value();
            json!({
                "stateMachineArn": sm.arn,
                "name": sm.name,
                "type": sm.machine_type,
                "creationDate": sm.creation_date,
            })
        })
        .collect();

    machines.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
    });

    Ok(json!({ "stateMachines": machines }))
}

// ---------------------------------------------------------------------------
// UpdateStateMachine
// ---------------------------------------------------------------------------

pub fn update_state_machine(
    state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["stateMachineArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "stateMachineArn is required"))?;

    let mut sm = state.state_machines.get_mut(arn).ok_or_else(|| {
        AwsError::not_found(
            "StateMachineDoesNotExist",
            format!("State machine not found: {arn}"),
        )
    })?;

    if let Some(definition) = input["definition"].as_str() {
        serde_json::from_str::<Value>(definition).map_err(|e| {
            AwsError::bad_request(
                "InvalidDefinition",
                format!("definition is not valid JSON: {e}"),
            )
        })?;
        sm.definition = definition.to_string();
    }

    if let Some(role_arn) = input["roleArn"].as_str() {
        sm.role_arn = role_arn.to_string();
    }

    let update_date = now_iso8601();
    info!(arn, "Updated state machine");

    Ok(json!({ "updateDate": update_date }))
}
