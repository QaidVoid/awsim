use awsim_core::AwsError;

pub fn entity_already_exists(entity: &str, name: &str) -> AwsError {
    AwsError::conflict(
        "EntityAlreadyExists",
        format!("{entity} with name {name} already exists"),
    )
}

pub fn no_such_entity(entity: &str, name: &str) -> AwsError {
    AwsError::not_found("NoSuchEntity", format!("{entity} {name} cannot be found"))
}

pub fn missing_parameter(param: &str) -> AwsError {
    AwsError::bad_request(
        "MissingParameter",
        format!("The request must contain the parameter {param}"),
    )
}

pub fn delete_conflict(message: impl Into<String>) -> AwsError {
    AwsError::conflict("DeleteConflict", message)
}

pub fn malformed_policy_document(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("MalformedPolicyDocument", message)
}

pub fn limit_exceeded(message: impl Into<String>) -> AwsError {
    // AWS IAM models LimitExceededException with httpResponseCode=409.
    AwsError::conflict("LimitExceeded", message)
}

pub fn validation_error(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("ValidationError", message)
}
