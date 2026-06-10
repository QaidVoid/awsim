//! Cognito IDP error helpers. The Smithy model annotates some shapes
//! with 403/404 `httpError` traits, but the live service is JSON-1.1
//! and answers every client error with HTTP 400; SDKs dispatch on the
//! `__type` shape name, which Cognito emits verbatim on the wire.

use awsim_core::AwsError;

pub fn not_authorized(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("NotAuthorizedException", message)
}

pub fn resource_not_found(message: impl Into<String>) -> AwsError {
    AwsError::service_not_found("ResourceNotFoundException", message)
}

pub fn user_not_found(message: impl Into<String>) -> AwsError {
    AwsError::service_not_found("UserNotFoundException", message)
}

pub fn invalid_parameter(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidParameterException", message)
}

pub fn invalid_password(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidPasswordException", message)
}

pub fn code_mismatch(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("CodeMismatchException", message)
}

pub fn username_exists(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("UsernameExistsException", message)
}

/// Cognito's only server fault; HTTP 500 with code `InternalErrorException`.
pub fn internal_error(message: impl Into<String>) -> AwsError {
    let mut err = AwsError::internal(message);
    err.code = "InternalErrorException".to_string();
    err
}
