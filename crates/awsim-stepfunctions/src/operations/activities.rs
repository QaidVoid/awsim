use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{Activity, StepFunctionsState};

fn now_iso8601() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn activity_to_value(a: &Activity) -> Value {
    json!({
        "activityArn": a.arn,
        "name": a.name,
        "creationDate": a.creation_date,
    })
}

// ---------------------------------------------------------------------------
// CreateActivity
// ---------------------------------------------------------------------------

pub fn create_activity(
    state: &StepFunctionsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "name is required"))?;

    let arn = format!(
        "arn:{}:states:{}:{}:activity:{}",
        ctx.partition, ctx.region, ctx.account_id, name
    );

    if state.activities.contains_key(&arn) {
        // Return existing (idempotent per AWS behavior)
        let existing = state.activities.get(&arn).unwrap();
        return Ok(json!({
            "activityArn": existing.arn,
            "creationDate": existing.creation_date,
        }));
    }

    let tags: HashMap<String, String> = input["tags"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let k = t["key"].as_str()?;
                    let v = t["value"].as_str()?;
                    Some((k.to_string(), v.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();

    let creation_date = now_iso8601();
    let activity = Activity {
        name: name.to_string(),
        arn: arn.clone(),
        creation_date: creation_date.clone(),
        tags,
    };

    info!(name, arn = %arn, "Created activity");
    state.activities.insert(arn.clone(), activity);

    Ok(json!({
        "activityArn": arn,
        "creationDate": creation_date,
    }))
}

// ---------------------------------------------------------------------------
// DeleteActivity
// ---------------------------------------------------------------------------

pub fn delete_activity(
    state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["activityArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "activityArn is required"))?;

    state.activities.remove(arn).ok_or_else(|| {
        AwsError::not_found("ActivityDoesNotExist", format!("Activity not found: {arn}"))
    })?;

    info!(arn, "Deleted activity");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DescribeActivity
// ---------------------------------------------------------------------------

pub fn describe_activity(
    state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["activityArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "activityArn is required"))?;

    let activity = state.activities.get(arn).ok_or_else(|| {
        AwsError::not_found("ActivityDoesNotExist", format!("Activity not found: {arn}"))
    })?;

    Ok(activity_to_value(&activity))
}

// ---------------------------------------------------------------------------
// ListActivities
// ---------------------------------------------------------------------------

pub fn list_activities(
    state: &StepFunctionsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let mut activities: Vec<Value> = state
        .activities
        .iter()
        .map(|entry| activity_to_value(entry.value()))
        .collect();

    activities.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });

    Ok(json!({ "activities": activities }))
}
