use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::pagination::{cap_max_results, paginate};
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
    super::schedules::validate_scheduler_name(&name, "Name")?;

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
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let max = cap_max_results(
        input.get("MaxResults").and_then(|v| v.as_i64()),
        super::schedules::LIST_DEFAULT_MAX,
        super::schedules::LIST_DEFAULT_MAX,
    );
    let next_token = input.get("NextToken").and_then(|v| v.as_str());
    let mut groups: Vec<ScheduleGroup> = state
        .schedule_groups
        .iter()
        .map(|e| e.value().clone())
        .collect();
    groups.sort_by(|a, b| a.name.cmp(&b.name));
    let page = paginate(groups, max, next_token, |g| g.name.clone())?;
    let items: Vec<Value> = page
        .items
        .iter()
        .map(|g| {
            json!({
                "Arn": g.arn,
                "Name": g.name,
                "State": g.state,
                "CreationDate": g.created_at,
                "LastModificationDate": g.created_at,
            })
        })
        .collect();
    let mut resp = json!({ "ScheduleGroups": items });
    if let Some(t) = page.next_token {
        resp["NextToken"] = json!(t);
    }
    Ok(resp)
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
