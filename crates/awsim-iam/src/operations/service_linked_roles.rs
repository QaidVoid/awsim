use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::{entity_already_exists, no_such_entity},
    ids::{new_role_id, new_uuid, now_iso8601},
    state::{IamState, Role},
};

use super::{opt_str, require_str};

pub fn create_service_linked_role(
    state: &IamState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let aws_service_name = require_str(input, "AWSServiceName")?;
    let description = opt_str(input, "Description").map(|s| s.to_string());
    let custom_suffix = opt_str(input, "CustomSuffix").unwrap_or("");

    let role_name = if custom_suffix.is_empty() {
        format!("AWSServiceRoleFor{}", aws_service_name.split('.').next().unwrap_or(aws_service_name))
    } else {
        format!(
            "AWSServiceRoleFor{}_{}",
            aws_service_name.split('.').next().unwrap_or(aws_service_name),
            custom_suffix
        )
    };

    if state.roles.contains_key(&role_name) {
        return Err(entity_already_exists("Role", &role_name));
    }

    let path = format!("/aws-service-role/{}/", aws_service_name);
    let assume_role_policy = format!(
        r#"{{"Version":"2012-10-17","Statement":[{{"Effect":"Allow","Principal":{{"Service":"{}"}},"Action":"sts:AssumeRole"}}]}}"#,
        aws_service_name
    );

    let role_id = new_role_id();
    let arn = format!(
        "arn:aws:iam::{}:role{}{}",
        ctx.account_id, path, role_name
    );

    let role = Role {
        role_name: role_name.clone(),
        role_id,
        arn: arn.clone(),
        path,
        assume_role_policy_document: assume_role_policy,
        description,
        create_date: now_iso8601(),
        max_session_duration: 3600,
        attached_policies: Vec::new(),
        inline_policies: HashMap::new(),
        tags: HashMap::new(),
    };

    let result = json!({
        "RoleName": role.role_name,
        "RoleId": role.role_id,
        "Arn": role.arn,
        "Path": role.path,
        "AssumeRolePolicyDocument": role.assume_role_policy_document,
        "CreateDate": role.create_date,
    });

    state.roles.insert(role_name, role);

    Ok(json!({ "Role": result }))
}

pub fn delete_service_linked_role(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let role_name = require_str(input, "RoleName")?;

    if !state.roles.contains_key(role_name) {
        return Err(no_such_entity("Role", role_name));
    }

    let task_id = new_uuid();
    state.deletion_tasks.insert(task_id.clone(), "SUCCEEDED".to_string());
    state.roles.remove(role_name);

    Ok(json!({ "DeletionTaskId": task_id }))
}

pub fn get_service_linked_role_deletion_status(
    state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let task_id = require_str(input, "DeletionTaskId")?;

    let status = state
        .deletion_tasks
        .get(task_id)
        .map(|s| s.value().clone())
        .unwrap_or_else(|| "SUCCEEDED".to_string());

    Ok(json!({ "Status": status }))
}
