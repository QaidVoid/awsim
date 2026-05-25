use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{KinesisState, Shard};
use crate::util::divide_hash_space;

/// UpdateShardCount — update the number of shards in a stream.
/// Replaces existing shards with a fresh set of shards at the new count.
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

    let current_count = stream.shards.len();
    let target = target_shard_count as usize;

    let stream_arn = stream.arn.clone();

    // Rebuild shards with the new count
    let ranges = divide_hash_space(target);
    stream.shards = ranges
        .into_iter()
        .enumerate()
        .map(|(i, (start, end))| Shard::new_range(i, start, end))
        .collect();

    Ok(json!({
        "StreamName": stream_name,
        "StreamARN": stream_arn,
        "CurrentShardCount": current_count,
        "TargetShardCount": target,
    }))
}
