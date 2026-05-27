pub mod resources;
pub mod tag_keys;
pub mod tag_values;
pub mod tagging;

use awsim_core::AwsError;
use serde_json::Value;

/// Enforce AWS's `TagsPerPage` bounds (100..=500) on `GetTagKeys` and
/// `GetTagValues`. Returns `Ok(())` when the field is absent (AWS
/// applies the service default in that case).
pub(crate) fn validate_tags_per_page(input: &Value) -> Result<(), AwsError> {
    match input.get("TagsPerPage").and_then(Value::as_i64) {
        Some(n) if !(100..=500).contains(&n) => Err(AwsError::validation(format!(
            "TagsPerPage `{n}` must be in 100..=500."
        ))),
        _ => Ok(()),
    }
}
