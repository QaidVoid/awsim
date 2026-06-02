use std::time::SystemTime;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::KinesisState;

pub fn handle(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_name = input["StreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "StreamName is required"))?;

    // Promote a due UpdateShardCount before reading status/shard count.
    if let Some(mut s) = state.streams.get_mut(stream_name) {
        s.promote(SystemTime::now());
    }

    let stream = state.streams.get(stream_name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", stream_name),
        )
    })?;

    Ok(json!({
        "StreamDescriptionSummary": {
            "StreamName": stream.name,
            "StreamARN": stream.arn,
            "StreamStatus": stream.status,
            "RetentionPeriodHours": stream.retention_hours,
            "StreamCreationTimestamp": stream.created_at,
            "EnhancedMonitoring": [],
            "EncryptionType": "NONE",
            "OpenShardCount": stream.shards.len(),
            "ConsumerCount": 0,
        }
    }))
}
