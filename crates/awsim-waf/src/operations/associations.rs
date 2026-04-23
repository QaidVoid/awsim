use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::WafState;

pub fn associate_web_acl(
    state: &WafState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let web_acl_arn = input["WebACLArn"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("WAFInvalidParameterException", "WebACLArn is required")
        })?
        .to_string();

    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("WAFInvalidParameterException", "ResourceArn is required")
        })?
        .to_string();

    let exists = state.web_acls.iter().any(|e| e.value().arn == web_acl_arn);
    if !exists {
        return Err(AwsError::not_found(
            "WAFNonexistentItemException",
            format!("WebACL not found: {web_acl_arn}"),
        ));
    }

    state.web_acl_associations.insert(resource_arn, web_acl_arn);

    Ok(json!({}))
}

pub fn disassociate_web_acl(
    state: &WafState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("WAFInvalidParameterException", "ResourceArn is required")
    })?;

    state.web_acl_associations.remove(resource_arn);

    Ok(json!({}))
}

pub fn get_web_acl_for_resource(
    state: &WafState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("WAFInvalidParameterException", "ResourceArn is required")
    })?;

    let acl_arn = match state.web_acl_associations.get(resource_arn) {
        Some(e) => e.value().clone(),
        None => return Ok(json!({})),
    };

    let acl = state
        .web_acls
        .iter()
        .find(|e| e.value().arn == acl_arn)
        .map(|e| e.value().clone());

    if let Some(acl) = acl {
        Ok(json!({
            "WebACL": {
                "ARN": acl.arn,
                "Id": acl.id,
                "Name": acl.name,
                "DefaultAction": acl.default_action,
                "Rules": acl.rules,
                "VisibilityConfig": acl.visibility_config,
            }
        }))
    } else {
        Ok(json!({}))
    }
}

pub fn list_resources_for_web_acl(
    state: &WafState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let web_acl_arn = input["WebACLArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("WAFInvalidParameterException", "WebACLArn is required")
    })?;

    let resources: Vec<String> = state
        .web_acl_associations
        .iter()
        .filter(|e| e.value() == web_acl_arn)
        .map(|e| e.key().clone())
        .collect();

    Ok(json!({ "ResourceArns": resources }))
}
