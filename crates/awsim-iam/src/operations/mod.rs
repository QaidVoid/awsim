pub mod account;
pub mod certificates;
pub mod credential_report;
pub mod groups;
pub mod instance_profiles;
pub mod misc;
pub mod mfa;
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
