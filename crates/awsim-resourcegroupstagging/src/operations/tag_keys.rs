use std::collections::BTreeSet;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::TaggingState;

/// `GetTagKeys` — return the union of all tag keys currently in use.
pub fn get_tag_keys(
    state: &TaggingState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let keys: BTreeSet<String> = state
        .resources
        .iter()
        .flat_map(|entry| entry.value().keys().cloned().collect::<Vec<_>>())
        .collect();
    Ok(json!({
        "PaginationToken": "",
        "TagKeys": keys.into_iter().collect::<Vec<_>>(),
    }))
}
