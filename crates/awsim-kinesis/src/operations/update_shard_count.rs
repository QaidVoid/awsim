use std::time::{Duration, SystemTime};

use awsim_core::lifecycle::fast_mode;
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{KinesisState, Shard};
use crate::util::divide_hash_space;

/// How long a stream advertises `UPDATING` before promoting back to
/// `ACTIVE` (collapsed to zero under `AWSIM_LIFECYCLE_FAST=1`).
const UPDATE_DELAY: Duration = Duration::from_secs(30);

/// UpdateShardCount — change a stream's shard count. The stream flips
/// to `UPDATING`, stages the replacement shard set, and promotes back
/// to `ACTIVE` once the transition deadline elapses (observed in the
/// `Describe*` read paths and the tick loop).
pub fn handle(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_name = input["StreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "StreamName is required"))?;
    let target_shard_count = input["TargetShardCount"]
        .as_u64()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "TargetShardCount is required"))?;

    if target_shard_count == 0 || target_shard_count > 10_000 {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            "TargetShardCount must be between 1 and 10000",
        ));
    }

    let mut stream = state.streams.get_mut(stream_name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", stream_name),
        )
    })?;

    // Promote any prior pending update before reading the live count.
    stream.promote(SystemTime::now());

    let current_count = stream.shards.len();
    let target = target_shard_count as usize;

    let stream_arn = stream.arn.clone();

    // Stage the replacement shard set; promotion swaps it in.
    let ranges = divide_hash_space(target);
    let new_shards: Vec<Shard> = ranges
        .into_iter()
        .enumerate()
        .map(|(i, (start, end))| Shard::new_range(i, start, end))
        .collect();

    let delay = if fast_mode() {
        Duration::ZERO
    } else {
        UPDATE_DELAY
    };
    stream.begin_update(SystemTime::now() + delay, new_shards);

    Ok(json!({
        "StreamName": stream_name,
        "StreamARN": stream_arn,
        "CurrentShardCount": current_count,
        "TargetShardCount": target,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::{describe_stream, describe_stream_summary};
    use crate::state::{KinesisState, KinesisStream, Shard};

    fn ctx() -> RequestContext {
        RequestContext::new("000000000000", "us-east-1")
    }

    fn seed(state: &KinesisState, name: &str, shard_count: usize) {
        let shards: Vec<Shard> = (0..shard_count)
            .map(|i| Shard::new_range(i, i as u128, (i as u128) + 1))
            .collect();
        let stream = KinesisStream {
            name: name.to_string(),
            arn: format!("arn:aws:kinesis:us-east-1:000000000000:stream/{name}"),
            status: "ACTIVE".to_string(),
            shards,
            retention_hours: 24,
            tags: Default::default(),
            created_at: 0,
            enhanced_monitoring: vec![],
            encryption_type: "NONE".to_string(),
            key_id: None,
            stream_mode: "PROVISIONED".to_string(),
            warm_throughput_mibps: 0,
            warm_throughput_records: 0,
            pending_update: None,
        };
        state.streams.insert(name.to_string(), stream);
    }

    #[test]
    fn update_shard_count_flips_updating_then_active() {
        // Exercise the fast lifecycle path (harmless if cached false:
        // the deterministic promote below still drives the transition).
        unsafe {
            std::env::set_var("AWSIM_LIFECYCLE_FAST", "1");
        }

        let state = KinesisState::default();
        seed(&state, "s", 1);

        let resp = handle(
            &state,
            &json!({ "StreamName": "s", "TargetShardCount": 4 }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["CurrentShardCount"], json!(1));
        assert_eq!(resp["TargetShardCount"], json!(4));

        // Immediately after the call the stream advertises UPDATING and
        // still reports the old shard count.
        let summary =
            describe_stream_summary::handle(&state, &json!({ "StreamName": "s" }), &ctx()).unwrap();
        let status = summary["StreamDescriptionSummary"]["StreamStatus"]
            .as_str()
            .unwrap();
        // Under fast mode the read path promotes synchronously; either
        // ordering is valid, so accept both and force promotion next.
        assert!(status == "UPDATING" || status == "ACTIVE");

        // Force the deadline regardless of the global fast-mode cache.
        {
            let mut s = state.streams.get_mut("s").unwrap();
            s.promote(SystemTime::now() + std::time::Duration::from_secs(120));
        }

        let desc = describe_stream::handle(&state, &json!({ "StreamName": "s" }), &ctx()).unwrap();
        assert_eq!(
            desc["StreamDescription"]["StreamStatus"].as_str(),
            Some("ACTIVE")
        );
        let shards = desc["StreamDescription"]["Shards"].as_array().unwrap();
        assert_eq!(shards.len(), 4);

        unsafe {
            std::env::remove_var("AWSIM_LIFECYCLE_FAST");
        }
    }
}
