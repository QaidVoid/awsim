//! Inbound email receipt rules (SES v1). AWS uses receipt rule sets to
//! process mail delivered to verified domains: each rule matches a set
//! of recipients and runs an ordered list of actions (deliver to S3,
//! notify SNS, invoke Lambda, bounce, add a header, stop). There is no
//! real inbound SMTP path in the emulator, so delivery is driven
//! synthetically via the awsim-only `DeliverReceiptMessage` operation,
//! which walks the active rule set and emits a `ses:ReceiptAction`
//! event per action for the binary's router to fan out.

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{ReceiptRule, ReceiptRuleSet, SesState};

fn now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Serialize a stored rule back into the AWS `ReceiptRule` shape.
fn rule_to_json(rule: &ReceiptRule) -> Value {
    let mut obj = json!({
        "Name": rule.name,
        "Enabled": rule.enabled,
        "ScanEnabled": rule.scan_enabled,
        "Recipients": rule.recipients,
        "Actions": rule.actions,
    });
    if let Some(tls) = &rule.tls_policy {
        obj["TlsPolicy"] = json!(tls);
    }
    obj
}

/// Parse an AWS `ReceiptRule` input object into the stored form.
fn parse_rule(r: &Value) -> Result<ReceiptRule, AwsError> {
    let name = r["Name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Rule.Name is required"))?
        .to_string();
    Ok(ReceiptRule {
        name,
        enabled: r["Enabled"].as_bool().unwrap_or(true),
        recipients: r["Recipients"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        scan_enabled: r["ScanEnabled"].as_bool().unwrap_or(false),
        tls_policy: r["TlsPolicy"].as_str().map(String::from),
        actions: r["Actions"].as_array().cloned().unwrap_or_default(),
    })
}

pub fn create_receipt_rule_set(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["RuleSetName"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "RuleSetName is required"))?;
    if state.receipt_rule_sets.contains_key(name) {
        return Err(AwsError::bad_request(
            "AlreadyExists",
            format!("Rule set already exists: {name}"),
        ));
    }
    state.receipt_rule_sets.insert(
        name.to_string(),
        ReceiptRuleSet {
            name: name.to_string(),
            created_at: now(),
            rules: Vec::new(),
        },
    );
    Ok(json!({}))
}

pub fn delete_receipt_rule_set(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["RuleSetName"].as_str().unwrap_or("");
    // AWS refuses to delete the active rule set.
    if state.active_receipt_rule_set.lock().unwrap().as_deref() == Some(name) {
        return Err(AwsError::bad_request(
            "CannotDelete",
            format!("Cannot delete active rule set: {name}"),
        ));
    }
    state.receipt_rule_sets.remove(name);
    Ok(json!({}))
}

pub fn describe_receipt_rule_set(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["RuleSetName"].as_str().unwrap_or("");
    let set = state.receipt_rule_sets.get(name).ok_or_else(|| {
        AwsError::not_found(
            "RuleSetDoesNotExist",
            format!("Rule set does not exist: {name}"),
        )
    })?;
    Ok(json!({
        "Metadata": { "Name": set.name, "CreatedTimestamp": set.created_at },
        "Rules": set.rules.iter().map(rule_to_json).collect::<Vec<_>>(),
    }))
}

pub fn list_receipt_rule_sets(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let sets: Vec<Value> = state
        .receipt_rule_sets
        .iter()
        .map(|e| json!({ "Name": e.name, "CreatedTimestamp": e.created_at }))
        .collect();
    Ok(json!({ "RuleSets": sets }))
}

pub fn set_active_receipt_rule_set(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // AWS allows clearing the active set by omitting RuleSetName.
    match input["RuleSetName"].as_str() {
        Some(name) => {
            if !state.receipt_rule_sets.contains_key(name) {
                return Err(AwsError::not_found(
                    "RuleSetDoesNotExist",
                    format!("Rule set does not exist: {name}"),
                ));
            }
            *state.active_receipt_rule_set.lock().unwrap() = Some(name.to_string());
        }
        None => *state.active_receipt_rule_set.lock().unwrap() = None,
    }
    Ok(json!({}))
}

