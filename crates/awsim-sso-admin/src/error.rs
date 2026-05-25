//! SSO-Admin error helpers. The Smithy model maps
//! `ResourceNotFoundException` to 404 explicitly.

use awsim_core::AwsError;

pub fn resource_not_found(message: impl Into<String>) -> AwsError {
    AwsError::not_found("ResourceNotFoundException", message)
}

pub fn validation(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ValidationException", message)
}
