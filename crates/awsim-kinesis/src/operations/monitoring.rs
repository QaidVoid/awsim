use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::KinesisState;

/// EnableEnhancedMonitoring — add shard-level metrics to the stream.
pub fn enable_enhanced_monitoring(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_name = input["StreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "StreamName is required"))?;

    let shard_level_metrics: Vec<String> = input["ShardLevelMetrics"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let mut stream = state.streams.get_mut(stream_name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", stream_name),
        )
    })?;

    let stream_arn = stream.arn.clone();

    for metric in &shard_level_metrics {
        if !stream.enhanced_monitoring.contains(metric) {
            stream.enhanced_monitoring.push(metric.clone());
        }
    }

    let enabled: Vec<Value> = stream
        .enhanced_monitoring
        .iter()
        .map(|m| Value::String(m.clone()))
        .collect();

    Ok(json!({
        "StreamName": stream_name,
        "StreamARN": stream_arn,
        "CurrentShardLevelMetrics": enabled,
        "DesiredShardLevelMetrics": shard_level_metrics,
    }))
}

/// DisableEnhancedMonitoring — remove shard-level metrics from the stream.
pub fn disable_enhanced_monitoring(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_name = input["StreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "StreamName is required"))?;

    let shard_level_metrics: Vec<String> = input["ShardLevelMetrics"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let mut stream = state.streams.get_mut(stream_name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", stream_name),
        )
    })?;

    let stream_arn = stream.arn.clone();

    stream
        .enhanced_monitoring
        .retain(|m| !shard_level_metrics.contains(m));

    let remaining: Vec<Value> = stream
        .enhanced_monitoring
        .iter()
        .map(|m| Value::String(m.clone()))
        .collect();

    Ok(json!({
        "StreamName": stream_name,
        "StreamARN": stream_arn,
        "CurrentShardLevelMetrics": remaining,
        "DesiredShardLevelMetrics": shard_level_metrics,
    }))
}
