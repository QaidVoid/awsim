use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{LogEvent, LogsState, now_millis};

// ---------------------------------------------------------------------------
// PutLogEvents
// ---------------------------------------------------------------------------

pub fn put_log_events(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let group_name = input["logGroupName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "logGroupName is required"))?;

    let stream_name = input["logStreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "logStreamName is required"))?;

    let log_events = input["logEvents"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "logEvents is required"))?;

    let group = state.log_groups.get(group_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {group_name}"),
        )
    })?;

    let mut stream = group.streams.get_mut(stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log stream not found: {stream_name}"),
        )
    })?;

    let ingestion_time = now_millis();
    let mut new_events: Vec<LogEvent> = Vec::with_capacity(log_events.len());

    for ev in log_events {
        let timestamp = ev["timestamp"].as_u64().ok_or_else(|| {
            AwsError::bad_request("InvalidParameterException", "each logEvent must have a timestamp")
        })?;
        let message = ev["message"].as_str().ok_or_else(|| {
            AwsError::bad_request("InvalidParameterException", "each logEvent must have a message")
        })?;

        new_events.push(LogEvent {
            timestamp,
            message: message.to_string(),
            ingestion_time,
        });
    }

    let seq_token = stream.next_sequence_token();

    // Merge and sort events by timestamp; extract metadata before releasing the write lock
    let (new_first_ts, new_last_ts) = {
        let mut events = stream.events.write().unwrap();
        events.extend(new_events.iter().cloned());
        events.sort_by_key(|e| e.timestamp);
        let first = events.first().map(|e| e.timestamp);
        let last = events.last().map(|e| e.timestamp);
        (first, last)
    };

    // Update stream metadata (outside the write-lock scope)
    if let Some(ts) = new_first_ts {
        if stream.first_event_timestamp.map_or(true, |existing| ts < existing) {
            stream.first_event_timestamp = Some(ts);
        }
    }
    if let Some(ts) = new_last_ts {
        stream.last_event_timestamp = Some(ts);
    }
    stream.last_ingestion_time = Some(ingestion_time);

    info!(
        log_group = %group_name,
        log_stream = %stream_name,
        count = new_events.len(),
        "Put log events"
    );

    Ok(json!({ "nextSequenceToken": seq_token.to_string() }))
}

// ---------------------------------------------------------------------------
// GetLogEvents
// ---------------------------------------------------------------------------

pub fn get_log_events(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let group_name = input["logGroupName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "logGroupName is required"))?;

    let stream_name = input["logStreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "logStreamName is required"))?;

    let start_time = input["startTime"].as_u64();
    let end_time = input["endTime"].as_u64();
    let start_from_head = input["startFromHead"].as_bool().unwrap_or(false);
    let limit = input["limit"].as_u64().unwrap_or(10000).min(10000) as usize;
    let next_token = input["nextToken"].as_str().unwrap_or("");

    let group = state.log_groups.get(group_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {group_name}"),
        )
    })?;

    let stream = group.streams.get(stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log stream not found: {stream_name}"),
        )
    })?;

    let events = stream.events.read().unwrap();

    // Filter by time range
    let filtered: Vec<&LogEvent> = events
        .iter()
        .filter(|e| {
            if let Some(start) = start_time {
                if e.timestamp < start {
                    return false;
                }
            }
            if let Some(end) = end_time {
                if e.timestamp > end {
                    return false;
                }
            }
            true
        })
        .collect();

    // Determine offset from nextToken
    let offset = if next_token.is_empty() {
        0usize
    } else {
        next_token.parse::<usize>().unwrap_or(0)
    };

    // Apply direction
    let page: Vec<Value> = if start_from_head {
        filtered
            .iter()
            .skip(offset)
            .take(limit)
            .map(|e| event_to_json(e))
            .collect()
    } else {
        // From tail: default direction for GetLogEvents without startFromHead
        let total = filtered.len();
        let start = total.saturating_sub(offset + limit);
        let end = total.saturating_sub(offset);
        filtered[start..end]
            .iter()
            .map(|e| event_to_json(e))
            .collect()
    };

    let next_forward_offset = offset + page.len();
    let next_backward_offset = if offset == 0 { 0 } else { offset.saturating_sub(limit) };

    Ok(json!({
        "events": page,
        "nextForwardToken": format!("f/{next_forward_offset}"),
        "nextBackwardToken": format!("b/{next_backward_offset}"),
    }))
}

// ---------------------------------------------------------------------------
// FilterLogEvents
// ---------------------------------------------------------------------------

pub fn filter_log_events(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let group_name = input["logGroupName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "logGroupName is required"))?;

    let stream_names: Option<Vec<&str>> = input["logStreamNames"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect());

    let filter_pattern = input["filterPattern"].as_str().unwrap_or("");
    let start_time = input["startTime"].as_u64();
    let end_time = input["endTime"].as_u64();
    let limit = input["limit"].as_u64().unwrap_or(10000).min(10000) as usize;

    let group = state.log_groups.get(group_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {group_name}"),
        )
    })?;

    let mut matched_events: Vec<Value> = Vec::new();
    let mut searched_streams: Vec<Value> = Vec::new();

    for stream_entry in group.streams.iter() {
        let sname = stream_entry.key().as_str();

        // Filter by logStreamNames if provided
        if let Some(ref names) = stream_names {
            if !names.contains(&sname) {
                continue;
            }
        }

        let stream = stream_entry.value();
        let events = stream.events.read().unwrap();

        let searched = true;
        let mut has_match = false;

        let stream_events: Vec<Value> = events
            .iter()
            .filter(|e| {
                if let Some(start) = start_time {
                    if e.timestamp < start {
                        return false;
                    }
                }
                if let Some(end) = end_time {
                    if e.timestamp > end {
                        return false;
                    }
                }
                // Simple substring match for filter pattern
                if filter_pattern.is_empty() || e.message.contains(filter_pattern) {
                    has_match = true;
                    return true;
                }
                false
            })
            .map(|e| {
                let mut obj = event_to_json(e);
                obj["logStreamName"] = json!(sname);
                obj["eventId"] = json!(format!("{}-{}", sname, e.timestamp));
                obj
            })
            .collect();

        matched_events.extend(stream_events);
        searched_streams.push(json!({
            "logStreamName": sname,
            "searchedCompletely": searched,
        }));
        let _ = has_match;
        let _ = searched;
    }

    // Sort by timestamp for stable output
    matched_events.sort_by_key(|e| e["timestamp"].as_u64().unwrap_or(0));
    matched_events.truncate(limit);

    Ok(json!({
        "events": matched_events,
        "searchedLogStreams": searched_streams,
    }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn event_to_json(e: &LogEvent) -> Value {
    json!({
        "timestamp": e.timestamp,
        "message": e.message,
        "ingestionTime": e.ingestion_time,
    })
}
