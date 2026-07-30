use std::time::SystemTime;

use awsim_core::pagination::{cap_max_results, paginate};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::KinesisState;

/// AWS returns at most 100 streams per page.
const MAX_STREAMS_PER_PAGE: usize = 100;

pub fn handle(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // Promote any due UpdateShardCount transitions before listing.
    let now = SystemTime::now();
    for mut e in state.streams.iter_mut() {
        e.value_mut().promote(now);
    }

    let mut summaries: Vec<Value> = state
        .streams
        .iter()
        .map(|e| {
            let s = e.value();
            json!({
                "StreamName": s.name,
                "StreamARN": s.arn,
                "StreamStatus": s.status,
                "StreamModeDetails": { "StreamMode": s.stream_mode },
                "StreamCreationTimestamp": s.created_at,
            })
        })
        .collect();
    summaries.sort_by(|a, b| a["StreamName"].as_str().cmp(&b["StreamName"].as_str()));

    // `Limit`, `NextToken` and `ExclusiveStartStreamName` were accepted
    // and ignored, so a caller asking for one page got every stream and
    // `HasMoreStreams: false` regardless.
    let limit = cap_max_results(
        input["Limit"].as_i64(),
        MAX_STREAMS_PER_PAGE,
        MAX_STREAMS_PER_PAGE,
    );
    let start = input["NextToken"]
        .as_str()
        .or_else(|| input["ExclusiveStartStreamName"].as_str());
    let page = paginate(summaries, limit, start, |s| {
        s["StreamName"].as_str().unwrap_or_default().to_string()
    })?;

    let stream_names: Vec<Value> = page.items.iter().map(|s| s["StreamName"].clone()).collect();

    let mut out = json!({
        "StreamNames": stream_names,
        "StreamSummaries": page.items,
        "HasMoreStreams": page.next_token.is_some(),
    });
    if let Some(token) = page.next_token {
        out["NextToken"] = json!(token);
    }
    Ok(out)
}
