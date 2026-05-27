use std::collections::BTreeSet;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::TaggingState;

/// `GetTagValues` — return all values currently associated with `Key`.
///
/// `TagsPerPage` follows the same 100..=500 envelope AWS documents
/// for `GetTagKeys`; values outside that range surface
/// `ValidationException`.
pub fn get_tag_values(
    state: &TaggingState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    super::validate_tags_per_page(input)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("tagging", "us-east-1")
    }

    #[test]
    fn rejects_out_of_range_tags_per_page() {
        let state = TaggingState::default();
        for bad in [0i64, 99, 501, 1000] {
            let err = get_tag_values(&state, &json!({ "Key": "Env", "TagsPerPage": bad }), &ctx())
                .unwrap_err();
            assert_eq!(err.code, "ValidationException", "input {bad}");
        }
    }
}
