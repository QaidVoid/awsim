use awsim_core::AwsError;

pub fn resource_not_found(resource_type: &str, id: &str) -> AwsError {
    AwsError::not_found(
        "LoadBalancerNotFound",
        format!("The {resource_type} '{id}' does not exist"),
    )
}

pub fn missing_parameter(param: &str) -> AwsError {
    AwsError::bad_request(
        "ValidationError",
        format!("'{}' is required", param),
    )
}
