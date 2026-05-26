use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::state::{LogStream, LogsState};

// ---------------------------------------------------------------------------
// CreateLogStream
// ---------------------------------------------------------------------------

pub fn create_log_stream(
    state: &LogsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let group_name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    let stream_name = input["logStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logStreamName is required")
    })?;

    let group = state.log_groups.get(group_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {group_name}"),
        )
    })?;

    if group.streams.contains_key(stream_name) {
        return Err(AwsError::conflict(
            "ResourceAlreadyExistsException",
            format!("Log stream already exists: {stream_name}"),
        ));
    }

    let arn = format!(
        "arn:aws:logs:{}:{}:log-group:{}:log-stream:{}",
        ctx.region, ctx.account_id, group_name, stream_name
    );

    let stream = LogStream::new(stream_name.to_string(), arn);
    info!(log_group = %group_name, log_stream = %stream_name, "Created log stream");
    group.streams.insert(stream_name.to_string(), stream);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DeleteLogStream
// ---------------------------------------------------------------------------

pub fn delete_log_stream(
    state: &LogsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let group_name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    let stream_name = input["logStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logStreamName is required")
    })?;

    let group = state.log_groups.get(group_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {group_name}"),
        )
    })?;

    group.streams.remove(stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log stream not found: {stream_name}"),
        )
    })?;

    if let Some(sqlite) = state.sqlite()
        && let Err(e) = sqlite.delete_stream(&ctx.account_id, &ctx.region, group_name, stream_name)
    {
        warn!(
            log_group = %group_name,
            log_stream = %stream_name,
            error = %e.message,
            "Failed to remove persisted log stream events"
        );
    }

    info!(log_group = %group_name, log_stream = %stream_name, "Deleted log stream");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DescribeLogStreams
// ---------------------------------------------------------------------------

pub fn describe_log_streams(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let group_name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    let group = state.log_groups.get(group_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {group_name}"),
        )
    })?;

    let prefix = input["logStreamNamePrefix"].as_str().unwrap_or("");
    let order_by = input["orderBy"].as_str().unwrap_or("LogStreamName");
    let descending = input["descending"].as_bool().unwrap_or(false);
    let limit = input["limit"].as_u64().unwrap_or(50).min(50) as usize;
    let next_token = input["nextToken"].as_str().unwrap_or("");

    // orderBy must be either LogStreamName or LastEventTime. AWS docs
    // also state that logStreamNamePrefix cannot be combined with
    // orderBy=LastEventTime — the sort key isn't a prefix-friendly
    // field, so both being set is rejected with InvalidParameterException.
    if !matches!(order_by, "LogStreamName" | "LastEventTime") {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!("orderBy `{order_by}` must be LogStreamName or LastEventTime."),
        ));
    }
    if order_by == "LastEventTime" && !prefix.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "logStreamNamePrefix cannot be specified with orderBy=LastEventTime.",
        ));
    }

    let mut streams: Vec<Value> = group
        .streams
        .iter()
        .filter(|e| e.key().starts_with(prefix))
        .map(|e| {
            let s = e.value();
            let mut obj = json!({
                "logStreamName": s.name,
                "arn": s.arn,
                "creationTime": s.creation_time,
                "uploadSequenceToken": s.upload_sequence_token.load(std::sync::atomic::Ordering::SeqCst).to_string(),
                "storedBytes": 0u64,
            });
            if let Some(t) = s.first_event_timestamp {
                obj["firstEventTimestamp"] = json!(t);
            }
            if let Some(t) = s.last_event_timestamp {
                obj["lastEventTimestamp"] = json!(t);
            }
            if let Some(t) = s.last_ingestion_time {
                obj["lastIngestionTime"] = json!(t);
            }
            obj
        })
        .collect();

    // Sort
    match order_by {
        "LastEventTime" => {
            streams.sort_by(|a, b| {
                let ta = a["lastEventTimestamp"].as_u64().unwrap_or(0);
                let tb = b["lastEventTimestamp"].as_u64().unwrap_or(0);
                ta.cmp(&tb)
            });
        }
        _ => {
            // LogStreamName
            streams.sort_by(|a, b| {
                a["logStreamName"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["logStreamName"].as_str().unwrap_or(""))
            });
        }
    }

    if descending {
        streams.reverse();
    }

    // Pagination
    let start = if next_token.is_empty() {
        0
    } else {
        streams
            .iter()
            .position(|s| s["logStreamName"].as_str().unwrap_or("") > next_token)
            .unwrap_or(streams.len())
    };

    let page = &streams[start..];
    let page: Vec<Value> = page.iter().take(limit).cloned().collect();
    let new_next_token = if start + limit < streams.len() {
        page.last()
            .and_then(|s| s["logStreamName"].as_str())
            .map(|s| s.to_string())
    } else {
        None
    };

    let mut result = json!({ "logStreams": page });
    if let Some(token) = new_next_token {
        result["nextToken"] = json!(token);
    }

    Ok(result)
}

#[cfg(test)]
mod order_by_tests {
    use super::*;
    use crate::state::LogsState;

    fn ctx() -> RequestContext {
        RequestContext::new("logs", "us-east-1")
    }

    fn setup(state: &LogsState, group: &str) {
        crate::operations::log_groups::create_log_group(
            state,
            &json!({ "logGroupName": group }),
            &ctx(),
        )
        .unwrap();
        create_log_stream(
            state,
            &json!({ "logGroupName": group, "logStreamName": "s1" }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn rejects_unknown_order_by() {
        let state = LogsState::default();
        setup(&state, "g");
        let err = describe_log_streams(
            &state,
            &json!({ "logGroupName": "g", "orderBy": "Bogus" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn rejects_prefix_with_order_by_last_event_time() {
        let state = LogsState::default();
        setup(&state, "g");
        let err = describe_log_streams(
            &state,
            &json!({
                "logGroupName": "g",
                "orderBy": "LastEventTime",
                "logStreamNamePrefix": "s",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
        assert!(err.message.contains("logStreamNamePrefix"));
    }

    #[test]
    fn accepts_order_by_last_event_time_without_prefix() {
        let state = LogsState::default();
        setup(&state, "g");
        describe_log_streams(
            &state,
            &json!({ "logGroupName": "g", "orderBy": "LastEventTime" }),
            &ctx(),
        )
        .unwrap();
    }
}
