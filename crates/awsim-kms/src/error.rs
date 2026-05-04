use awsim_core::AwsError;

pub fn not_found(resource: &str) -> AwsError {
    AwsError::not_found("NotFoundException", format!("{resource} does not exist"))
}

pub fn invalid_key_id(key_id: &str) -> AwsError {
    AwsError::bad_request("InvalidKeyIdException", format!("Invalid key ID: {key_id}"))
}

pub fn alias_exists(alias: &str) -> AwsError {
    AwsError::conflict(
        "AlreadyExistsException",
        format!("An alias with the name {alias} already exists"),
    )
}

pub fn key_disabled(key_id: &str) -> AwsError {
    AwsError::bad_request("DisabledException", format!("Key {key_id} is disabled"))
}

pub fn kms_invalid_state(message: impl Into<String>) -> AwsError {
    AwsError::conflict("KMSInvalidStateException", message)
}

pub fn key_pending_deletion(key_id: &str) -> AwsError {
    kms_invalid_state(format!("Key {key_id} is pending deletion"))
}

pub fn missing_parameter(param: &str) -> AwsError {
    AwsError::bad_request(
        "InvalidParameterException",
        format!("Missing required parameter: {param}"),
    )
}

pub fn invalid_parameter(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidParameterException", message)
}

/// HTTP 400 InvalidKeyUsageException — raised when an operation is called
/// against a key whose KeyUsage doesn't match (e.g. GenerateMac on a
/// SIGN_VERIFY key).
pub fn invalid_key_usage(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidKeyUsageException", message)
}
