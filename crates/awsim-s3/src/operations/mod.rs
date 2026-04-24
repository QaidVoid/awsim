pub mod bucket;
pub mod config;
pub mod list;
pub mod multipart;
pub mod object;

use awsim_core::AwsError;
use serde_json::Value;

/// Extract a required string field from a JSON Value.
pub fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input.get(key).and_then(Value::as_str).ok_or_else(|| {
        AwsError::bad_request(
            "MissingParameter",
            format!("Missing required parameter: {key}"),
        )
    })
}

/// Extract an optional string field from a JSON Value.
pub fn opt_str<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(Value::as_str)
}