pub fn describe_active_receipt_rule_set(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let active = state.active_receipt_rule_set.lock().unwrap().clone();
    match active.and_then(|n| state.receipt_rule_sets.get(&n).map(|s| s.clone())) {
        Some(set) => Ok(json!({
            "Metadata": { "Name": set.name, "CreatedTimestamp": set.created_at },
            "Rules": set.rules.iter().map(rule_to_json).collect::<Vec<_>>(),
        })),
        None => Ok(json!({})),
    }
}

pub fn create_receipt_rule(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let set_name = input["RuleSetName"].as_str().unwrap_or("");
    let rule = parse_rule(&input["Rule"])?;
    let mut set = state.receipt_rule_sets.get_mut(set_name).ok_or_else(|| {
        AwsError::not_found(
            "RuleSetDoesNotExist",
            format!("Rule set does not exist: {set_name}"),
        )
    })?;
    if set.rules.iter().any(|r| r.name == rule.name) {
        return Err(AwsError::bad_request(
            "AlreadyExists",
            format!("Rule already exists: {}", rule.name),
        ));
    }
    // `After` inserts the new rule directly after the named rule.
    match input["After"].as_str().filter(|s| !s.is_empty()) {
        Some(after) => {
            let pos = set
                .rules
                .iter()
                .position(|r| r.name == after)
                .map(|i| i + 1)
                .unwrap_or(set.rules.len());
            set.rules.insert(pos, rule);
        }
        None => set.rules.push(rule),
    }
    Ok(json!({}))
}

pub fn update_receipt_rule(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let set_name = input["RuleSetName"].as_str().unwrap_or("");
    let rule = parse_rule(&input["Rule"])?;
    let mut set = state.receipt_rule_sets.get_mut(set_name).ok_or_else(|| {
        AwsError::not_found(
            "RuleSetDoesNotExist",
            format!("Rule set does not exist: {set_name}"),
        )
    })?;
    let slot = set
        .rules
        .iter_mut()
        .find(|r| r.name == rule.name)
        .ok_or_else(|| {
            AwsError::not_found(
                "RuleDoesNotExist",
                format!("Rule does not exist: {}", rule.name),
            )
        })?;
    *slot = rule;
    Ok(json!({}))
}

pub fn delete_receipt_rule(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let set_name = input["RuleSetName"].as_str().unwrap_or("");
    let rule_name = input["RuleName"].as_str().unwrap_or("");
    let mut set = state.receipt_rule_sets.get_mut(set_name).ok_or_else(|| {
        AwsError::not_found(
            "RuleSetDoesNotExist",
            format!("Rule set does not exist: {set_name}"),
        )
    })?;
    set.rules.retain(|r| r.name != rule_name);
    Ok(json!({}))
}

pub fn describe_receipt_rule(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let set_name = input["RuleSetName"].as_str().unwrap_or("");
    let rule_name = input["RuleName"].as_str().unwrap_or("");
    let set = state.receipt_rule_sets.get(set_name).ok_or_else(|| {
        AwsError::not_found(
            "RuleSetDoesNotExist",
            format!("Rule set does not exist: {set_name}"),
        )
    })?;
    let rule = set
        .rules
        .iter()
        .find(|r| r.name == rule_name)
        .ok_or_else(|| {
            AwsError::not_found(
                "RuleDoesNotExist",
                format!("Rule does not exist: {rule_name}"),
            )
        })?;
    Ok(json!({ "Rule": rule_to_json(rule) }))
}

