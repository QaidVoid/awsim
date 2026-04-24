use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{Organization, OrganizationsState, Root};

pub fn create_organization(
    state: &OrganizationsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let feature_set = input["FeatureSet"].as_str().unwrap_or("ALL").to_string();
    let org_id = format!("o-{}", &uuid::Uuid::new_v4().simple().to_string()[..10]);
    let arn = format!(
        "arn:aws:organizations::{}:organization/{}",
        ctx.account_id, org_id
    );

    let master_arn = format!(
        "arn:aws:organizations::{}:account/{}/{}",
        ctx.account_id, org_id, ctx.account_id
    );
    let org = Organization {
        id: org_id.clone(),
        arn: arn.clone(),
        feature_set: feature_set.clone(),
        master_account_id: ctx.account_id.clone(),
        master_account_arn: master_arn.clone(),
        master_account_email: format!("master+{}@example.com", ctx.account_id),
    };

    if state.organization.read().unwrap().is_some() {
        return Err(AwsError::conflict(
            "AlreadyInOrganizationException",
            "Organization already exists",
        ));
    }

    *state.organization.write().unwrap() = Some(org.clone());

    let root_id = format!("r-{}", &uuid::Uuid::new_v4().simple().to_string()[..4]);
    let root_arn = format!(
        "arn:aws:organizations::{}:root/{}/{}",
        ctx.account_id, org_id, root_id
    );
    state.roots.insert(
        root_id.clone(),
        Root {
            id: root_id.clone(),
            arn: root_arn,
            name: "Root".to_string(),
            policy_types: vec![],
        },
    );

    Ok(json!({
        "Organization": serialize_org(&org)
    }))
}

pub fn describe_organization(
    state: &OrganizationsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let guard = state.organization.read().unwrap();
    let org = guard.as_ref().ok_or_else(|| {
        AwsError::not_found("AWSOrganizationsNotInUseException", "No organization")
    })?;
    Ok(json!({ "Organization": serialize_org(org) }))
}

pub(crate) fn serialize_org(org: &Organization) -> Value {
    json!({
        "Id": org.id,
        "Arn": org.arn,
        "FeatureSet": org.feature_set,
        "MasterAccountArn": org.master_account_arn,
        "MasterAccountId": org.master_account_id,
        "MasterAccountEmail": org.master_account_email,
        "AvailablePolicyTypes": [],
    })
}
