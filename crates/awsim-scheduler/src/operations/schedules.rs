use std::time::{SystemTime, UNIX_EPOCH};

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

    let group_name = input["GroupName"].as_str().unwrap_or("default").to_string();

    let schedule_state = input["State"].as_str().unwrap_or("ENABLED").to_string();
    let flexible_time_window = input["FlexibleTimeWindow"].clone();
    let flexible_time_window = if flexible_time_window.is_null() {
        json!({ "Mode": "OFF" })
    } else {
        flexible_time_window
    };

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
    };

    state.schedules.insert(key, schedule);

    Ok(json!({ "ScheduleArn": arn }))
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
    if !input["Target"].is_null() {
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
