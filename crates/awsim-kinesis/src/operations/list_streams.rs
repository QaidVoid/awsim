use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::KinesisState;

pub fn handle(
    state: &KinesisState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let mut stream_names: Vec<String> = state.streams.iter().map(|e| e.key().clone()).collect();
    stream_names.sort();

    let stream_summaries: Vec<Value> = state
        .streams
        .iter()
        .map(|e| {
            let s = e.value();
            json!({
                "StreamName": s.name,
                "StreamARN": s.arn,
                "StreamStatus": s.status,
                "StreamCreationTimestamp": s.created_at,
            })
        })
        .collect();

    Ok(json!({
        "StreamNames": stream_names,
        "StreamSummaries": stream_summaries,
        "HasMoreStreams": false,
    }))
}
