use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::operations::buses::ensure_default_bus;
use crate::state::{EventBridgeState, Target};

// ---------------------------------------------------------------------------
// PutTargets
// ---------------------------------------------------------------------------

pub fn put_targets(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let rule_name = input["Rule"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Rule is required"))?;

    let bus_name = input["EventBusName"].as_str().unwrap_or("default");

    let targets_input = input["Targets"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Targets is required"))?;

    ensure_default_bus(state, ctx);

    let mut bus = state.event_buses.get_mut(bus_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Event bus {bus_name} does not exist"),
        )
    })?;

    let rule = bus.rules.get_mut(rule_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Rule {rule_name} does not exist on event bus {bus_name}"),
        )
    })?;

    let mut failed_entries: Vec<Value> = Vec::new();
    let mut failed_count = 0u64;

    for target_input in targets_input {
        let id = match target_input["Id"].as_str() {
            Some(id) => id.to_string(),
            None => {
                failed_count += 1;
                failed_entries.push(json!({
                    "TargetId": Value::Null,
                    "ErrorCode": "InvalidParameterValue",
                    "ErrorMessage": "Id is required for each target",
                }));
                continue;
            }
        };

        let arn = match target_input["Arn"].as_str() {
            Some(a) => a.to_string(),
            None => {
                failed_count += 1;
                failed_entries.push(json!({
                    "TargetId": id,
                    "ErrorCode": "InvalidParameterValue",
                    "ErrorMessage": "Arn is required for each target",
                }));
                continue;
            }
        };

        let input_val = target_input["Input"].as_str().map(|s| s.to_string());
        let input_path = target_input["InputPath"].as_str().map(|s| s.to_string());

        // Upsert: replace if same ID already exists
        if let Some(pos) = rule.targets.iter().position(|t| t.id == id) {
            rule.targets[pos] = Target {
                id,
                arn,
                input: input_val,
                input_path,
            };
        } else {
            rule.targets.push(Target {
                id,
                arn,
                input: input_val,
                input_path,
            });
        }
    }

    info!(
        rule = %rule_name,
        bus = %bus_name,
        added = targets_input.len() - failed_count as usize,
        failed = failed_count,
        "Put targets"
    );

    Ok(json!({
        "FailedEntryCount": failed_count,
        "FailedEntries": failed_entries,
    }))
}

// ---------------------------------------------------------------------------
// RemoveTargets
// ---------------------------------------------------------------------------

pub fn remove_targets(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let rule_name = input["Rule"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Rule is required"))?;

    let bus_name = input["EventBusName"].as_str().unwrap_or("default");

    let ids = input["Ids"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Ids is required"))?;

    ensure_default_bus(state, ctx);

    let mut bus = state.event_buses.get_mut(bus_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Event bus {bus_name} does not exist"),
        )
    })?;

    let rule = bus.rules.get_mut(rule_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Rule {rule_name} does not exist on event bus {bus_name}"),
        )
    })?;

    let mut failed_entries: Vec<Value> = Vec::new();
    let mut failed_count = 0u64;

    for id_val in ids {
        if let Some(id) = id_val.as_str() {
            let before_len = rule.targets.len();
            rule.targets.retain(|t| t.id != id);
            if rule.targets.len() == before_len {
                // Not found — AWS still succeeds, but records a failure entry
                failed_count += 1;
                failed_entries.push(json!({
                    "TargetId": id,
                    "ErrorCode": "ResourceNotFoundException",
                    "ErrorMessage": format!("Target {id} not found on rule {rule_name}"),
                }));
            }
        }
    }

    info!(rule = %rule_name, bus = %bus_name, "Removed targets");

    Ok(json!({
        "FailedEntryCount": failed_count,
        "FailedEntries": failed_entries,
    }))
}

// ---------------------------------------------------------------------------
// ListTargetsByRule
// ---------------------------------------------------------------------------

pub fn list_targets_by_rule(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let rule_name = input["Rule"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Rule is required"))?;

    let bus_name = input["EventBusName"].as_str().unwrap_or("default");

    ensure_default_bus(state, ctx);

    let bus = state.event_buses.get(bus_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Event bus {bus_name} does not exist"),
        )
    })?;

    let rule = bus.rules.get(rule_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Rule {rule_name} does not exist on event bus {bus_name}"),
        )
    })?;

    let targets: Vec<Value> = rule.targets.iter().map(target_to_json).collect();

    Ok(json!({ "Targets": targets }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn target_to_json(target: &Target) -> Value {
    let mut obj = json!({
        "Id": target.id,
        "Arn": target.arn,
    });
    if let Some(inp) = &target.input {
        obj["Input"] = Value::String(inp.clone());
    }
    if let Some(ip) = &target.input_path {
        obj["InputPath"] = Value::String(ip.clone());
    }
    obj
}
