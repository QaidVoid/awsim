use std::collections::HashMap;
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

/// Validate `roleArn` shape when the caller supplies one. AWS only
/// requires a roleArn for state machines that perform service
/// integrations, and rejects malformed ARNs with InvalidParameter. An
/// empty roleArn passes here so unit tests can construct machines
/// without spinning up an IAM role.
fn validate_role_arn(role_arn: &str) -> Result<(), AwsError> {
    if role_arn.is_empty() {
        return Ok(());
    }
    let parts: Vec<&str> = role_arn.splitn(6, ':').collect();
    let shape_ok = parts.len() == 6
        && parts[0] == "arn"
        && (parts[1] == "aws" || parts[1].starts_with("aws-"))
        && parts[2] == "iam"
        && parts[3].is_empty()
        && parts[5].starts_with("role/");
    if !shape_ok {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            format!("roleArn `{role_arn}` is not a valid IAM role ARN."),
        ));
    }
    Ok(())
}

fn sm_to_value(sm: &StateMachine) -> Value {
    let mut obj = json!({
        "stateMachineArn": sm.arn,
        "name": sm.name,
        "status": sm.status,
        "definition": sm.definition,
        "roleArn": sm.role_arn,
        "type": sm.machine_type,
        "creationDate": sm.creation_date,
    });
    if let Some(tc) = &sm.tracing_configuration {
        obj["tracingConfiguration"] = tc.clone();
    }
    if let Some(ec) = &sm.encryption_configuration {
        obj["encryptionConfiguration"] = ec.clone();
    }
    obj
}

/// Validate `tracingConfiguration` per Smithy: an object with an
/// optional boolean `enabled`. Anything else fails CreateStateMachine
/// with InvalidParameterValue.
fn validate_tracing_config(input: &Value) -> Result<Option<Value>, AwsError> {
    let Some(tc) = input.get("tracingConfiguration") else {
        return Ok(None);
    };
    if tc.is_null() {
        return Ok(None);
    }
    let obj = tc.as_object().ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterValue",
            "tracingConfiguration must be an object.",
        )
    })?;
    if let Some(enabled) = obj.get("enabled")
        && !enabled.is_boolean()
    {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            "tracingConfiguration.enabled must be a boolean.",
        ));
    }
    Ok(Some(Value::Object(obj.clone())))
}

/// Validate `encryptionConfiguration`: `type` must be
/// `AWS_OWNED_KEY` or `CUSTOMER_MANAGED_KMS_KEY`; the latter requires
/// a non-empty `kmsKeyId`. Returns the validated object.
fn validate_encryption_config(input: &Value) -> Result<Option<Value>, AwsError> {
    let Some(ec) = input.get("encryptionConfiguration") else {
        return Ok(None);
    };
    if ec.is_null() {
        return Ok(None);
    }
    let obj = ec.as_object().ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterValue",
            "encryptionConfiguration must be an object.",
        )
    })?;
    let ty = obj.get("type").and_then(Value::as_str).ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterValue",
            "encryptionConfiguration.type is required.",
        )
    })?;
    if !matches!(ty, "AWS_OWNED_KEY" | "CUSTOMER_MANAGED_KMS_KEY") {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            format!(
                "encryptionConfiguration.type `{ty}` must be AWS_OWNED_KEY or CUSTOMER_MANAGED_KMS_KEY."
            ),
        ));
    }
    if ty == "CUSTOMER_MANAGED_KMS_KEY" {
        let kms_key = obj.get("kmsKeyId").and_then(Value::as_str).unwrap_or("");
        if kms_key.is_empty() {
            return Err(AwsError::bad_request(
                "InvalidParameterValue",
                "encryptionConfiguration.kmsKeyId is required when type is CUSTOMER_MANAGED_KMS_KEY.",
            ));
        }
    }
    Ok(Some(Value::Object(obj.clone())))
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
    validate_role_arn(&role_arn)?;
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

    // Extract tags from CreateStateMachine input
    let tags: HashMap<String, String> = input["tags"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let k = t["key"].as_str()?;
                    let v = t["value"].as_str()?;
                    Some((k.to_string(), v.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();

    let tracing_configuration = validate_tracing_config(input)?;
    let encryption_configuration = validate_encryption_config(input)?;

    let creation_date = now_iso8601();
    let sm = StateMachine {
        name: name.to_string(),
        arn: arn.clone(),
        definition: definition.to_string(),
        role_arn,
        machine_type,
        status: "ACTIVE".to_string(),
        creation_date: creation_date.clone(),
        tags,
        tracing_configuration,
        encryption_configuration,
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
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
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
        validate_role_arn(role_arn)?;
        sm.role_arn = role_arn.to_string();
    }

    if input.get("tracingConfiguration").is_some() {
        sm.tracing_configuration = validate_tracing_config(input)?;
    }
    if input.get("encryptionConfiguration").is_some() {
        sm.encryption_configuration = validate_encryption_config(input)?;
    }

    let update_date = now_iso8601();
    info!(arn, "Updated state machine");

    Ok(json!({ "updateDate": update_date }))
}
