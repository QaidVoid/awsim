use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::idempotency::{Lookup, hash_request, validate_token};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{Schedule, SchedulerState};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// CreateSchedule
// ---------------------------------------------------------------------------

pub fn create_schedule(
    state: &SchedulerState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // ClientToken idempotency: a same-token replay short-circuits to
    // the cached `{ScheduleArn}` so callers (CloudFormation, Terraform)
    // can safely retry create requests on transient network errors
    // without minting a second schedule. Mismatched bodies surface
    // IdempotencyParameterMismatchException per AWS docs.
    let client_token = input
        .get("ClientToken")
        .and_then(|v| v.as_str())
        .map(String::from);
    if let Some(ref token) = client_token {
        validate_token(token)?;
        let req_hash = hash_request(&canonical_create_schedule_body(input));
        match state.client_token_cache.lookup(token, req_hash) {
            Lookup::Hit(v) => return Ok(v),
            Lookup::Mismatch => {
                return Err(AwsError::bad_request(
                    "IdempotencyParameterMismatchException",
                    "Request parameters do not match those used in a prior CreateSchedule call \
                     with the same ClientToken.",
                ));
            }
            Lookup::Miss => {}
        }
    }

    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Name is required"))?
        .to_string();

    let schedule_expression = input["ScheduleExpression"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("ValidationException", "ScheduleExpression is required")
        })?
        .to_string();

    let target = input["Target"].clone();
    if target.is_null() {
        return Err(AwsError::bad_request(
            "ValidationException",
            "Target is required",
        ));
    }
    validate_target_arn(&target)?;

    let group_name = input["GroupName"].as_str().unwrap_or("default").to_string();

    let schedule_state = input["State"].as_str().unwrap_or("ENABLED").to_string();
    let flexible_time_window = input["FlexibleTimeWindow"].clone();
    let flexible_time_window = if flexible_time_window.is_null() {
        json!({ "Mode": "OFF" })
    } else {
        flexible_time_window
    };
    let schedule_expression_timezone = resolve_timezone(input)?;

    let key = format!("{group_name}/{name}");
    if state.schedules.contains_key(&key) {
        return Err(AwsError::conflict(
            "ConflictException",
            format!("Schedule '{name}' already exists in group '{group_name}'"),
        ));
    }

    let arn = format!(
        "arn:aws:scheduler:{}:{}:schedule/{}/{}",
        ctx.region, ctx.account_id, group_name, name
    );

    let now = now_secs();
    let schedule = Schedule {
        name: name.clone(),
        group_name: group_name.clone(),
        arn: arn.clone(),
        schedule_expression,
        target,
        flexible_time_window,
        state: schedule_state,
        created_at: now,
        last_modified_at: now,
        schedule_expression_timezone,
    };

    state.schedules.insert(key, schedule);

    let result = json!({ "ScheduleArn": arn });
    if let Some(token) = client_token {
        let req_hash = hash_request(&canonical_create_schedule_body(input));
        state
            .client_token_cache
            .insert(token, req_hash, result.clone());
    }
    Ok(result)
}

