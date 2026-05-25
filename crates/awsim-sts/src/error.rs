//! STS error helpers. STS uses the Query protocol; most STS errors are
//! generic Query validation/auth failures rather than service-specific
//! shapes. This module collects the few STS-specific exceptions so call
//! sites stop spelling out the shape names.

use awsim_core::AwsError;

pub fn invalid_authorization_message(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidAuthorizationMessageException", message)
}
