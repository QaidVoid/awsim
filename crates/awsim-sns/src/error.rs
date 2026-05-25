//! SNS error helpers. SNS uses the Query protocol; the wire codes are
//! the `awsQueryError.code` overrides from `sns.json`:
//! `InvalidParameter` for InvalidParameterException (400) and `NotFound`
//! for NotFoundException (404).

use awsim_core::AwsError;

pub fn invalid_parameter(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidParameter", message)
}

pub fn not_found(message: impl Into<String>) -> AwsError {
    AwsError::not_found("NotFound", message)
}