/// Resolve `ScheduleExpressionTimezone` from the request, falling
/// back to UTC when the field is absent. The validator accepts the
/// IANA `Continent/City` form, the `UTC` / `GMT` shorthand, and the
/// `Etc/*` aliases. AWS rejects anything else with
/// `ValidationException`.
fn resolve_timezone(input: &Value) -> Result<String, AwsError> {
    let tz = match input
        .get("ScheduleExpressionTimezone")
        .and_then(|v| v.as_str())
    {
        Some(t) => t.trim(),
        None => return Ok("UTC".to_string()),
    };
    if tz.is_empty() {
        return Ok("UTC".to_string());
    }
    // Tolerate `UTC` / `GMT` shorthand and the `Etc/*` family that
    // share a flat single-component form.
    if matches!(tz, "UTC" | "GMT") {
        return Ok(tz.to_string());
    }
    // IANA shape: one or more `<segment>/<segment>` blocks where
    // each segment is `[A-Za-z][A-Za-z0-9_+-]*`. We accept any
    // string matching that pattern and don't require a curated
    // catalogue: AWSim isn't a real cron daemon, so the timezone is
    // metadata the runner could consume later but isn't actually
    // honoured at fire time.
    let parts: Vec<&str> = tz.split('/').collect();
    if parts.len() < 2 || parts.iter().any(|p| p.is_empty()) {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "ScheduleExpressionTimezone `{tz}` must be an IANA `Region/City` \
                 identifier (or `UTC`/`GMT`)."
            ),
        ));
    }
    // IANA tzdata entries capitalise each segment: `America/New_York`,
    // `Etc/UTC`, `America/Argentina/Buenos_Aires`. Rejecting
    // lowercase-first segments catches the most common typos.
    let segment_ok = |s: &&str| -> bool {
        let mut chars = s.chars();
        let first = match chars.next() {
            Some(c) => c,
            None => return false,
        };
        if !first.is_ascii_uppercase() {
            return false;
        }
        chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-'))
    };
    if !parts.iter().all(segment_ok) {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "ScheduleExpressionTimezone `{tz}` segments must match \
                 `[A-Za-z][A-Za-z0-9_+-]*`."
            ),
        ));
    }
    Ok(tz.to_string())
}

/// Validate the `Target.Arn` shape. AWS Scheduler accepts two
/// flavours:
///
/// 1. **Templated service target** — a normal AWS resource ARN
///    (`arn:aws:<service>:<region>:<account>:<resource>`). We
///    sanity-check the prefix and segment count but don't enforce a
///    service catalog.
/// 2. **Universal target** — `arn:aws:scheduler:::aws-sdk:<service>:<action>`.
///    The middle region/account/resource-type segments are empty by
///    convention and `<service>:<action>` is required to be a non-empty
///    lowercase-letter pair. AWS rejects invalid `service:action`
///    combinations with `ValidationException`.
fn validate_target_arn(target: &Value) -> Result<(), AwsError> {
    let arn = target
        .get("Arn")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Target.Arn is required"))?;
    if !arn.starts_with("arn:") {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("Target.Arn `{arn}` must start with `arn:`."),
        ));
    }
    // Universal target: arn:aws:scheduler:::aws-sdk:<service>:<action>
    if let Some(tail) = arn.strip_prefix("arn:aws:scheduler:::") {
        let parts: Vec<&str> = tail.splitn(3, ':').collect();
        if parts.len() != 3 || parts[0] != "aws-sdk" {
            return Err(AwsError::bad_request(
                "ValidationException",
                format!(
                    "Universal target `{arn}` must be \
                     `arn:aws:scheduler:::aws-sdk:<service>:<action>`."
                ),
            ));
        }
        let service = parts[1];
        let action = parts[2];
        let service_ok = !service.is_empty()
            && service
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !service_ok {
            return Err(AwsError::bad_request(
                "ValidationException",
                format!(
                    "Universal target `{arn}` has an invalid service `{service}`; \
                     must be lowercase letters / digits / `-`."
                ),
            ));
        }
        // Actions are camelCase: at least one letter, no whitespace.
        let action_ok = !action.is_empty()
            && action.chars().all(|c| c.is_ascii_alphanumeric())
            && action
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic());
        if !action_ok {
            return Err(AwsError::bad_request(
                "ValidationException",
                format!(
                    "Universal target `{arn}` has an invalid action `{action}`; \
                     must be camelCase alphanumerics starting with a letter."
                ),
            ));
        }
        return Ok(());
    }
    // Templated target: standard 6-segment ARN.
    let segs: Vec<&str> = arn.split(':').collect();
    if segs.len() < 6 || segs[0] != "arn" || segs[1].is_empty() || segs[2].is_empty() {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "Target.Arn `{arn}` must have the AWS-standard shape \
                 `arn:<partition>:<service>:<region>:<account>:<resource>`."
            ),
        ));
    }
    Ok(())
}

