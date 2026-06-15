use awsim_core::AwsError;

pub fn db_instance_already_exists(identifier: &str) -> AwsError {
    AwsError::conflict(
        "DBInstanceAlreadyExists",
        format!("DB instance already exists: {identifier}"),
    )
}

pub fn db_instance_not_found(identifier: &str) -> AwsError {
    AwsError::not_found(
        "DBInstanceNotFound",
        format!("DB instance not found: {identifier}"),
    )
}

pub fn db_cluster_already_exists(identifier: &str) -> AwsError {
    AwsError::conflict(
        "DBClusterAlreadyExistsFault",
        format!("DB cluster already exists: {identifier}"),
    )
}

pub fn db_cluster_not_found(identifier: &str) -> AwsError {
    AwsError::not_found(
        "DBClusterNotFoundFault",
        format!("DB cluster not found: {identifier}"),
    )
}

pub fn db_subnet_group_already_exists(name: &str) -> AwsError {
    AwsError::conflict(
        "DBSubnetGroupAlreadyExists",
        format!("DB subnet group already exists: {name}"),
    )
}

pub fn db_subnet_group_not_found(name: &str) -> AwsError {
    AwsError::not_found(
        "DBSubnetGroupNotFoundFault",
        format!("DB subnet group not found: {name}"),
    )
}

pub fn db_parameter_group_already_exists(name: &str) -> AwsError {
    AwsError::conflict(
        "DBParameterGroupAlreadyExists",
        format!("DB parameter group already exists: {name}"),
    )
}

pub fn db_parameter_group_not_found(name: &str) -> AwsError {
    AwsError::not_found(
        "DBParameterGroupNotFound",
        format!("DB parameter group not found: {name}"),
    )
}

pub fn db_cluster_snapshot_already_exists(identifier: &str) -> AwsError {
    AwsError::conflict(
        "DBClusterSnapshotAlreadyExistsFault",
        format!("DB cluster snapshot already exists: {identifier}"),
    )
}

pub fn db_cluster_snapshot_not_found(identifier: &str) -> AwsError {
    AwsError::not_found(
        "DBClusterSnapshotNotFoundFault",
        format!("DB cluster snapshot not found: {identifier}"),
    )
}

pub fn db_cluster_role_already_exists(role_arn: &str) -> AwsError {
    AwsError::conflict(
        "DBClusterRoleAlreadyExists",
        format!("Role is already associated with the cluster: {role_arn}"),
    )
}

pub fn db_cluster_role_not_found(role_arn: &str) -> AwsError {
    AwsError::not_found(
        "DBClusterRoleNotFound",
        format!("Role is not associated with the cluster: {role_arn}"),
    )
}

pub fn missing_parameter(param: &str) -> AwsError {
    AwsError::bad_request(
        "MissingParameter",
        format!("The request must contain the parameter {param}"),
    )
}

pub fn invalid_parameter(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidParameterValue", message)
}

pub fn invalid_db_instance_state(identifier: &str, state: &str) -> AwsError {
    AwsError::conflict(
        "InvalidDBInstanceState",
        format!("DB instance {identifier} is in state {state}"),
    )
}
