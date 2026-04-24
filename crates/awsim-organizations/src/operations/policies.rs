use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{OrganizationsState, Policy};

pub fn create_policy(
    state: &OrganizationsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Name is required"))?;
    let content = input["Content"].as_str().unwrap_or("{}").to_string();
    let ptype = input["Type"]
        .as_str()
        .unwrap_or("SERVICE_CONTROL_POLICY")
        .to_string();
    let description = input["Description"].as_str().unwrap_or("").to_string();

    let uid = uuid::Uuid::new_v4().simple().to_string();
    let policy_id = format!("p-{}", &uid[..8]);
    let arn = format!(
        "arn:aws:organizations::{}:policy/{}/{}/{}",
        ctx.account_id,
        state
            .organization
            .read()
            .unwrap()
            .as_ref()
            .map(|o| o.id.clone())
            .unwrap_or_default(),
        ptype.to_lowercase(),
        policy_id,
    );
    let policy = Policy {
        id: policy_id.clone(),
        arn,
        name: name.to_string(),
        description,
        policy_type: ptype,
        content,
        aws_managed: false,
    };
    state.policies.insert(policy_id.clone(), policy.clone());
    Ok(json!({ "Policy": serialize_policy_full(&policy) }))
}

pub fn describe_policy(
    state: &OrganizationsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input["PolicyId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "PolicyId is required"))?;
    let p = state.policies.get(id).ok_or_else(|| {
        AwsError::not_found("PolicyNotFoundException", format!("Policy {id} not found"))
    })?;
    Ok(json!({ "Policy": serialize_policy_full(&p) }))
}

pub fn list_policies(
    state: &OrganizationsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let policies: Vec<Value> = state
        .policies
        .iter()
        .map(|e| serialize_policy_summary(e.value()))
        .collect();
    Ok(json!({ "Policies": policies }))
}

pub fn attach_policy(
    state: &OrganizationsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let policy_id = input["PolicyId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "PolicyId is required"))?;
    let target = input["TargetId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "TargetId is required"))?;

    let mut entry = state
        .policy_attachments
        .entry(policy_id.to_string())
        .or_default();
    if !entry.contains(&target.to_string()) {
        entry.push(target.to_string());
    }
    Ok(json!({}))
}

pub fn detach_policy(
    state: &OrganizationsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let policy_id = input["PolicyId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "PolicyId is required"))?;
    let target = input["TargetId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "TargetId is required"))?;
    if let Some(mut v) = state.policy_attachments.get_mut(policy_id) {
        v.retain(|t| t != target);
    }
    Ok(json!({}))
}

fn serialize_policy_summary(p: &Policy) -> Value {
    json!({
        "Id": p.id,
        "Arn": p.arn,
        "Name": p.name,
        "Description": p.description,
        "Type": p.policy_type,
        "AwsManaged": p.aws_managed,
    })
}

fn serialize_policy_full(p: &Policy) -> Value {
    json!({
        "PolicySummary": serialize_policy_summary(p),
        "Content": p.content,
    })
}
