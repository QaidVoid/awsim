//! ECR error helpers. The Smithy model leaves `httpError` unset on
//! every ecr error shape (except `ValidationException`), so AWS returns
//! HTTP 400 for everything including the `*NotFoundException` and
//! `*AlreadyExistsException` variants.

use awsim_core::AwsError;

pub fn repository_not_found(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("RepositoryNotFoundException", message)
}

pub fn repository_already_exists(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("RepositoryAlreadyExistsException", message)
}

pub fn image_not_found(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ImageNotFoundException", message)
}

pub fn invalid_parameter(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidParameterException", message)
}

pub fn lifecycle_policy_not_found(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("LifecyclePolicyNotFoundException", message)
}
