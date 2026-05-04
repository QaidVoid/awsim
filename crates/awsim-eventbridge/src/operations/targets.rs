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

        // InputTransformer: parse and validate. AWS requires exactly
        // one of Input / InputPath / InputTransformer per target.
        let input_transformer = match parse_input_transformer(&target_input["InputTransformer"]) {
            Ok(t) => t,
            Err(msg) => {
                failed_count += 1;
                failed_entries.push(json!({
                    "TargetId": id,
                    "ErrorCode": "InvalidParameterValue",
                    "ErrorMessage": msg,
                }));
                continue;
            }
        };

        let input_modes = [
            input_val.is_some(),
            input_path.is_some(),
            input_transformer.is_some(),
        ];
        if input_modes.iter().filter(|x| **x).count() > 1 {
            failed_count += 1;
            failed_entries.push(json!({
                "TargetId": id,
                "ErrorCode": "InvalidParameterValue",
                "ErrorMessage": "Specify at most one of Input, InputPath, or InputTransformer per target",
            }));
            continue;
        }

        // Upsert: replace if same ID already exists
        if let Some(pos) = rule.targets.iter().position(|t| t.id == id) {
            rule.targets[pos] = Target {
                id,
                arn,
                input: input_val,
                input_path,
                input_transformer,
            };
        } else {
            rule.targets.push(Target {
                id,
                arn,
                input: input_val,
                input_path,
                input_transformer,
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

/// Parse an `InputTransformer` Value into the internal struct,
/// returning Ok(None) when the input is null/absent. Validates that:
///   - InputTemplate is present and non-empty
///   - every key in InputPathsMap appears as `<key>` in the template
fn parse_input_transformer(
    value: &Value,
) -> Result<Option<crate::state::InputTransformer>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let obj = value
        .as_object()
        .ok_or_else(|| "InputTransformer must be an object".to_string())?;
    let template = obj
        .get("InputTemplate")
        .and_then(Value::as_str)
        .ok_or_else(|| "InputTransformer.InputTemplate is required".to_string())?
        .to_string();
    if template.is_empty() {
        return Err("InputTransformer.InputTemplate must be non-empty".to_string());
    }
    let paths: std::collections::HashMap<String, String> = obj
        .get("InputPathsMap")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    for key in paths.keys() {
        let placeholder = format!("<{key}>");
        if !template.contains(&placeholder) {
            return Err(format!(
                "InputPathsMap key '{key}' is not referenced in InputTemplate"
            ));
        }
    }
    Ok(Some(crate::state::InputTransformer {
        input_paths_map: paths,
        input_template: template,
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
