//! SSM error helpers. Every SSM error in the Smithy model defaults to
//! HTTP 400 (none carry an explicit `httpError` trait), even shapes
//! whose names suggest 404 / 409 semantics. Consolidating the codes
//! here prevents call sites from reaching for `not_found` / `conflict`
//! and accidentally returning the wrong status.

use awsim_core::AwsError;

pub fn parameter_not_found(name: &str) -> AwsError {
    AwsError::bad_request("ParameterNotFound", format!("Parameter {name} not found"))
}

pub fn parameter_already_exists(name: &str) -> AwsError {
    AwsError::bad_request(
        "ParameterAlreadyExists",
        format!("Parameter {name} already exists"),
    )
}

pub fn invalid_parameters(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidParameters", message)
}

pub fn resource_not_found(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ResourceNotFoundException", message)
}
