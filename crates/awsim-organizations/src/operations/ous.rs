use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{OrganizationalUnit, OrganizationsState};

pub fn create_ou(
    state: &OrganizationsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Name is required"))?;
    let parent_id = input["ParentId"].as_str().unwrap_or("r-0000");

    let uid = uuid::Uuid::new_v4().simple().to_string();
    let ou_id = format!("ou-{}-{}", &uid[..4], &uid[4..12]);
    let arn = format!(
        "arn:aws:organizations::{}:ou/{}/{}",
        ctx.account_id,
        state
            .organization
            .read()
            .unwrap()
            .as_ref()
            .map(|o| o.id.clone())
            .unwrap_or_default(),
        ou_id
    );
    let ou = OrganizationalUnit {
        id: ou_id.clone(),
        arn,
        name: name.to_string(),
    };
    state.ous.insert(ou_id.clone(), ou.clone());
    state.parents.insert(ou_id.clone(), parent_id.to_string());

    Ok(json!({ "OrganizationalUnit": serialize_ou(&ou) }))
}

pub fn describe_ou(
    state: &OrganizationsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input["OrganizationalUnitId"].as_str().ok_or_else(|| {
        AwsError::bad_request("MissingParameter", "OrganizationalUnitId is required")
    })?;
    let ou = state.ous.get(id).ok_or_else(|| {
        AwsError::not_found(
            "OrganizationalUnitNotFoundException",
            format!("OU {id} not found"),
        )
    })?;
    Ok(json!({ "OrganizationalUnit": serialize_ou(&ou) }))
}

pub fn list_ous_for_parent(
    state: &OrganizationsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let parent = input["ParentId"].as_str().unwrap_or("");
    let ous: Vec<Value> = state
        .ous
        .iter()
        .filter(|e| {
            state
                .parents
                .get(e.key())
                .map(|p| p.value() == parent)
                .unwrap_or(false)
        })
        .map(|e| serialize_ou(e.value()))
        .collect();
    Ok(json!({ "OrganizationalUnits": ous }))
}

pub(crate) fn serialize_ou(ou: &OrganizationalUnit) -> Value {
    json!({
        "Id": ou.id,
        "Arn": ou.arn,
        "Name": ou.name,
    })
}
