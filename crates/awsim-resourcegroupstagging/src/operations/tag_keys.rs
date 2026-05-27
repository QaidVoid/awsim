use std::collections::BTreeSet;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::TaggingState;

/// `GetTagKeys` — return the union of all tag keys currently in use.
///
/// AWS bounds `TagsPerPage` to 100..=500 inclusive when the caller
/// supplies it; we reject values outside that range with
/// `ValidationException` to match the AWS surface, even though the
/// emulator's small fleets fit comfortably in a single page.
pub fn get_tag_keys(
    state: &TaggingState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    super::validate_tags_per_page(input)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn ctx() -> RequestContext {
        RequestContext::new("tagging", "us-east-1")
    }

    #[test]
    fn rejects_tags_per_page_below_100() {
        let state = TaggingState::default();
        let err = get_tag_keys(&state, &json!({ "TagsPerPage": 50 }), &ctx()).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn rejects_tags_per_page_above_500() {
        let state = TaggingState::default();
        let err = get_tag_keys(&state, &json!({ "TagsPerPage": 501 }), &ctx()).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn accepts_tags_per_page_at_bounds() {
        let state = TaggingState::default();
        state.resources.insert(
            "arn:aws:s3:::b".into(),
            BTreeMap::from([("k".into(), "v".into())]),
        );
        get_tag_keys(&state, &json!({ "TagsPerPage": 100 }), &ctx()).unwrap();
        get_tag_keys(&state, &json!({ "TagsPerPage": 500 }), &ctx()).unwrap();
    }
}
