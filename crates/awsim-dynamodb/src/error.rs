//! DynamoDB error helpers. The Smithy model leaves `httpError` unset on
//! every dynamodb error shape, so AWS returns HTTP 400 across the board
//! (including for `ResourceNotFoundException`, `ResourceInUseException`,
//! `ConditionalCheckFailedException`, and the various `*AlreadyExists`
//! shapes). SDK retry classifiers key off the `__type` string, not the
//! status code, so the catalog test pins those code strings here.

use awsim_core::AwsError;

pub fn resource_in_use(resource: impl Into<String>) -> AwsError {
    AwsError::bad_request("ResourceInUseException", resource)
}

pub fn table_already_exists(name: &str) -> AwsError {
    AwsError::bad_request(
        "TableAlreadyExistsException",
        format!("Table already exists: {name}"),
    )
}

pub fn global_table_already_exists(name: &str) -> AwsError {
    AwsError::bad_request(
        "GlobalTableAlreadyExistsException",
        format!("Global table '{name}' already exists"),
    )
}

pub fn resource_not_found(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ResourceNotFoundException", message)
}

pub fn provisioned_throughput_exceeded(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ProvisionedThroughputExceededException", message)
}

pub fn conditional_check_failed(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ConditionalCheckFailedException", message)
}

pub fn transaction_canceled(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("TransactionCanceledException", message)
}
