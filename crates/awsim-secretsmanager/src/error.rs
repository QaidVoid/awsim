use awsim_core::AwsError;

pub fn resource_not_found(id: &str) -> AwsError {
    AwsError::not_found(
        "ResourceNotFoundException",
        format!("Secrets Manager can't find the specified secret: {id}"),
    )
}

pub fn resource_exists(name: &str) -> AwsError {
    AwsError::conflict(
        "ResourceExistsException",
        format!("A secret with name {name} already exists"),
    )
}

pub fn invalid_parameter(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidParameterException", message)
}

pub fn invalid_request(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidRequestException", message)
}

pub fn missing_parameter(param: &str) -> AwsError {
    AwsError::bad_request(
        "InvalidParameterException",
        format!("Missing required parameter: {param}"),
    )
}
