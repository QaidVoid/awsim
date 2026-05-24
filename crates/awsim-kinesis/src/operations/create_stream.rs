use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{KinesisState, KinesisStream, Shard, now_secs};
use crate::util::divide_hash_space;

pub fn handle(
    state: &KinesisState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_name = input["StreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "StreamName is required"))?;
    validate_stream_name(stream_name)?;

    let stream_mode = input["StreamModeDetails"]["StreamMode"]
        .as_str()
        .unwrap_or("PROVISIONED")
        .to_string();
    if !matches!(stream_mode.as_str(), "PROVISIONED" | "ON_DEMAND") {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!("StreamMode '{stream_mode}' must be PROVISIONED or ON_DEMAND."),
        ));
    }

    let raw_shard_count = input["ShardCount"].as_u64();
    // PROVISIONED mode requires ShardCount in [1, 10000]; ON_DEMAND
    // ignores it (AWS assigns shards based on throughput). Mirror
    // that: require the field on PROVISIONED, reject zero / overflow,
    // and default to 4 for ON_DEMAND to match AWS's initial allocation.
    let shard_count = if stream_mode == "ON_DEMAND" {
        raw_shard_count.unwrap_or(4).max(1) as usize
    } else {
        let count = raw_shard_count.unwrap_or(1);
        if !(1..=10_000).contains(&count) {
            return Err(AwsError::bad_request(
                "InvalidArgumentException",
                format!("ShardCount {count} must be between 1 and 10000."),
            ));
        }
        count as usize
    };

    if state.streams.contains_key(stream_name) {
        return Err(AwsError::conflict(
            "ResourceInUseException",
            format!("Stream {} already exists", stream_name),
        ));
    }

    let arn = format!(
        "arn:aws:kinesis:{}:{}:stream/{}",
        ctx.region, ctx.account_id, stream_name
    );

    let ranges = divide_hash_space(shard_count);
    let shards: Vec<Shard> = ranges
        .into_iter()
        .enumerate()
        .map(|(i, (start, end))| Shard::new_range(i, start, end))
        .collect();

    let stream = KinesisStream {
        name: stream_name.to_string(),
        arn,
        status: "ACTIVE".to_string(),
        shards,
        retention_hours: 24,
        tags: Default::default(),
        created_at: now_secs(),
        enhanced_monitoring: Vec::new(),
        encryption_type: "NONE".to_string(),
        key_id: None,
        stream_mode,
        warm_throughput_mibps: 0,
        warm_throughput_records: 0,
    };

    info!(stream = %stream_name, shards = shard_count, "Created Kinesis stream");
    state.streams.insert(stream_name.to_string(), stream);

    Ok(json!({}))
}

/// Validate a Kinesis stream name against AWS's documented regex:
/// 1-128 characters from `[a-zA-Z0-9_.-]+`. Without this check, a
/// caller can register a name with spaces or slashes that real
/// Kinesis refuses on first PutRecord.
fn validate_stream_name(name: &str) -> Result<(), AwsError> {
    if name.is_empty() || name.len() > 128 {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!(
                "StreamName length must be between 1 and 128, got {}.",
                name.len()
            ),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!("StreamName '{name}' must match [a-zA-Z0-9_.-]+."),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod create_stream_validation_tests {
    use super::*;

    #[test]
    fn name_accepts_documented_charset() {
        validate_stream_name("orders").unwrap();
        validate_stream_name("orders-v2").unwrap();
        validate_stream_name("orders.dev_123").unwrap();
    }

    #[test]
    fn name_rejects_disallowed_chars() {
        assert!(validate_stream_name("orders/v2").is_err());
        assert!(validate_stream_name("orders v2").is_err());
        assert!(validate_stream_name("orders:v2").is_err());
    }

    #[test]
    fn name_rejects_empty_or_too_long() {
        assert!(validate_stream_name("").is_err());
        let long = "a".repeat(129);
        assert!(validate_stream_name(&long).is_err());
    }
}
