use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::sqlite_store::LogEventRow;
use crate::state::{LogsState, now_millis};

// ---------------------------------------------------------------------------
// PutLogEvents
// ---------------------------------------------------------------------------

pub fn put_log_events(
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

    let log_events = input["logEvents"].as_array().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logEvents is required")
    })?;

    // AWS documents a per-request cap of 10000 log events and rejects
    // anything larger with InvalidParameterException at the API
    // boundary. Beyond that, batched ingestion silently drops events
    // here which masks the failure clients would see in prod.
    const MAX_EVENTS_PER_REQUEST: usize = 10_000;
    if log_events.len() > MAX_EVENTS_PER_REQUEST {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!(
                "logEvents contains {} entries; the maximum allowed per PutLogEvents request is {MAX_EVENTS_PER_REQUEST}.",
                log_events.len()
            ),
        ));
    }

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

    // AWS validates `sequenceToken` strictly when the caller supplies
    // one: a mismatch returns InvalidSequenceTokenException carrying
    // the expected token. Modern SDKs (>= 2021) omit the field and the
    // server-side enforcement is skipped — match that behaviour by
    // only validating when the caller passed something non-empty.
    let supplied_token = input["sequenceToken"].as_str().filter(|s| !s.is_empty());
    if let Some(token) = supplied_token {
        let expected = stream
            .upload_sequence_token
            .load(std::sync::atomic::Ordering::SeqCst)
            .to_string();
        if token != expected {
            let mut err = AwsError::bad_request(
                "InvalidSequenceTokenException",
                format!(
                    "The given sequenceToken `{token}` is invalid. The next expected token is `{expected}`."
                ),
            );
            let mut extras = serde_json::Map::new();
            extras.insert("expectedSequenceToken".to_string(), Value::String(expected));
            err.extras = Some(Box::new(extras));
            return Err(err);
        }
    }

    let ingestion_time = now_millis();
    // AWS rejects events whose timestamp falls outside the documented
    // ingestion window: older than 14 days or more than 2 hours in the
    // future. Rejected entries are reported via rejectedLogEventsInfo
    // and are not persisted. Indices here match the original input
    // order so callers can correlate to their request.
    const PAST_WINDOW_MS: u64 = 14 * 24 * 60 * 60 * 1000;
    const FUTURE_WINDOW_MS: u64 = 2 * 60 * 60 * 1000;
    let oldest_allowed = ingestion_time.saturating_sub(PAST_WINDOW_MS);
    let newest_allowed = ingestion_time + FUTURE_WINDOW_MS;

    let mut new_events: Vec<LogEventRow> = Vec::with_capacity(log_events.len());
    let mut min_ts = u64::MAX;
    let mut max_ts = 0u64;
    let mut too_old_end_idx: Option<usize> = None;
    let mut too_new_start_idx: Option<usize> = None;

    for (idx, ev) in log_events.iter().enumerate() {
        let timestamp = ev["timestamp"].as_u64().ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterException",
                "each logEvent must have a timestamp",
            )
        })?;
        let message = ev["message"].as_str().ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterException",
                "each logEvent must have a message",
            )
        })?;
        if timestamp < oldest_allowed {
            too_old_end_idx = Some(idx);
            continue;
        }
        if timestamp > newest_allowed {
            if too_new_start_idx.is_none() {
                too_new_start_idx = Some(idx);
            }
            continue;
        }
        if timestamp < min_ts {
            min_ts = timestamp;
        }
        if timestamp > max_ts {
            max_ts = timestamp;
        }
        new_events.push(LogEventRow {
            timestamp,
            message: message.to_string(),
            ingestion_time,
        });
    }

    let seq_token = stream.next_sequence_token();

    let sqlite = state.sqlite().ok_or_else(|| {
        AwsError::internal("CloudWatch Logs sqlite store not initialised".to_string())
    })?;
    sqlite.put_events(
        &ctx.account_id,
        &ctx.region,
        group_name,
        stream_name,
        &new_events,
    )?;

    // Enforce retention immediately after writes, so a chatty workload
    // doesn't accumulate events past `retentionInDays` between sweeps.
    if let Some(days) = group.retention_in_days
        && days > 0
    {
        let cutoff = ingestion_time.saturating_sub((days as u64) * 86_400_000);
        let _ = sqlite.trim_older_than(&ctx.account_id, &ctx.region, group_name, cutoff);
    }

    if !new_events.is_empty() {
        if stream.first_event_timestamp.is_none_or(|ex| min_ts < ex) {
            stream.first_event_timestamp = Some(min_ts);
        }
        if stream.last_event_timestamp.is_none_or(|ex| max_ts > ex) {
            stream.last_event_timestamp = Some(max_ts);
        }
        stream.last_ingestion_time = Some(ingestion_time);
    }

    info!(
        log_group = %group_name,
        log_stream = %stream_name,
        count = new_events.len(),
        "Put log events"
    );

    let mut response = json!({ "nextSequenceToken": seq_token.to_string() });
    if too_old_end_idx.is_some() || too_new_start_idx.is_some() {
        let mut info = serde_json::Map::new();
        if let Some(idx) = too_old_end_idx {
            info.insert("tooOldLogEventEndIndex".to_string(), json!(idx));
            info.insert("expiredLogEventEndIndex".to_string(), json!(idx));
        }
        if let Some(idx) = too_new_start_idx {
            info.insert("tooNewLogEventStartIndex".to_string(), json!(idx));
        }
        response["rejectedLogEventsInfo"] = Value::Object(info);
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// GetLogEvents
// ---------------------------------------------------------------------------

pub fn get_log_events(
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

    if !group.streams.contains_key(stream_name) {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log stream not found: {stream_name}"),
        ));
    }

    let sqlite = state.sqlite().ok_or_else(|| {
        AwsError::internal("CloudWatch Logs sqlite store not initialised".to_string())
    })?;

    let total = sqlite.count_events(
        &ctx.account_id,
        &ctx.region,
        group_name,
        stream_name,
        start_time,
        end_time,
    )?;

    // Token format: "{f|b}/{offset}" — keep the legacy shape so SDK
    // callers don't need to change. Offsets count from head when
    // ascending, from tail otherwise.
    let offset = parse_offset(next_token);

    let (page_offset, ascending) = if start_from_head {
        (offset, true)
    } else {
        // From tail: page_offset advances backward through the data.
        let bound = total.saturating_sub(offset + limit);
        (bound, true)
    };
    let take = if start_from_head {
        limit
    } else {
        // When pulling from the tail, clamp the requested page so we
        // don't slip below offset 0 once we've exhausted the buffer.
        limit.min(total.saturating_sub(offset))
    };

    let rows = sqlite.get_events(
        &ctx.account_id,
        &ctx.region,
        group_name,
        stream_name,
        start_time,
        end_time,
        page_offset,
        take,
        ascending,
    )?;

    let page: Vec<Value> = rows.iter().map(event_to_json).collect();

    let next_forward_offset = if start_from_head {
        offset + page.len()
    } else {
        // Tail pagination: forward = older, so accumulated offset
        // grows by the page size up to total.
        (offset + page.len()).min(total)
    };
    let next_backward_offset = if offset == 0 {
        0
    } else {
        offset.saturating_sub(limit)
    };

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
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let group_name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    let stream_names: Option<Vec<String>> = input["logStreamNames"].as_array().map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    });

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

    let sqlite = state.sqlite().ok_or_else(|| {
        AwsError::internal("CloudWatch Logs sqlite store not initialised".to_string())
    })?;

    let substring = if filter_pattern.is_empty() {
        None
    } else {
        Some(filter_pattern)
    };

    let rows = sqlite.filter_events(
        &ctx.account_id,
        &ctx.region,
        group_name,
        stream_names.as_deref(),
        substring,
        start_time,
        end_time,
        limit,
    )?;

    let matched_events: Vec<Value> = rows
        .into_iter()
        .map(|(stream_name, ev)| {
            let mut obj = event_to_json(&ev);
            obj["logStreamName"] = json!(stream_name);
            obj["eventId"] = json!(format!("{}-{}", stream_name, ev.timestamp));
            obj
        })
        .collect();

    let searched_streams: Vec<Value> = group
        .streams
        .iter()
        .filter_map(|s| {
            let sname = s.key();
            if let Some(ref names) = stream_names
                && !names.iter().any(|n| n == sname)
            {
                return None;
            }
            Some(json!({
                "logStreamName": sname,
                "searchedCompletely": true,
            }))
        })
        .collect();

    Ok(json!({
        "events": matched_events,
        "searchedLogStreams": searched_streams,
    }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn event_to_json(e: &LogEventRow) -> Value {
    json!({
        "timestamp": e.timestamp,
        "message": e.message,
        "ingestionTime": e.ingestion_time,
    })
}

/// Pagination tokens have the form `"{f|b}/{offset}"`. Older
/// awsim builds emitted a bare integer; tolerate both.
fn parse_offset(token: &str) -> usize {
    if token.is_empty() {
        return 0;
    }
    let body = token.split_once('/').map(|(_, rest)| rest).unwrap_or(token);
    body.parse::<usize>().unwrap_or(0)
}

#[cfg(test)]
mod sequence_token_tests {
    use super::*;
    use crate::SqliteStore;
    use crate::state::LogGroup;
    use std::sync::Arc;

    fn ctx() -> RequestContext {
        RequestContext::new("logs", "us-east-1")
    }

    fn fresh_state() -> LogsState {
        let dir =
            std::env::temp_dir().join(format!("awsim-logs-seqtoken-{}", uuid::Uuid::new_v4()));
        let path = dir.join("logs.db");
        std::fs::create_dir_all(&dir).unwrap();
        let store = Arc::new(SqliteStore::open(path).unwrap());
        let state = LogsState::default();
        state.set_sqlite(store);
        state
    }

    fn seed_stream(state: &LogsState) {
        let group = LogGroup::new(
            "g".to_string(),
            "arn:aws:logs:us-east-1:000000000000:log-group:g".to_string(),
            std::collections::HashMap::new(),
        );
        state.log_groups.insert("g".to_string(), group);
        let group = state.log_groups.get("g").unwrap();
        group.streams.insert(
            "s".to_string(),
            crate::state::LogStream::new(
                "s".to_string(),
                "arn:aws:logs:us-east-1:000000000000:log-group:g:log-stream:s".to_string(),
            ),
        );
    }

    fn make_input(token: Option<&str>) -> Value {
        let mut input = json!({
            "logGroupName": "g",
            "logStreamName": "s",
            "logEvents": [ { "timestamp": now_millis(), "message": "hello" } ],
        });
        if let Some(t) = token {
            input["sequenceToken"] = json!(t);
        }
        input
    }

    #[test]
    fn omitted_token_passes_through() {
        let state = fresh_state();
        seed_stream(&state);
        put_log_events(&state, &make_input(None), &ctx()).unwrap();
    }

    #[test]
    fn matching_token_succeeds() {
        let state = fresh_state();
        seed_stream(&state);
        // Initial token == "1".
        put_log_events(&state, &make_input(Some("1")), &ctx()).unwrap();
    }

    #[test]
    fn mismatched_token_returns_invalid_sequence_token() {
        let state = fresh_state();
        seed_stream(&state);
        let err = put_log_events(&state, &make_input(Some("999")), &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidSequenceTokenException");
        let extras = err.extras.unwrap();
        assert_eq!(
            extras.get("expectedSequenceToken").and_then(|v| v.as_str()),
            Some("1")
        );
    }
}
