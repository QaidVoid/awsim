pub mod account;
pub mod certificates;
pub mod credential_report;
pub mod groups;
pub mod instance_profiles;
pub mod mfa;
pub mod misc;
pub mod oidc;
pub mod policies;
pub mod roles;
pub mod saml;
pub mod service_linked_roles;
pub mod ssh_keys;
pub mod tags;
pub mod users;

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

/// Extract an optional boolean parameter. awsQuery requests carry
/// booleans as the strings `true` / `false`; JSON callers send booleans.
pub fn opt_bool(input: &Value, key: &str) -> Option<bool> {
    match input.get(key)? {
        Value::Bool(b) => Some(*b),
        Value::String(s) if s.eq_ignore_ascii_case("true") => Some(true),
        Value::String(s) if s.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

/// Extract an optional unsigned integer parameter, which awsQuery
/// requests carry as a decimal string.
pub fn opt_u32(input: &Value, key: &str) -> Option<u32> {
    match input.get(key)? {
        Value::Number(n) => n.as_u64().and_then(|n| u32::try_from(n).ok()),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_query_string_scalars_parse() {
        let input = json!({ "A": "true", "B": false, "C": "7200", "D": 900, "E": "x" });
        assert_eq!(opt_bool(&input, "A"), Some(true));
        assert_eq!(opt_bool(&input, "B"), Some(false));
        assert_eq!(opt_bool(&input, "E"), None);
        assert_eq!(opt_u32(&input, "C"), Some(7200));
        assert_eq!(opt_u32(&input, "D"), Some(900));
        assert_eq!(opt_u32(&input, "E"), None);
    }
}
