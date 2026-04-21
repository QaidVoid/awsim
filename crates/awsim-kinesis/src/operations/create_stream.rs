use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{KinesisState, KinesisStream, Shard, now_secs};
use crate::util::divide_hash_space;

pub fn handle(state: &KinesisState, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
    let stream_name = input["StreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "StreamName is required"))?;

    let shard_count = input["ShardCount"].as_u64().unwrap_or(1).max(1) as usize;

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
    };

    info!(stream = %stream_name, shards = shard_count, "Created Kinesis stream");
    state.streams.insert(stream_name.to_string(), stream);

    Ok(json!({}))
}
