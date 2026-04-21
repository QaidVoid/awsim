use awsim_core::AwsError;

pub fn resource_not_found(resource_type: &str, name: &str) -> AwsError {
    AwsError::not_found(
        "ResourceNotFoundException",
        format!("Function not found: {resource_type} {name}"),
    )
}

pub fn resource_conflict(message: impl Into<String>) -> AwsError {
    AwsError::conflict("ResourceConflictException", message)
}

pub fn invalid_parameter(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidParameterValueException", message)
}

pub fn missing_parameter(param: &str) -> AwsError {
    AwsError::bad_request(
        "InvalidParameterValueException",
        format!("The request must contain the parameter {param}"),
    )
}
