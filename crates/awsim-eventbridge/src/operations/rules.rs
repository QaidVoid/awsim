use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::operations::buses::ensure_default_bus;
use crate::state::{EventBridgeState, Rule};

// ---------------------------------------------------------------------------
// PutRule
// ---------------------------------------------------------------------------

pub fn put_rule(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Name is required"))?;

    let bus_name = input["EventBusName"].as_str().unwrap_or("default");

    ensure_default_bus(state, ctx);

    let mut bus = state.event_buses.get_mut(bus_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Event bus {bus_name} does not exist"),
        )
    })?;

    let event_pattern = input["EventPattern"].as_str().map(|s| s.to_string());
    let schedule_expression = input["ScheduleExpression"].as_str().map(|s| s.to_string());

    // At least one of EventPattern or ScheduleExpression must be specified
    if event_pattern.is_none() && schedule_expression.is_none() {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            "Either EventPattern or ScheduleExpression must be specified",
        ));
    }

    let state_str = input["State"].as_str().unwrap_or("ENABLED").to_string();
    if state_str != "ENABLED" && state_str != "DISABLED" {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            "State must be ENABLED or DISABLED",
        ));
    }

    let description = input["Description"].as_str().unwrap_or("").to_string();

    let arn = format!(
        "arn:aws:events:{}:{}:rule/{}/{}",
        ctx.region, ctx.account_id, bus_name, name
    );

    let existing_targets = bus
        .rules
        .get(name)
        .map(|r| r.targets.clone())
        .unwrap_or_default();

    let rule = Rule {
        name: name.to_string(),
        arn: arn.clone(),
        event_bus_name: bus_name.to_string(),
        event_pattern,
        schedule_expression,
        state: state_str,
        description,
        targets: existing_targets,
    };

    info!(rule = %name, bus = %bus_name, "Put rule");
    bus.rules.insert(name.to_string(), rule);

    Ok(json!({ "RuleArn": arn }))
}

// ---------------------------------------------------------------------------
// DeleteRule
// ---------------------------------------------------------------------------

pub fn delete_rule(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Name is required"))?;

    let bus_name = input["EventBusName"].as_str().unwrap_or("default");

    ensure_default_bus(state, ctx);

    let mut bus = state.event_buses.get_mut(bus_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Event bus {bus_name} does not exist"),
        )
    })?;

    let force = input["Force"].as_bool().unwrap_or(false);

    let target_count = bus
        .rules
        .get(name)
        .map(|r| r.targets.len())
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Rule {name} does not exist on event bus {bus_name}"),
            )
        })?;

    // AWS rejects DeleteRule when the rule still has targets attached
    // unless the caller explicitly passes Force=true. The error code is
    // ManagedRuleException for managed rules and a plain client error
    // for the user-rule case; we surface it as the ValidationException
    // shape that user code commonly catches.
    if target_count > 0 && !force {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "Rule {name} still has {target_count} target(s) attached; \
                 set Force=true or remove the targets first"
            ),
        ));
    }

    bus.rules.remove(name);
    info!(rule = %name, bus = %bus_name, "Deleted rule");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DescribeRule
// ---------------------------------------------------------------------------

pub fn describe_rule(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Name is required"))?;

    let bus_name = input["EventBusName"].as_str().unwrap_or("default");

    ensure_default_bus(state, ctx);

    let bus = state.event_buses.get(bus_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Event bus {bus_name} does not exist"),
        )
    })?;

    let rule = bus.rules.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Rule {name} does not exist on event bus {bus_name}"),
        )
    })?;

    Ok(rule_to_json(rule))
}

// ---------------------------------------------------------------------------
// ListRules
// ---------------------------------------------------------------------------

pub fn list_rules(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let bus_name = input["EventBusName"].as_str().unwrap_or("default");
    let name_prefix = input["NamePrefix"].as_str().unwrap_or("");

    ensure_default_bus(state, ctx);

    let bus = state.event_buses.get(bus_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Event bus {bus_name} does not exist"),
        )
    })?;

    let rules: Vec<Value> = bus
        .rules
        .values()
        .filter(|r| name_prefix.is_empty() || r.name.starts_with(name_prefix))
        .map(rule_to_json)
        .collect();

    Ok(json!({ "Rules": rules }))
}

// ---------------------------------------------------------------------------
// EnableRule / DisableRule
// ---------------------------------------------------------------------------

pub fn enable_rule(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    set_rule_state(state, input, ctx, "ENABLED")
}

pub fn disable_rule(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    set_rule_state(state, input, ctx, "DISABLED")
}

fn set_rule_state(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
    new_state: &str,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Name is required"))?;

    let bus_name = input["EventBusName"].as_str().unwrap_or("default");

    ensure_default_bus(state, ctx);

    let mut bus = state.event_buses.get_mut(bus_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Event bus {bus_name} does not exist"),
        )
    })?;

    let rule = bus.rules.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Rule {name} does not exist on event bus {bus_name}"),
        )
    })?;

    rule.state = new_state.to_string();
    info!(rule = %name, bus = %bus_name, state = %new_state, "Set rule state");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn rule_to_json(rule: &Rule) -> Value {
    let mut obj = json!({
        "Name": rule.name,
        "Arn": rule.arn,
        "EventBusName": rule.event_bus_name,
        "State": rule.state,
        "Description": rule.description,
    });

    if let Some(ep) = &rule.event_pattern {
        obj["EventPattern"] = Value::String(ep.clone());
    }
    if let Some(se) = &rule.schedule_expression {
        obj["ScheduleExpression"] = Value::String(se.clone());
    }

    obj
}
