use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use super::delete_stream::resolve_stream_name;
use crate::state::KinesisState;

fn require_resource_arn(input: &Value) -> Result<&str, AwsError> {
    input["ResourceARN"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "ResourceARN is required"))
}

pub fn put_resource_policy(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = require_resource_arn(input)?.to_string();
    let policy = input["Policy"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Policy is required"))?;
    state.resource_policies.insert(arn, policy.to_string());
    Ok(json!({}))
}

pub fn get_resource_policy(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = require_resource_arn(input)?;
    let policy = state
        .resource_policies
        .get(arn)
        .map(|p| p.value().clone())
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Policy for {arn} not found"),
            )
        })?;
    Ok(json!({ "Policy": policy }))
}

pub fn delete_resource_policy(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = require_resource_arn(input)?;
    state.resource_policies.remove(arn);
    Ok(json!({}))
}

pub fn tag_resource(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = require_resource_arn(input)?.to_string();
    let mut tags = state.resource_tags.entry(arn).or_default();
    if let Some(map) = input["Tags"].as_object() {
        for (k, v) in map {
            if let Some(s) = v.as_str() {
                tags.insert(k.clone(), s.to_string());
            }
        }
    }
    Ok(json!({}))
}

pub fn untag_resource(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = require_resource_arn(input)?;
    if let Some(mut entry) = state.resource_tags.get_mut(arn)
        && let Some(keys) = input["TagKeys"].as_array()
    {
        for k in keys {
            if let Some(s) = k.as_str() {
                entry.remove(s);
            }
        }
    }
    Ok(json!({}))
}

pub fn list_tags_for_resource(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = require_resource_arn(input)?;

    let stream_tags: Option<HashMap<String, String>> = if arn.contains(":stream/") {
        let name = arn.rsplit('/').next().unwrap_or("");
        state.streams.get(name).map(|s| s.tags.clone())
    } else {
        None
    };
    let resource_tag_map = state.resource_tags.get(arn).map(|t| t.value().clone());

    let merged = match (stream_tags, resource_tag_map) {
        (Some(mut a), Some(b)) => {
            a.extend(b);
            a
        }
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => HashMap::new(),
    };

    let tags: Vec<Value> = merged
        .into_iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();

    Ok(json!({
        "Tags": tags,
        "NextToken": null,
    }))
}

pub fn describe_account_settings(
    state: &KinesisState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let s = state.account_settings.read().unwrap().clone();
    Ok(json!({
        "MaxRecordSize": s.max_record_size,
        "DefaultShardLimit": s.default_shard_limit,
    }))
}

pub fn describe_limits(
    state: &KinesisState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let s = state.account_settings.read().unwrap().clone();
    let open_shard_count: u64 = state
        .streams
        .iter()
        .map(|e| e.value().shards.len() as u64)
        .sum();

    Ok(json!({
        "ShardLimit": s.default_shard_limit,
        "OpenShardCount": open_shard_count,
        "OnDemandStreamCount": 0u64,
        "OnDemandStreamCountLimit": 50u64,
    }))
}

pub fn update_account_settings(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let mut settings = state.account_settings.write().unwrap();
    if let Some(v) = input["MaxRecordSize"].as_u64() {
        settings.max_record_size = v;
    }
    if let Some(v) = input["DefaultShardLimit"].as_u64() {
        settings.default_shard_limit = v;
    }
    Ok(json!({}))
}

pub fn update_max_record_size(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let v = input["MaxRecordSize"]
        .as_u64()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "MaxRecordSize is required"))?;
    state.account_settings.write().unwrap().max_record_size = v;
    Ok(json!({ "MaxRecordSize": v }))
}

pub fn update_stream_mode(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_name = resolve_stream_name(state, input)?;
    let mode = input["StreamModeDetails"]["StreamMode"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request(
                "MissingParameter",
                "StreamModeDetails.StreamMode is required",
            )
        })?;

    let mut stream = state.streams.get_mut(&stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", stream_name),
        )
    })?;
    stream.stream_mode = mode.to_string();

    Ok(json!({}))
}

pub fn update_stream_warm_throughput(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_name = resolve_stream_name(state, input)?;
    let mibps = input["WarmThroughputMiBps"].as_u64().unwrap_or(0);
    let records = input["WarmThroughputRecordsPerSecond"]
        .as_u64()
        .unwrap_or(0);

    let mut stream = state.streams.get_mut(&stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", stream_name),
        )
    })?;
    stream.warm_throughput_mibps = mibps;
    stream.warm_throughput_records = records;

    Ok(json!({
        "StreamName": stream_name,
        "StreamARN": stream.arn,
        "CurrentWarmThroughput": {
            "MiBps": stream.warm_throughput_mibps,
            "RecordsPerSecond": stream.warm_throughput_records,
        },
        "DesiredWarmThroughput": {
            "MiBps": mibps,
            "RecordsPerSecond": records,
        },
    }))
}
