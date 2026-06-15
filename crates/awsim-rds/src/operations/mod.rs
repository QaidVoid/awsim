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

/// Coerce a JSON value to a bool, accepting both native booleans and the
/// string forms the awsQuery protocol delivers (every form value arrives
/// as a string).
pub fn coerce_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Coerce a JSON value to a u64, accepting native numbers and numeric
/// strings (the awsQuery wire form).
pub fn coerce_u64(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Coerce a JSON value to an i64, accepting native numbers and numeric
/// strings.
pub fn coerce_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Coerce a JSON value to an f64, accepting native numbers and numeric
/// strings.
pub fn coerce_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Extract an optional bool parameter from the input Value.
pub fn opt_bool(input: &Value, key: &str) -> Option<bool> {
    input.get(key).and_then(coerce_bool)
}

/// Extract an optional u32 parameter from the input Value.
pub fn opt_u32(input: &Value, key: &str) -> Option<u32> {
    input.get(key).and_then(coerce_u64).map(|v| v as u32)
}

#[cfg(test)]
mod coercion_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn coerces_numbers_and_booleans_from_strings() {
        // The awsQuery protocol delivers every value as a string.
        let input = json!({
            "AllocatedStorage": "50",
            "PubliclyAccessible": "true",
            "MultiAZ": "false",
            "MinCapacity": "0.5",
        });
        assert_eq!(opt_u32(&input, "AllocatedStorage"), Some(50));
        assert_eq!(opt_bool(&input, "PubliclyAccessible"), Some(true));
        assert_eq!(opt_bool(&input, "MultiAZ"), Some(false));
        assert_eq!(coerce_f64(&input["MinCapacity"]), Some(0.5));
    }

    #[test]
    fn still_accepts_native_json_types() {
        let input = json!({ "AllocatedStorage": 50, "PubliclyAccessible": true });
        assert_eq!(opt_u32(&input, "AllocatedStorage"), Some(50));
        assert_eq!(opt_bool(&input, "PubliclyAccessible"), Some(true));
    }

    #[test]
    fn rejects_non_numeric_and_non_boolean_strings() {
        let input = json!({ "n": "abc", "b": "yes" });
        assert_eq!(opt_u32(&input, "n"), None);
        assert_eq!(opt_bool(&input, "b"), None);
    }
}
