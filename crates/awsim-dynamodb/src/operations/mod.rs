pub mod backup;
pub mod batch;
pub mod item;
pub mod kinesis_dest;
pub mod partiql;
pub mod query;
pub mod resource_policy;
pub mod streams;
pub mod table;
pub mod transact;

use serde_json::Value;

/// Extract an optional string from input JSON.
pub fn opt_str<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(|v| v.as_str())
}

/// Extract a required string from input JSON.
pub fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, awsim_core::AwsError> {
    input.get(key).and_then(|v| v.as_str()).ok_or_else(|| {
        awsim_core::AwsError::bad_request("ValidationException", format!("{key} is required"))
    })
}

/// Build an empty ExpressionAttributeNames map if not present.
pub fn get_expr_attr_names(input: &Value) -> std::collections::HashMap<String, String> {
    input
        .get("ExpressionAttributeNames")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Build an ExpressionAttributeValues map.
pub fn get_expr_attr_values(input: &Value) -> serde_json::Map<String, Value> {
    input
        .get("ExpressionAttributeValues")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default()
}
