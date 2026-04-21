use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::KinesisState;

pub fn handle(state: &KinesisState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let stream_name = input["StreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "StreamName is required"))?;

    let stream = state.streams.get(stream_name).ok_or_else(|| {
        AwsError::not_found(
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
