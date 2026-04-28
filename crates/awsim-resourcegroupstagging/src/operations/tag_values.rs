use std::collections::BTreeSet;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::TaggingState;

/// `GetTagValues` — return all values currently associated with `Key`.
pub fn get_tag_values(
    state: &TaggingState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key = input
        .get("Key")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::validation("Key is required"))?;

    let values: BTreeSet<String> = state
        .resources
        .iter()
        .filter_map(|entry| entry.value().get(key).cloned())
        .collect();

    Ok(json!({
        "PaginationToken": "",
        "TagValues": values.into_iter().collect::<Vec<_>>(),
    }))
}
