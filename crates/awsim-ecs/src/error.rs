//! ECS error helpers. The Smithy model marks every error as HTTP 400
//! (only `AccessDeniedException` is 403); even `*NotFoundException`
//! variants are 400 because ECS treats unknown resources as request
//! validation failures rather than REST resource lookups.

use awsim_core::AwsError;

pub fn cluster_not_found(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ClusterNotFoundException", message)
}

pub fn service_not_found(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ServiceNotFoundException", message)
}

pub fn invalid_parameter(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidParameterException", message)
}

pub fn client_exception(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ClientException", message)
}
