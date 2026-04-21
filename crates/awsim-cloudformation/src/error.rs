use awsim_core::AwsError;

pub fn stack_not_found(name: &str) -> AwsError {
    AwsError::not_found(
        "ValidationError",
        format!("Stack with id {name} does not exist"),
    )
}

pub fn stack_already_exists(name: &str) -> AwsError {
    AwsError::conflict(
        "AlreadyExistsException",
        format!("Stack [{name}] already exists"),
    )
}

pub fn change_set_not_found(name: &str) -> AwsError {
    AwsError::not_found(
        "ChangeSetNotFoundException",
        format!("ChangeSet [{name}] does not exist"),
    )
}

pub fn missing_parameter(param: &str) -> AwsError {
    AwsError::bad_request(
        "MissingParameter",
        format!("The request must contain the parameter {param}"),
    )
}

pub fn invalid_template(msg: impl Into<String>) -> AwsError {
    AwsError::bad_request("ValidationError", msg)
}
