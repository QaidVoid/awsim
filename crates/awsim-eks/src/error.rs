//! EKS error helpers. EKS uses REST-JSON; the wire `__type` field
//! carries the shape name verbatim. HTTP statuses follow `eks.json`:
//! ResourceNotFound is 404, ResourceInUse is 409, parameter validation
//! is 400.

use awsim_core::AwsError;

pub fn resource_not_found(message: impl Into<String>) -> AwsError {
    AwsError::not_found("ResourceNotFoundException", message)
}

pub fn resource_in_use(message: impl Into<String>) -> AwsError {
    AwsError::conflict("ResourceInUseException", message)
}

pub fn invalid_parameter(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidParameterException", message)
}

pub fn invalid_request(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidRequestException", message)
}
