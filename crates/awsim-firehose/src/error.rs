//! Firehose error helpers. The Smithy model leaves `httpError` unset on
//! every firehose error shape (except `ServiceUnavailableException`),
//! so AWS returns HTTP 400 across the board.

use awsim_core::AwsError;

pub fn resource_not_found(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ResourceNotFoundException", message)
}

pub fn resource_in_use(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ResourceInUseException", message)
}

pub fn invalid_argument(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidArgumentException", message)
}
