pub mod gateways;
pub mod instances;
pub mod key_pairs;
pub mod metadata;
pub mod route_tables;
pub mod security_groups;
pub mod stubs;
pub mod subnets;
pub mod tags;
pub mod vpcs;

use serde_json::Value;

/// Extract a required string parameter from the input Value.
pub fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, awsim_core::AwsError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| crate::error::missing_parameter(key))
}

/// Extract an optional string parameter from the input Value.
pub fn opt_str<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(|v| v.as_str())
}

/// Extract an optional i64 from the input Value (handles both string and number).
pub fn opt_i64(input: &Value, key: &str) -> Option<i64> {
    input.get(key).and_then(|v| match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    })
}
