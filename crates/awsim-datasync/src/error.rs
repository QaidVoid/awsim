//! DataSync error helpers. The Smithy model declares
//! `InvalidRequestException` (400) and `InternalException` (400) only;
//! every error surfaces with HTTP 400.

use awsim_core::AwsError;

pub fn invalid_request(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidRequestException", message)
}
