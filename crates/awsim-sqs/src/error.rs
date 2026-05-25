//! SQS error helpers. SQS uses the Query protocol and overrides most
//! wire codes via `aws.protocols#awsQueryError.code`. Per the
//! `sqs.json` Smithy model, `NonExistentQueue` is HTTP 400 (not 404)
//! because SQS treats a missing queue as a request validation failure
//! rather than a REST resource lookup. `ReceiptHandleIsInvalid` and
//! `ResourceNotFoundException` are 404.

use awsim_core::AwsError;

pub fn nonexistent_queue(queue: &str) -> AwsError {
    AwsError::bad_request(
        "AWS.SimpleQueueService.NonExistentQueue",
        format!("The specified queue does not exist: {queue}"),
    )
}

pub fn receipt_handle_invalid(message: impl Into<String>) -> AwsError {
    AwsError::not_found("ReceiptHandleIsInvalid", message)
}

pub fn resource_not_found(message: impl Into<String>) -> AwsError {
    AwsError::not_found("ResourceNotFoundException", message)
}

pub fn queue_name_exists(name: &str) -> AwsError {
    AwsError::bad_request(
        "QueueAlreadyExists",
        format!("A queue already exists with the name {name}"),
    )
}

pub fn batch_request_too_long(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("AWS.SimpleQueueService.BatchRequestTooLong", message)
}
