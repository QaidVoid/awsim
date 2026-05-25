//! Kinesis error helpers. The Smithy model leaves `httpError` unset on
//! every kinesis error shape, so all kinesis errors are HTTP 400 even
//! when the semantics are "not found" or "already exists".

use awsim_core::AwsError;

pub fn resource_not_found(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ResourceNotFoundException", message)
}

pub fn resource_in_use(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ResourceInUseException", message)
}

pub fn limit_exceeded(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("LimitExceededException", message)
}

pub fn provisioned_throughput_exceeded(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ProvisionedThroughputExceededException", message)
}

pub fn expired_iterator(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ExpiredIteratorException", message)
}

pub fn invalid_argument(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidArgumentException", message)
}
