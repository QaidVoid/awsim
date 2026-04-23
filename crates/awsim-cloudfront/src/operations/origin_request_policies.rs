use awsim_core::AwsError;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    ids::{new_etag, now_iso8601},
    state::{CloudFrontState, OriginRequestPolicy},
};

fn not_found(id: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchOriginRequestPolicy",
        format!("The specified origin request policy does not exist: {id}"),
    )
}

fn policy_to_value(p: &OriginRequestPolicy) -> Value {
    json!({
        "Id": p.id,
        "LastModifiedTime": p.created_at,
        "OriginRequestPolicyConfig": {
            "Name": p.name,
            "Comment": p.comment,
            "HeadersConfig": { "HeaderBehavior": "none" },
            "CookiesConfig": { "CookieBehavior": "none" },
            "QueryStringsConfig": { "QueryStringBehavior": "none" },
        }
    })
}

pub fn create_origin_request_policy(
    state: &CloudFrontState,
    input: &Value,
) -> Result<Value, AwsError> {
    let cfg = input.get("OriginRequestPolicyConfig").unwrap_or(input);
    let name = cfg
        .get("Name")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();
    let comment = cfg
        .get("Comment")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let id = Uuid::new_v4().to_string();
    let etag = new_etag();

    let policy = OriginRequestPolicy {
        id: id.clone(),
        name,
        comment,
        created_at: now_iso8601(),
        etag: etag.clone(),
    };

    let result = policy_to_value(&policy);
    state.origin_request_policies.insert(id, policy);

    Ok(json!({
        "OriginRequestPolicy": result,
        "ETag": etag,
    }))
}

pub fn get_origin_request_policy(state: &CloudFrontState, id: &str) -> Result<Value, AwsError> {
    let p = state.origin_request_policies.get(id).ok_or_else(|| not_found(id))?;
    let etag = p.etag.clone();
    let result = policy_to_value(&p);
    Ok(json!({
        "OriginRequestPolicy": result,
        "ETag": etag,
    }))
}

pub fn delete_origin_request_policy(state: &CloudFrontState, id: &str) -> Result<Value, AwsError> {
    if state.origin_request_policies.remove(id).is_none() {
        return Err(not_found(id));
    }
    Ok(json!({}))
}

pub fn list_origin_request_policies(state: &CloudFrontState) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .origin_request_policies
        .iter()
        .map(|e| json!({ "Type": "custom", "OriginRequestPolicy": policy_to_value(e.value()) }))
        .collect();
    let qty = items.len();
    Ok(json!({
        "OriginRequestPolicyList": {
            "MaxItems": 100,
            "Quantity": qty,
            "Items": { "OriginRequestPolicySummary": items }
        }
    }))
}
