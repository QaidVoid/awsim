use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{EventBridgeState, Replay};

fn now_iso8601() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn replay_to_value(r: &Replay) -> Value {
    json!({
        "ReplayName": r.name,
        "ReplayArn": r.arn,
        "Description": r.description,
        "EventSourceArn": r.event_source_arn,
        "Destination": r.destination,
        "EventStartTime": r.event_start_time,
        "EventEndTime": r.event_end_time,
        "State": r.state,
        "StateReason": r.state_reason,
        "ReplayStartTime": r.replay_start_time,
        "ReplayEndTime": r.replay_end_time,
    })
}

// ---------------------------------------------------------------------------
// StartReplay
// ---------------------------------------------------------------------------

pub fn start_replay(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ReplayName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ReplayName is required"))?;

    if state.replays.contains_key(name) {
        return Err(AwsError::conflict(
            "ResourceAlreadyExistsException",
            format!("Replay {name} already exists"),
        ));
    }

    let event_source_arn = input["EventSourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EventSourceArn is required"))?;

    let arn = format!(
        "arn:aws:events:{}:{}:replay/{}",
        ctx.region, ctx.account_id, name
    );

    let now = now_iso8601();
    let replay = Replay {
        name: name.to_string(),
        arn: arn.clone(),
        description: input["Description"].as_str().unwrap_or("").to_string(),
        event_source_arn: event_source_arn.to_string(),
        destination: input["Destination"].clone(),
        event_start_time: input["EventStartTime"].as_str().unwrap_or("").to_string(),
        event_end_time: input["EventEndTime"].as_str().unwrap_or("").to_string(),
        state: "COMPLETED".to_string(),
        state_reason: None,
        replay_start_time: Some(now.clone()),
        replay_end_time: Some(now),
    };

    state.replays.insert(name.to_string(), replay);

    Ok(json!({
        "ReplayArn": arn,
        "State": "COMPLETED",
        "StateReason": null,
        "ReplayStartTime": state.replays.get(name).map(|r| r.replay_start_time.clone()).unwrap_or_default(),
    }))
}

// ---------------------------------------------------------------------------
// CancelReplay
// ---------------------------------------------------------------------------

pub fn cancel_replay(
    state: &EventBridgeState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ReplayName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ReplayName is required"))?;

    let mut replay = state.replays.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Replay {name} does not exist"),
        )
    })?;

    replay.state = "CANCELLED".to_string();
    replay.state_reason = Some("Cancelled by user".to_string());

    Ok(json!({
        "ReplayArn": replay.arn,
        "State": "CANCELLED",
        "StateReason": "Cancelled by user",
    }))
}

// ---------------------------------------------------------------------------
// DescribeReplay
// ---------------------------------------------------------------------------

pub fn describe_replay(
    state: &EventBridgeState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ReplayName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ReplayName is required"))?;

    let replay = state.replays.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Replay {name} does not exist"),
        )
    })?;

    Ok(replay_to_value(&replay))
}

// ---------------------------------------------------------------------------
// ListReplays
// ---------------------------------------------------------------------------

pub fn list_replays(
    state: &EventBridgeState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let replays: Vec<Value> = state
        .replays
        .iter()
        .map(|entry| replay_to_value(entry.value()))
        .collect();

    Ok(json!({ "Replays": replays }))
}
