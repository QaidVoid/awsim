use awsim_core::{AwsError, RequestContext, arn};
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

    if let Some(expr) = schedule_expression.as_deref() {
        validate_schedule_expression(expr)?;
    }

    let state_str = input["State"].as_str().unwrap_or("ENABLED").to_string();
    if state_str != "ENABLED" && state_str != "DISABLED" {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            "State must be ENABLED or DISABLED",
        ));
    }

    let description = input["Description"].as_str().unwrap_or("").to_string();
    let managed_by = input["ManagedBy"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let arn = arn::build(ctx, "events", format!("rule/{bus_name}/{name}"));

    // AWS rejects mutations to a managed rule unless the caller is
    // the owning service. External callers (no ManagedBy on the
    // input or a mismatch) get ManagedRuleException so the simulator
    // mirrors the production failure mode.
    let (existing_targets, existing_managed_by) = bus
        .rules
        .get(name)
        .map(|r| (r.targets.clone(), r.managed_by.clone()))
        .unwrap_or_default();
    if let Some(ref owner) = existing_managed_by
        && managed_by.as_ref() != Some(owner)
    {
        return Err(AwsError::bad_request(
            "ManagedRuleException",
            format!("Rule {name} is managed by {owner}; only {owner} can modify it."),
        ));
    }

    // AWS caps an event bus at 300 rules. Beyond that, PutRule returns
    // LimitExceededException. The check skips when the rule already
    // exists (this is an update, not a new slot).
    const MAX_RULES_PER_BUS: usize = 300;
    if !bus.rules.contains_key(name) && bus.rules.len() >= MAX_RULES_PER_BUS {
        return Err(AwsError::conflict(
            "LimitExceededException",
            format!("Event bus {bus_name} already has the maximum {MAX_RULES_PER_BUS} rules."),
        ));
    }

    let rule = Rule {
        name: name.to_string(),
        arn: arn.clone(),
        event_bus_name: bus_name.to_string(),
        event_pattern,
        schedule_expression,
        state: state_str,
        description,
        targets: existing_targets,
        managed_by,
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

    let (target_count, managed_by) = bus
        .rules
        .get(name)
        .map(|r| (r.targets.len(), r.managed_by.clone()))
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Rule {name} does not exist on event bus {bus_name}"),
            )
        })?;

    // Managed rules are owned by another service principal; AWS only
    // lets that owner clean them up. Force=true bypasses the regular
    // "has targets" guard but it does not bypass managed-rule
    // ownership.
    if let Some(owner) = managed_by {
        return Err(AwsError::bad_request(
            "ManagedRuleException",
            format!("Rule {name} is managed by {owner}; only {owner} can delete it."),
        ));
    }

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
    if let Some(mb) = &rule.managed_by {
        obj["ManagedBy"] = Value::String(mb.clone());
    }

    obj
}

/// Validate a `ScheduleExpression` against AWS's accepted forms.
///
/// AWS EventBridge only honours two shapes:
///   * `rate(N unit)` where unit is `minute`, `minutes`, `hour`,
///     `hours`, `day`, or `days`. `N` is a positive integer, and
///     a value of `1` requires the singular unit.
///   * `cron(<minutes> <hours> <day-of-month> <month> <day-of-week> <year>)`
///     - the six-field cron dialect EventBridge documents.
///
/// Anything else is rejected with ValidationException at PutRule
/// time. The check is intentionally lightweight: it ensures the
/// shape is recognisable so a typo (`rate 5 minutes`, `cron(* * * * *)`)
/// fails fast instead of silently never firing.
fn validate_schedule_expression(expr: &str) -> Result<(), AwsError> {
    let trimmed = expr.trim();
    if let Some(inner) = trimmed
        .strip_prefix("rate(")
        .and_then(|s| s.strip_suffix(')'))
    {
        return validate_rate_inner(inner);
    }
    if let Some(inner) = trimmed
        .strip_prefix("cron(")
        .and_then(|s| s.strip_suffix(')'))
    {
        return validate_cron_inner(inner);
    }
    Err(AwsError::bad_request(
        "ValidationException",
        format!("ScheduleExpression '{expr}' is not a valid rate() or cron() expression."),
    ))
}

fn validate_rate_inner(inner: &str) -> Result<(), AwsError> {
    let mut parts = inner.split_whitespace();
    let value_raw = parts.next();
    let unit = parts.next();
    if value_raw.is_none() || unit.is_none() || parts.next().is_some() {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("rate({inner}) must be 'rate(N unit)'."),
        ));
    }
    let value: u64 = value_raw.unwrap().parse().map_err(|_| {
        AwsError::bad_request(
            "ValidationException",
            format!("rate({inner}) value must be a positive integer."),
        )
    })?;
    if value == 0 {
        return Err(AwsError::bad_request(
            "ValidationException",
            "rate() value must be at least 1.",
        ));
    }
    let unit = unit.unwrap();
    let singular = value == 1;
    let valid = match unit {
        "minute" | "hour" | "day" => singular,
        "minutes" | "hours" | "days" => !singular,
        _ => false,
    };
    if !valid {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "rate({inner}) unit must be minute(s), hour(s), or day(s); singular for value 1, plural otherwise."
            ),
        ));
    }
    Ok(())
}

fn validate_cron_inner(inner: &str) -> Result<(), AwsError> {
    // EventBridge cron uses six fields, not the traditional five.
    let count = inner.split_whitespace().count();
    if count != 6 {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "cron({inner}) must have exactly 6 space-separated fields (minutes hours day-of-month month day-of-week year)."
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod schedule_expression_tests {
    use super::*;

    #[test]
    fn rate_singular_unit_only_valid_for_one() {
        validate_schedule_expression("rate(1 minute)").unwrap();
        validate_schedule_expression("rate(1 hour)").unwrap();
        validate_schedule_expression("rate(1 day)").unwrap();
        assert!(validate_schedule_expression("rate(1 minutes)").is_err());
        assert!(validate_schedule_expression("rate(2 minute)").is_err());
    }

    #[test]
    fn rate_plural_unit_required_for_n_above_one() {
        validate_schedule_expression("rate(5 minutes)").unwrap();
        validate_schedule_expression("rate(12 hours)").unwrap();
        validate_schedule_expression("rate(30 days)").unwrap();
    }

    #[test]
    fn rate_rejects_zero_and_negatives() {
        assert!(validate_schedule_expression("rate(0 minutes)").is_err());
        assert!(validate_schedule_expression("rate(-1 minutes)").is_err());
    }

    #[test]
    fn rate_rejects_unknown_unit() {
        assert!(validate_schedule_expression("rate(5 seconds)").is_err());
        assert!(validate_schedule_expression("rate(2 weeks)").is_err());
    }

    #[test]
    fn cron_requires_six_fields() {
        validate_schedule_expression("cron(0 12 * * ? *)").unwrap();
        assert!(validate_schedule_expression("cron(* * * * *)").is_err());
        assert!(validate_schedule_expression("cron(0 12 * *)").is_err());
    }

    #[test]
    fn rejects_unknown_wrapper() {
        assert!(validate_schedule_expression("at(2026-01-01T00:00:00)").is_err());
        assert!(validate_schedule_expression("every 5 minutes").is_err());
        assert!(validate_schedule_expression("").is_err());
    }
}
