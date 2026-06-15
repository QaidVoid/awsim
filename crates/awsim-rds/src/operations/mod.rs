pub mod cluster_endpoints;
pub mod cluster_parameter_groups;
pub mod cluster_snapshots;
pub mod clusters;
pub mod engine_versions;
pub mod instances;
pub mod parameter_groups;
pub mod snapshots;
pub mod subnet_groups;
pub mod tags;

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

/// Extract an optional bool parameter from the input Value.
pub fn opt_bool(input: &Value, key: &str) -> Option<bool> {
    input.get(key).and_then(|v| v.as_bool())
}

/// Extract an optional u32 parameter from the input Value.
pub fn opt_u32(input: &Value, key: &str) -> Option<u32> {
    input.get(key).and_then(|v| v.as_u64()).map(|v| v as u32)
}
