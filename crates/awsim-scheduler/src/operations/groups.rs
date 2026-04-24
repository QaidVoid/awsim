use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{ScheduleGroup, SchedulerState};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// CreateScheduleGroup
// ---------------------------------------------------------------------------

pub fn create_schedule_group(
    state: &SchedulerState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Name is required"))?
        .to_string();

    if state.schedule_groups.contains_key(&name) {
        return Err(AwsError::conflict(
            "ConflictException",
            format!("Schedule group '{name}' already exists"),
        ));
    }

    let arn = format!(
        "arn:aws:scheduler:{}:{}:schedule-group/{}",
        ctx.region, ctx.account_id, name
    );

    let group = ScheduleGroup {
        name: name.clone(),
        arn: arn.clone(),
        state: "ACTIVE".to_string(),
        created_at: now_secs(),
    };

    state.schedule_groups.insert(name, group);

    Ok(json!({ "ScheduleGroupArn": arn }))
}

// ---------------------------------------------------------------------------
// GetScheduleGroup
// ---------------------------------------------------------------------------

pub fn get_schedule_group(
    state: &SchedulerState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Name is required"))?;

    let group = state.schedule_groups.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Schedule group not found: {name}"),
        )
    })?;

    Ok(json!({
        "Arn": group.arn,
        "Name": group.name,
        "State": group.state,
        "CreationDate": group.created_at,
        "LastModificationDate": group.created_at,
    }))
}

// ---------------------------------------------------------------------------
// ListScheduleGroups
// ---------------------------------------------------------------------------

pub fn list_schedule_groups(
    state: &SchedulerState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list: Vec<Value> = state
        .schedule_groups
        .iter()
        .map(|e| {
            let g = e.value();
            json!({
                "Arn": g.arn,
                "Name": g.name,
                "State": g.state,
                "CreationDate": g.created_at,
                "LastModificationDate": g.created_at,
            })
        })
        .collect();

    Ok(json!({ "ScheduleGroups": list }))
}

// ---------------------------------------------------------------------------
// DeleteScheduleGroup
// ---------------------------------------------------------------------------

pub fn delete_schedule_group(
    state: &SchedulerState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Name is required"))?;

    if name == "default" {
        return Err(AwsError::bad_request(
            "ValidationException",
            "Cannot delete the default schedule group",
        ));
    }

    if state.schedule_groups.remove(name).is_none() {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Schedule group not found: {name}"),
        ));
    }

    // Also delete all schedules in this group
    state.schedules.retain(|_, s| s.group_name != name);

    Ok(json!({}))
}
