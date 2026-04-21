use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::KinesisState;

pub fn handle_merge(_state: &KinesisState, _input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    // MergeShards is a stub — resharding is complex and not required for local dev
    Ok(json!({}))
}

pub fn handle_split(_state: &KinesisState, _input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    // SplitShard is a stub — resharding is complex and not required for local dev
    Ok(json!({}))
}
