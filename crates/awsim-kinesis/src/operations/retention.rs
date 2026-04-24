use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use super::delete_stream::resolve_stream_name;
use crate::state::KinesisState;

/// Minimum and maximum retention period in hours (Kinesis limits).
const MIN_RETENTION_HOURS: u32 = 24;
const MAX_RETENTION_HOURS: u32 = 8760; // 365 days

pub fn increase(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_name = resolve_stream_name(state, input)?;

    let hours = input["RetentionPeriodHours"].as_u64().ok_or_else(|| {
        AwsError::bad_request("MissingParameter", "RetentionPeriodHours is required")
    })? as u32;

    if hours > MAX_RETENTION_HOURS {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!(
                "RetentionPeriodHours must be at most {}",
                MAX_RETENTION_HOURS
            ),
        ));
    }

    let mut stream = state.streams.get_mut(&stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", stream_name),
        )
    })?;

    if hours <= stream.retention_hours {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            "IncreaseStreamRetentionPeriod requires a value greater than the current retention period",
        ));
    }

    stream.retention_hours = hours;
    Ok(json!({}))
}

pub fn decrease(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_name = resolve_stream_name(state, input)?;

    let hours = input["RetentionPeriodHours"].as_u64().ok_or_else(|| {
        AwsError::bad_request("MissingParameter", "RetentionPeriodHours is required")
    })? as u32;

    if hours < MIN_RETENTION_HOURS {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!(
                "RetentionPeriodHours must be at least {}",
                MIN_RETENTION_HOURS
            ),
        ));
    }

    let mut stream = state.streams.get_mut(&stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", stream_name),
        )
    })?;

    if hours >= stream.retention_hours {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            "DecreaseStreamRetentionPeriod requires a value less than the current retention period",
        ));
    }

    stream.retention_hours = hours;
    Ok(json!({}))
}
