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

    validate_resource_arn(&resource_arn)?;

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

/// AWS WAFv2 only allows associating a Regional WebACL with a fixed
/// set of resource service types. AssociateWebACL rejects anything
/// else with WAFInvalidParameterException at the API boundary; mirror
/// that so callers don't silently associate ACLs with unsupported
/// resources.
fn validate_resource_arn(arn: &str) -> Result<(), AwsError> {
    if !arn.starts_with("arn:") {
        return Err(AwsError::bad_request(
            "WAFInvalidParameterException",
            format!("ResourceArn `{arn}` is not a valid ARN."),
        ));
    }
    let parts: Vec<&str> = arn.splitn(6, ':').collect();
    let service = parts.get(2).copied().unwrap_or("");
    let resource = parts.get(5).copied().unwrap_or("");
    let allowed = match service {
        // ALB: arn:aws:elasticloadbalancing:...:loadbalancer/app/<name>/<id>
        "elasticloadbalancing" => resource.starts_with("loadbalancer/app/"),
        // REST/HTTP API stage: arn:aws:apigateway:...::/restapis/<id>/stages/<name>
        "apigateway" => resource.contains("/stages/"),
        "appsync" => resource.starts_with("apis/"),
        "cognito-idp" => resource.starts_with("userpool/"),
        "apprunner" => resource.starts_with("service/"),
        // EC2 Verified Access Instance only.
        "ec2" => resource.starts_with("verified-access-instance/"),
        _ => false,
    };
    if !allowed {
        return Err(AwsError::bad_request(
            "WAFInvalidParameterException",
            format!(
                "ResourceArn `{arn}` is not a supported WAFv2 association target. \
                 Allowed: ALB, API Gateway Stage, AppSync GraphQL API, Cognito User Pool, \
                 App Runner Service, EC2 Verified Access Instance."
            ),
        ));
    }
    Ok(())
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

#[cfg(test)]
mod resource_arn_tests {
    use super::*;

    #[test]
    fn accepts_alb() {
        validate_resource_arn(
            "arn:aws:elasticloadbalancing:us-east-1:000000000000:loadbalancer/app/lb/abcd",
        )
        .unwrap();
    }

    #[test]
    fn accepts_apigateway_stage() {
        validate_resource_arn("arn:aws:apigateway:us-east-1::/restapis/abc123/stages/prod")
            .unwrap();
    }

    #[test]
    fn accepts_cognito_user_pool() {
        validate_resource_arn(
            "arn:aws:cognito-idp:us-east-1:000000000000:userpool/us-east-1_abcd1234",
        )
        .unwrap();
    }

    #[test]
    fn rejects_s3_bucket() {
        let err = validate_resource_arn("arn:aws:s3:::my-bucket").unwrap_err();
        assert_eq!(err.code, "WAFInvalidParameterException");
    }

    #[test]
    fn rejects_nlb() {
        // NLB lives under elasticloadbalancing but uses /net/ not /app/.
        let err = validate_resource_arn(
            "arn:aws:elasticloadbalancing:us-east-1:000000000000:loadbalancer/net/nlb/xy",
        )
        .unwrap_err();
        assert_eq!(err.code, "WAFInvalidParameterException");
    }

    #[test]
    fn rejects_non_arn() {
        let err = validate_resource_arn("not-an-arn").unwrap_err();
        assert_eq!(err.code, "WAFInvalidParameterException");
    }
}