/// Hashable canonical representation of `CreateSchedule`'s body for
/// the `ClientToken` cache. AWS compares the *request parameters*, so
/// we strip the `ClientToken` itself before sorting object keys so
/// that the same logical body always hashes the same.
fn canonical_create_schedule_body(input: &Value) -> Value {
    let mut clone = input.clone();
    if let Some(obj) = clone.as_object_mut() {
        obj.remove("ClientToken");
    }
    fn canonicalise(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut sorted: std::collections::BTreeMap<&str, Value> =
                    std::collections::BTreeMap::new();
                for (k, v) in map {
                    sorted.insert(k.as_str(), canonicalise(v));
                }
                let mut out = serde_json::Map::new();
                for (k, v) in sorted {
                    out.insert(k.to_string(), v);
                }
                Value::Object(out)
            }
            Value::Array(arr) => Value::Array(arr.iter().map(canonicalise).collect()),
            other => other.clone(),
        }
    }
    canonicalise(&clone)
}

// ---------------------------------------------------------------------------
// GetSchedule
// ---------------------------------------------------------------------------

pub fn get_schedule(
    state: &SchedulerState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Name is required"))?;

    let group_name = input["GroupName"].as_str().unwrap_or("default");

    let key = format!("{group_name}/{name}");
    let schedule = state.schedules.get(&key).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Schedule not found: {name}"),
        )
    })?;

    Ok(json!({
        "Arn": schedule.arn,
        "Name": schedule.name,
        "GroupName": schedule.group_name,
        "ScheduleExpression": schedule.schedule_expression,
        "ScheduleExpressionTimezone": schedule.schedule_expression_timezone,
        "Target": schedule.target,
        "FlexibleTimeWindow": schedule.flexible_time_window,
        "State": schedule.state,
        "CreationDate": schedule.created_at,
        "LastModificationDate": schedule.last_modified_at,
    }))
}

// ---------------------------------------------------------------------------
// ListSchedules
// ---------------------------------------------------------------------------

pub fn list_schedules(
    state: &SchedulerState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let group_filter = input["GroupName"].as_str();

    let list: Vec<Value> = state
        .schedules
        .iter()
        .filter(|e| {
            if let Some(g) = group_filter {
                e.value().group_name == g
            } else {
                true
            }
        })
        .map(|e| {
            let s = e.value();
            json!({
                "Arn": s.arn,
                "Name": s.name,
                "GroupName": s.group_name,
                "ScheduleExpression": s.schedule_expression,
                "State": s.state,
                "CreationDate": s.created_at,
                "LastModificationDate": s.last_modified_at,
                "Target": {
                    "Arn": s.target["Arn"],
                },
            })
        })
        .collect();

    Ok(json!({ "Schedules": list }))
}

// ---------------------------------------------------------------------------
// DeleteSchedule
// ---------------------------------------------------------------------------

pub fn delete_schedule(
    state: &SchedulerState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Name is required"))?;

    let group_name = input["GroupName"].as_str().unwrap_or("default");

    let key = format!("{group_name}/{name}");
    if state.schedules.remove(&key).is_none() {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Schedule not found: {name}"),
        ));
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UpdateSchedule
// ---------------------------------------------------------------------------

pub fn update_schedule(
    state: &SchedulerState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Name is required"))?;

    let group_name = input["GroupName"].as_str().unwrap_or("default");

    let key = format!("{group_name}/{name}");
    let mut schedule = state.schedules.get_mut(&key).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Schedule not found: {name}"),
        )
    })?;

    if let Some(expr) = input["ScheduleExpression"].as_str() {
        schedule.schedule_expression = expr.to_string();
    }
    if input.get("ScheduleExpressionTimezone").is_some() {
        // Re-resolve on every update so a malformed value rejects
        // before we touch the live record.
        schedule.schedule_expression_timezone = resolve_timezone(input)?;
    }
    if !input["Target"].is_null() {
        validate_target_arn(&input["Target"])?;
        schedule.target = input["Target"].clone();
    }
    if let Some(s) = input["State"].as_str() {
        schedule.state = s.to_string();
    }
    if !input["FlexibleTimeWindow"].is_null() {
        schedule.flexible_time_window = input["FlexibleTimeWindow"].clone();
    }
    schedule.last_modified_at = now_secs();

    let arn = schedule.arn.clone();

    Ok(json!({ "ScheduleArn": arn }))
}