pub fn reorder_receipt_rule_set(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let set_name = input["RuleSetName"].as_str().unwrap_or("");
    let order: Vec<String> = input["RuleNames"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let mut set = state.receipt_rule_sets.get_mut(set_name).ok_or_else(|| {
        AwsError::not_found(
            "RuleSetDoesNotExist",
            format!("Rule set does not exist: {set_name}"),
        )
    })?;
    // Every named rule must exist and the list must be a permutation.
    if order.len() != set.rules.len()
        || order
            .iter()
            .any(|n| !set.rules.iter().any(|r| &r.name == n))
    {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            "RuleNames must be a permutation of the existing rule names",
        ));
    }
    let mut reordered = Vec::with_capacity(set.rules.len());
    for name in &order {
        if let Some(pos) = set.rules.iter().position(|r| &r.name == name) {
            reordered.push(set.rules.remove(pos));
        }
    }
    set.rules = reordered;
    Ok(json!({}))
}

/// awsim-only synthetic inbound delivery. Resolves the named (or
/// active) rule set, walks its enabled rules in order, and for every
/// matching rule runs each action: a `ses:ReceiptAction` event is
/// emitted onto the bus (best-effort; `None` in tests) and the action
/// is appended to the returned summary so callers can assert behavior
/// without a bus. A `StopAction` halts all further rule processing.
pub fn deliver_receipt_message(
    state: &SesState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let active = state.active_receipt_rule_set.lock().unwrap().clone();
    let set_name = input["RuleSetName"]
        .as_str()
        .map(String::from)
        .or(active)
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterValue", "no active receipt rule set")
        })?;
    let set = state.receipt_rule_sets.get(&set_name).ok_or_else(|| {
        AwsError::not_found(
            "RuleSetDoesNotExist",
            format!("Rule set does not exist: {set_name}"),
        )
    })?;
    let recipients: Vec<String> = input["Recipients"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let sample = input["MessageData"]
        .as_str()
        .unwrap_or("From: sender@example.com\r\nSubject: sample\r\n\r\nbody");
    let message_id = uuid::Uuid::new_v4().to_string();

    let mut emitted: Vec<Value> = Vec::new();
    'rules: for rule in &set.rules {
        if !rule.enabled {
            continue;
        }
        if !rule.recipients.is_empty()
            && !rule
                .recipients
                .iter()
                .any(|r| recipients.iter().any(|x| x == r))
        {
            continue;
        }
        for action in &rule.actions {
            let action_type = action
                .as_object()
                .and_then(|m| m.keys().next().cloned())
                .unwrap_or_else(|| "Unknown".to_string());
            let detail = json!({
                "rule": rule.name,
                "actionType": action_type,
                "action": action,
                "messageId": message_id,
                "recipients": recipients,
                "sampleBytes": sample.as_bytes().iter().take(256).copied().collect::<Vec<_>>(),
            });
            if let Some(bus) = ctx.event_bus.as_ref() {
                bus.publish(awsim_core::events::InternalEvent {
                    source: "ses".into(),
                    event_type: "ses:ReceiptAction".into(),
                    region: ctx.region.clone(),
                    account_id: ctx.account_id.clone(),
                    detail: detail.clone(),
                });
            }
            emitted.push(detail);
            if action_type == "StopAction" {
                break 'rules;
            }
        }
    }
    Ok(json!({ "MessageId": message_id, "Actions": emitted }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("ses", "us-east-1")
    }

    fn seeded() -> SesState {
        let state = SesState::default();
        create_receipt_rule_set(&state, &json!({ "RuleSetName": "rs" }), &ctx()).unwrap();
        state
    }

    #[test]
    fn create_describe_round_trips_rule() {
        let state = seeded();
        create_receipt_rule(
            &state,
            &json!({
                "RuleSetName": "rs",
                "Rule": {
                    "Name": "r1",
                    "Enabled": true,
                    "Recipients": ["a@example.com"],
                    "Actions": [{ "SNSAction": { "TopicArn": "arn:aws:sns:us-east-1:000000000000:t" } }],
                },
            }),
            &ctx(),
        )
        .unwrap();
        let out =
            describe_receipt_rule_set(&state, &json!({ "RuleSetName": "rs" }), &ctx()).unwrap();
        assert_eq!(out["Rules"][0]["Name"], "r1");
        assert_eq!(out["Rules"][0]["Recipients"][0], "a@example.com");
        assert_eq!(
            out["Rules"][0]["Actions"][0]["SNSAction"]["TopicArn"],
            "arn:aws:sns:us-east-1:000000000000:t"
        );
    }

    #[test]
    fn create_rule_on_missing_set_errors() {
        let state = SesState::default();
        let err = create_receipt_rule(
            &state,
            &json!({ "RuleSetName": "nope", "Rule": { "Name": "r1" } }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "RuleSetDoesNotExist");
    }

    #[test]
    fn set_and_describe_active_rule_set() {
        let state = seeded();
        assert!(
            describe_active_receipt_rule_set(&state, &json!({}), &ctx())
                .unwrap()
                .get("Metadata")
                .is_none()
        );
        set_active_receipt_rule_set(&state, &json!({ "RuleSetName": "rs" }), &ctx()).unwrap();
        let out = describe_active_receipt_rule_set(&state, &json!({}), &ctx()).unwrap();
        assert_eq!(out["Metadata"]["Name"], "rs");
    }

    #[test]
    fn deliver_walks_actions_in_order_and_returns_summary() {
        let state = seeded();
        create_receipt_rule(
            &state,
            &json!({
                "RuleSetName": "rs",
                "Rule": {
                    "Name": "r1",
                    "Actions": [
                        { "SNSAction": { "TopicArn": "arn:aws:sns:us-east-1:000000000000:t" } },
                        { "S3Action": { "BucketName": "b", "ObjectKeyPrefix": "mail/" } }
                    ],
                },
            }),
            &ctx(),
        )
        .unwrap();
        set_active_receipt_rule_set(&state, &json!({ "RuleSetName": "rs" }), &ctx()).unwrap();
        let out =
            deliver_receipt_message(&state, &json!({ "Recipients": ["a@example.com"] }), &ctx())
                .unwrap();
        let actions = out["Actions"].as_array().unwrap();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0]["actionType"], "SNSAction");
        assert_eq!(actions[1]["actionType"], "S3Action");
        assert!(out["MessageId"].is_string());
    }

    #[test]
    fn deliver_respects_recipient_match_and_stop_action() {
        let state = seeded();
        create_receipt_rule(
            &state,
            &json!({
                "RuleSetName": "rs",
                "Rule": {
                    "Name": "scoped",
                    "Recipients": ["only@example.com"],
                    "Actions": [{ "BounceAction": { "SmtpReplyCode": "550" } }],
                },
            }),
            &ctx(),
        )
        .unwrap();
        create_receipt_rule(
            &state,
            &json!({
                "RuleSetName": "rs",
                "Rule": {
                    "Name": "stopper",
                    "Actions": [
                        { "StopAction": { "Scope": "RuleSet" } },
                        { "SNSAction": { "TopicArn": "arn:aws:sns:us-east-1:000000000000:t" } }
                    ],
                },
            }),
            &ctx(),
        )
        .unwrap();
        // Recipient doesn't match the scoped rule, so only the stopper runs;
        // StopAction halts before the trailing SNSAction.
        let out = deliver_receipt_message(
            &state,
            &json!({ "RuleSetName": "rs", "Recipients": ["someone@example.com"] }),
            &ctx(),
        )
        .unwrap();
        let actions = out["Actions"].as_array().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0]["actionType"], "StopAction");
    }

    #[test]
    fn reorder_permutes_rules() {
        let state = seeded();
        for name in ["a", "b", "c"] {
            create_receipt_rule(
                &state,
                &json!({ "RuleSetName": "rs", "Rule": { "Name": name } }),
                &ctx(),
            )
            .unwrap();
        }
        reorder_receipt_rule_set(
            &state,
            &json!({ "RuleSetName": "rs", "RuleNames": ["c", "a", "b"] }),
            &ctx(),
        )
        .unwrap();
        let out =
            describe_receipt_rule_set(&state, &json!({ "RuleSetName": "rs" }), &ctx()).unwrap();
        let names: Vec<&str> = out["Rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["Name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["c", "a", "b"]);
    }
}
