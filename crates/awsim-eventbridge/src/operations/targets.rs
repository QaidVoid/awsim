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

    // AWS caps a rule at 5 targets. Compute the post-call total
    // (existing targets keyed by Id minus the ones this call replaces,
    // plus new ones) and reject before any state mutation when it
    // would exceed the cap.
    const MAX_TARGETS_PER_RULE: usize = 5;
    {
        let incoming_ids: std::collections::HashSet<String> = targets_input
            .iter()
            .filter_map(|t| t.get("Id").and_then(|v| v.as_str()).map(str::to_string))
            .collect();
        let keep = rule
            .targets
            .iter()
            .filter(|t| !incoming_ids.contains(&t.id))
            .count();
        let projected = keep + incoming_ids.len();
        if projected > MAX_TARGETS_PER_RULE {
            return Err(AwsError::bad_request(
                "LimitExceededException",
                format!(
                    "Rule {rule_name} can have at most {MAX_TARGETS_PER_RULE} targets ({projected} requested)."
                ),
            ));
        }
    }

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

        let dead_letter_arn = match parse_dead_letter_config(&target_input["DeadLetterConfig"]) {
            Ok(v) => v,
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

        let retry_policy = match parse_retry_policy(&target_input["RetryPolicy"]) {
            Ok(v) => v,
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

        let batch_parameters = match target_input.get("BatchParameters") {
            Some(v) if !v.is_null() => {
                if !v.is_object() {
                    failed_count += 1;
                    failed_entries.push(json!({
                        "TargetId": id,
                        "ErrorCode": "InvalidParameterValue",
                        "ErrorMessage": "BatchParameters must be an object.",
                    }));
                    continue;
                }
                if v.get("JobDefinition")
                    .and_then(|j| j.as_str())
                    .filter(|s| !s.is_empty())
                    .is_none()
                {
                    failed_count += 1;
                    failed_entries.push(json!({
                        "TargetId": id,
                        "ErrorCode": "InvalidParameterValue",
                        "ErrorMessage": "BatchParameters.JobDefinition is required.",
                    }));
                    continue;
                }
                if v.get("JobName")
                    .and_then(|j| j.as_str())
                    .filter(|s| !s.is_empty())
                    .is_none()
                {
                    failed_count += 1;
                    failed_entries.push(json!({
                        "TargetId": id,
                        "ErrorCode": "InvalidParameterValue",
                        "ErrorMessage": "BatchParameters.JobName is required.",
                    }));
                    continue;
                }
                Some(v.clone())
            }
            _ => None,
        };

        // Upsert: replace if same ID already exists
        if let Some(pos) = rule.targets.iter().position(|t| t.id == id) {
            rule.targets[pos] = Target {
                id,
                arn,
                input: input_val,
                input_path,
                input_transformer,
                batch_parameters,
                dead_letter_arn,
                retry_policy,
            };
        } else {
            rule.targets.push(Target {
                id,
                arn,
                input: input_val,
                input_path,
                input_transformer,
                batch_parameters,
                dead_letter_arn,
                retry_policy,
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

/// Parse a `DeadLetterConfig` Value into an SQS ARN. EventBridge only
/// accepts SQS queue ARNs here; cross-region or non-SQS ARNs are
/// rejected at PutTargets with InvalidParameterValue.
fn parse_dead_letter_config(value: &Value) -> Result<Option<String>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let obj = value
        .as_object()
        .ok_or_else(|| "DeadLetterConfig must be an object".to_string())?;
    let Some(arn) = obj.get("Arn").and_then(Value::as_str) else {
        return Ok(None);
    };
    if arn.is_empty() {
        return Err("DeadLetterConfig.Arn must be non-empty".to_string());
    }
    if !arn.starts_with("arn:aws:sqs:") {
        return Err(format!(
            "DeadLetterConfig.Arn '{arn}' must be an SQS queue ARN"
        ));
    }
    Ok(Some(arn.to_string()))
}

/// Parse a `RetryPolicy` Value. AWS bounds MaximumEventAgeInSeconds at
/// 60..=86400 and MaximumRetryAttempts at 0..=185; outside-range values
/// are rejected.
fn parse_retry_policy(value: &Value) -> Result<Option<(u32, u32)>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let obj = value
        .as_object()
        .ok_or_else(|| "RetryPolicy must be an object".to_string())?;
    let age = match obj.get("MaximumEventAgeInSeconds").and_then(Value::as_i64) {
        Some(n) if !(60..=86_400).contains(&n) => {
            return Err(format!(
                "RetryPolicy.MaximumEventAgeInSeconds {n} must be between 60 and 86400"
            ));
        }
        Some(n) => n as u32,
        None => 86_400,
    };
    let attempts = match obj.get("MaximumRetryAttempts").and_then(Value::as_i64) {
        Some(n) if !(0..=185).contains(&n) => {
            return Err(format!(
                "RetryPolicy.MaximumRetryAttempts {n} must be between 0 and 185"
            ));
        }
        Some(n) => n as u32,
        None => 185,
    };
    Ok(Some((age, attempts)))
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
    if let Some(bp) = &target.batch_parameters {
        obj["BatchParameters"] = bp.clone();
    }
    if let Some(ref dlq) = target.dead_letter_arn {
        obj["DeadLetterConfig"] = json!({ "Arn": dlq });
    }
    if let Some((age, attempts)) = target.retry_policy {
        obj["RetryPolicy"] = json!({
            "MaximumEventAgeInSeconds": age,
            "MaximumRetryAttempts": attempts,
        });
    }
    obj
}

/// Outcome of a single delivery attempt against an EventBridge target.
/// Callers (the future delivery loop / tests) hand this back to
/// `next_delivery_step` to decide what to do.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Success,
    Failure,
}

/// What the dispatcher should do with a failed (or successful)
/// delivery attempt. AWS retries up to `MaximumRetryAttempts` and
/// caps the event's total age at `MaximumEventAgeInSeconds`; once
/// either ceiling is reached, the event goes to the DLQ when
/// configured or is dropped otherwise.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryStep {
    Done,
    Retry { attempt_index: u32 },
    SendToDeadLetter { dlq_arn: String },
    Drop,
}

/// Decide the next step given the outcome of the most recent attempt.
/// `attempts_so_far` counts attempts that already happened (including
/// the one whose outcome is being reported), so the first call sees
/// `attempts_so_far == 1`. `age_secs` is how long the event has been
/// in flight overall.
#[allow(dead_code)]
pub fn next_delivery_step(
    outcome: DeliveryOutcome,
    target: &Target,
    attempts_so_far: u32,
    age_secs: u64,
) -> DeliveryStep {
    if outcome == DeliveryOutcome::Success {
        return DeliveryStep::Done;
    }
    let (max_age, max_attempts) = target.retry_policy.unwrap_or((86_400, 185));
    let age_exceeded = age_secs >= u64::from(max_age);
    let attempts_exceeded = attempts_so_far > max_attempts;
    if age_exceeded || attempts_exceeded {
        return match target.dead_letter_arn.clone() {
            Some(dlq_arn) => DeliveryStep::SendToDeadLetter { dlq_arn },
            None => DeliveryStep::Drop,
        };
    }
    DeliveryStep::Retry {
        attempt_index: attempts_so_far,
    }
}

#[cfg(test)]
mod dlq_retry_tests {
    use super::*;

    #[test]
    fn parses_dead_letter_config_sqs_arn() {
        let v = json!({ "Arn": "arn:aws:sqs:us-east-1:000000000000:dlq" });
        assert_eq!(
            parse_dead_letter_config(&v).unwrap().as_deref(),
            Some("arn:aws:sqs:us-east-1:000000000000:dlq"),
        );
    }

    #[test]
    fn rejects_non_sqs_dlq_arn() {
        let v = json!({ "Arn": "arn:aws:sns:us-east-1:000000000000:topic" });
        let err = parse_dead_letter_config(&v).unwrap_err();
        assert!(err.contains("SQS"));
    }

    #[test]
    fn dlq_returns_none_when_absent() {
        assert!(parse_dead_letter_config(&Value::Null).unwrap().is_none());
    }

    #[test]
    fn parses_retry_policy_within_bounds() {
        let v = json!({ "MaximumEventAgeInSeconds": 3600, "MaximumRetryAttempts": 5 });
        assert_eq!(parse_retry_policy(&v).unwrap(), Some((3600, 5)));
    }

    #[test]
    fn applies_retry_policy_defaults() {
        let v = json!({});
        assert_eq!(parse_retry_policy(&v).unwrap(), Some((86_400, 185)));
    }

    #[test]
    fn rejects_retry_policy_age_below_minimum() {
        let v = json!({ "MaximumEventAgeInSeconds": 30 });
        assert!(parse_retry_policy(&v).is_err());
    }

    #[test]
    fn rejects_retry_policy_attempts_above_maximum() {
        let v = json!({ "MaximumRetryAttempts": 200 });
        assert!(parse_retry_policy(&v).is_err());
    }

    fn target_with(retry: Option<(u32, u32)>, dlq: Option<&str>) -> Target {
        Target {
            id: "t1".into(),
            arn: "arn:aws:lambda:us-east-1:0:function:f".into(),
            input: None,
            input_path: None,
            input_transformer: None,
            batch_parameters: None,
            dead_letter_arn: dlq.map(str::to_string),
            retry_policy: retry,
        }
    }

    #[test]
    fn success_returns_done() {
        let target = target_with(None, None);
        assert_eq!(
            next_delivery_step(DeliveryOutcome::Success, &target, 1, 0),
            DeliveryStep::Done
        );
    }

    #[test]
    fn first_failure_under_caps_schedules_retry() {
        let target = target_with(Some((3600, 5)), None);
        assert_eq!(
            next_delivery_step(DeliveryOutcome::Failure, &target, 1, 10),
            DeliveryStep::Retry { attempt_index: 1 }
        );
    }

    #[test]
    fn exhausted_attempts_route_to_dead_letter_when_configured() {
        let target = target_with(Some((3600, 3)), Some("arn:aws:sqs:us-east-1:0:dlq"));
        let step = next_delivery_step(DeliveryOutcome::Failure, &target, 4, 10);
        assert!(matches!(
            step,
            DeliveryStep::SendToDeadLetter { ref dlq_arn } if dlq_arn == "arn:aws:sqs:us-east-1:0:dlq"
        ));
    }

    #[test]
    fn exceeded_age_with_no_dlq_drops_event() {
        let target = target_with(Some((30, 100)), None);
        assert_eq!(
            next_delivery_step(DeliveryOutcome::Failure, &target, 1, 60),
            DeliveryStep::Drop
        );
    }

    #[test]
    fn no_retry_policy_uses_aws_defaults_for_cap() {
        // AWS default: 185 attempts, 86 400 s. So one failure after
        // a few seconds → still retry.
        let target = target_with(None, None);
        assert_eq!(
            next_delivery_step(DeliveryOutcome::Failure, &target, 1, 60),
            DeliveryStep::Retry { attempt_index: 1 }
        );
    }
}
