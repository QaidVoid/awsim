use awsim_core::AwsError;

pub fn resource_not_found(resource_type: &str, id: &str) -> AwsError {
    AwsError::not_found(
        "InvalidParameterValue",
        format!("The {resource_type} '{id}' does not exist"),
    )
}

pub fn resource_already_exists(resource_type: &str, id: &str) -> AwsError {
    AwsError::conflict(
        "InvalidParameterValue",
        format!("The {resource_type} '{id}' already exists"),
    )
}

pub fn missing_parameter(param: &str) -> AwsError {
    AwsError::bad_request(
        "MissingParameter",
        format!("The request must contain the parameter {param}"),
    )
}

pub fn invalid_parameter(msg: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidParameterValue", msg)
}
