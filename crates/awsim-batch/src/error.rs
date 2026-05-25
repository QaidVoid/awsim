//! Batch error helpers. AWS Batch's Smithy model only declares
//! `ClientException` (400) and `ServerException` (500) as named error
//! shapes; everything else surfaces via the generic protocol layer.

use awsim_core::AwsError;

pub fn client_exception(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ClientException", message)
}
