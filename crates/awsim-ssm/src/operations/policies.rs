use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{SsmResourcePolicy, SsmState};

pub fn put_resource_policy(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceArn is required"))?
        .to_string();
    let policy = input["Policy"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Policy is required"))?
        .to_string();

    let policy_id = input["PolicyId"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let policy_hash = format!("{:x}", md5ish(&policy));

    let p = SsmResourcePolicy {
        policy_id: policy_id.clone(),
        policy_hash: policy_hash.clone(),
        resource_arn: resource_arn.clone(),
        policy,
    };

    state
        .resource_policies
        .entry(resource_arn)
        .or_default()
        .push(p);

    Ok(json!({
        "PolicyId": policy_id,
        "PolicyHash": policy_hash,
    }))
}

pub fn get_resource_policies(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceArn is required"))?;

    let policies: Vec<Value> = state
        .resource_policies
        .get(resource_arn)
        .map(|e| {
            e.value()
                .iter()
                .map(|p| {
                    json!({
                        "PolicyId": p.policy_id,
                        "PolicyHash": p.policy_hash,
                        "Policy": p.policy,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(json!({ "Policies": policies }))
}

pub fn delete_resource_policy(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceArn is required"))?;
    let policy_id = input["PolicyId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PolicyId is required"))?;

    if let Some(mut entry) = state.resource_policies.get_mut(resource_arn) {
        entry.retain(|p| p.policy_id != policy_id);
    }

    Ok(json!({}))
}

fn md5ish(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
