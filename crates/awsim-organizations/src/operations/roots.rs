use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::OrganizationsState;

pub fn list_roots(
    state: &OrganizationsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let roots: Vec<Value> = state
        .roots
        .iter()
        .map(|e| {
            let r = e.value();
            json!({
                "Id": r.id,
                "Arn": r.arn,
                "Name": r.name,
                "PolicyTypes": [],
            })
        })
        .collect();
    Ok(json!({ "Roots": roots }))
}

pub fn list_children(
    state: &OrganizationsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let parent = input["ParentId"].as_str().unwrap_or("");
    let child_type = input["ChildType"].as_str().unwrap_or("ORGANIZATIONAL_UNIT");
    let children: Vec<Value> = if child_type == "ACCOUNT" {
        state
            .accounts
            .iter()
            .filter(|e| {
                state
                    .parents
                    .get(&e.value().id)
                    .map(|p| p.value() == parent)
                    .unwrap_or(parent.starts_with("r-"))
            })
            .map(|e| json!({ "Id": e.value().id, "Type": "ACCOUNT" }))
            .collect()
    } else {
        state
            .ous
            .iter()
            .filter(|e| {
                state
                    .parents
                    .get(e.key())
                    .map(|p| p.value() == parent)
                    .unwrap_or(false)
            })
            .map(|e| json!({ "Id": e.value().id, "Type": "ORGANIZATIONAL_UNIT" }))
            .collect()
    };
    Ok(json!({ "Children": children }))
}
