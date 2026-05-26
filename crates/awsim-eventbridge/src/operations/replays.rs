use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{EventBridgeState, Replay};
use crate::util::now_iso8601;

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

    // EventSourceArn must reference an existing archive in the account.
    // AWS rejects unknown sources with ResourceNotFoundException.
    let archive_name = event_source_arn
        .rsplit('/')
        .next()
        .unwrap_or(event_source_arn);
    let archive_event_pattern = state
        .archives
        .get(archive_name)
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Archive `{archive_name}` referenced by EventSourceArn does not exist"),
            )
        })?
        .event_pattern
        .clone();

    // Destination.Arn must point to an existing event bus (replays can
    // only target the same account's buses today).
    let destination = input["Destination"].clone();
    let dest_arn = destination
        .get("Arn")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Destination.Arn is required"))?;
    let bus_name = dest_arn.rsplit('/').next().unwrap_or(dest_arn);
    if !state.event_buses.contains_key(bus_name) {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Destination bus `{bus_name}` does not exist"),
        ));
    }

    // FilterArns optionally restrict replay to specific rules on the
    // destination bus. Validate that each named arn resolves to a known
    // rule.
    if let Some(filter_arns) = destination.get("FilterArns").and_then(Value::as_array) {
        let bus = state.event_buses.get(bus_name).unwrap();
        for fa in filter_arns {
            let arn = fa.as_str().unwrap_or("");
            let rule_name = arn.rsplit('/').next().unwrap_or(arn);
            if !bus.rules.contains_key(rule_name) {
                return Err(AwsError::not_found(
                    "ResourceNotFoundException",
                    format!(
                        "Destination.FilterArns entry `{arn}` does not match any rule on bus `{bus_name}`"
                    ),
                ));
            }
        }
    }

    // EventStartTime must be strictly before EventEndTime.
    let event_start_time = input["EventStartTime"].as_str().unwrap_or("");
    let event_end_time = input["EventEndTime"].as_str().unwrap_or("");
    if !event_start_time.is_empty()
        && !event_end_time.is_empty()
        && event_start_time >= event_end_time
    {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            "EventStartTime must be strictly before EventEndTime.",
        ));
    }

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
        destination,
        event_start_time: event_start_time.to_string(),
        event_end_time: event_end_time.to_string(),
        // The archive's event_pattern is captured in state_reason so
        // callers can see which filter the replay would apply at delivery.
        state: "COMPLETED".to_string(),
        state_reason: archive_event_pattern
            .as_ref()
            .map(|p| format!("Filtered by archive EventPattern: {p}")),
        replay_start_time: Some(now.clone()),
        replay_end_time: Some(now),
    };

    state.replays.insert(name.to_string(), replay);

    Ok(json!({
        "ReplayArn": arn,
        "State": "COMPLETED",
        "StateReason": state.replays.get(name).and_then(|r| r.state_reason.clone()),
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

#[cfg(test)]
mod start_replay_tests {
    use super::*;
    use crate::state::{Archive, EventBus};

    fn ctx() -> RequestContext {
        RequestContext::new("events", "us-east-1")
    }

    fn seed_state() -> EventBridgeState {
        let state = EventBridgeState::default();
        state.archives.insert(
            "ar1".to_string(),
            Archive {
                name: "ar1".to_string(),
                arn: "arn:aws:events:us-east-1:000000000000:archive/ar1".to_string(),
                event_source_arn: "arn:aws:events:us-east-1:000000000000:event-bus/default"
                    .to_string(),
                description: String::new(),
                event_pattern: Some(r#"{"source":["my.app"]}"#.to_string()),
                retention_days: 0,
                state: "ENABLED".to_string(),
                creation_time: String::new(),
            },
        );
        state.event_buses.insert(
            "default".to_string(),
            EventBus::new(
                "default".to_string(),
                "arn:aws:events:us-east-1:000000000000:event-bus/default".to_string(),
            ),
        );
        state
    }

    #[test]
    fn rejects_unknown_archive_source() {
        let state = seed_state();
        let err = start_replay(
            &state,
            &json!({
                "ReplayName": "r1",
                "EventSourceArn": "arn:aws:events:us-east-1:000000000000:archive/missing",
                "Destination": { "Arn": "arn:aws:events:us-east-1:000000000000:event-bus/default" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
        assert!(err.message.contains("Archive"));
    }

    #[test]
    fn rejects_destination_bus_that_does_not_exist() {
        let state = seed_state();
        let err = start_replay(
            &state,
            &json!({
                "ReplayName": "r2",
                "EventSourceArn": "arn:aws:events:us-east-1:000000000000:archive/ar1",
                "Destination": { "Arn": "arn:aws:events:us-east-1:000000000000:event-bus/missing" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
        assert!(err.message.contains("missing"));
    }

    #[test]
    fn rejects_event_start_after_end() {
        let state = seed_state();
        let err = start_replay(
            &state,
            &json!({
                "ReplayName": "r3",
                "EventSourceArn": "arn:aws:events:us-east-1:000000000000:archive/ar1",
                "Destination": { "Arn": "arn:aws:events:us-east-1:000000000000:event-bus/default" },
                "EventStartTime": "2026-01-02T00:00:00Z",
                "EventEndTime": "2026-01-01T00:00:00Z",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
    }

    #[test]
    fn includes_archive_pattern_in_state_reason() {
        let state = seed_state();
        let resp = start_replay(
            &state,
            &json!({
                "ReplayName": "r4",
                "EventSourceArn": "arn:aws:events:us-east-1:000000000000:archive/ar1",
                "Destination": { "Arn": "arn:aws:events:us-east-1:000000000000:event-bus/default" },
            }),
            &ctx(),
        )
        .unwrap();
        let reason = resp["StateReason"].as_str().expect("state reason captured");
        assert!(reason.contains("my.app"));
    }
}
