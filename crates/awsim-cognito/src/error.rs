//! Cognito IDP error helpers. The Smithy model splits status codes
//! between the typical 400s and 403 for auth/policy failures
//! (NotAuthorizedException, AccessDeniedException, ForbiddenException).
//! Most cognito errors use the shape name verbatim on the wire.

use awsim_core::AwsError;

pub fn not_authorized(message: impl Into<String>) -> AwsError {
    AwsError::forbidden("NotAuthorizedException", message)
}

pub fn resource_not_found(message: impl Into<String>) -> AwsError {
    AwsError::not_found("ResourceNotFoundException", message)
}

pub fn user_not_found(message: impl Into<String>) -> AwsError {
    AwsError::not_found("UserNotFoundException", message)
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
