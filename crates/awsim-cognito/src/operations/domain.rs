use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::CognitoState;

// ---------------------------------------------------------------------------
// CreateUserPoolDomain
// ---------------------------------------------------------------------------

/// The region a pool belongs to, encoded as the prefix of its id
/// (`{region}_{suffix}`). The hosted-UI CloudFront hostname uses the pool's
/// region, not the caller's request region.
fn pool_region(pool_id: &str) -> &str {
    pool_id.split('_').next().unwrap_or(pool_id)
}

pub fn create_user_pool_domain(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let domain = input["Domain"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "Domain is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    if state.domain_pool_map.contains_key(domain) {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!("Domain already exists: {domain}"),
        ));
    }

    pool.domain = Some(domain.to_string());
    state
        .domain_pool_map
        .insert(domain.to_string(), pool_id.to_string());

    info!(domain = %domain, pool_id = %pool_id, "Cognito: created user pool domain");

    Ok(json!({
        "CloudFrontDomain": format!("{domain}.auth.{}.amazoncognito.com", pool_region(pool_id))
    }))
}

// ---------------------------------------------------------------------------
// DescribeUserPoolDomain
// ---------------------------------------------------------------------------

pub fn describe_user_pool_domain(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let domain = input["Domain"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "Domain is required"))?;

    let pool_id_entry = state.domain_pool_map.get(domain);

    if let Some(pool_id_ref) = pool_id_entry {
        let pool_id = pool_id_ref.clone();
        drop(pool_id_ref);

        let pool = state.user_pools.get(&pool_id).ok_or_else(|| {
            AwsError::service_not_found(
                "ResourceNotFoundException",
                format!("User pool not found for domain: {domain}"),
            )
        })?;

        Ok(json!({
            "DomainDescription": {
                "Domain": domain,
                "UserPoolId": pool.id,
                "AWSAccountId": ctx.account_id,
                "S3Bucket": "",
                "CloudFrontDistribution": format!("{domain}.auth.{}.amazoncognito.com", pool_region(&pool.id)),
                "CustomDomainConfig": {},
                "Status": "ACTIVE",
                "Version": "1"
            }
        }))
    } else {
        Ok(json!({ "DomainDescription": {} }))
    }
}

// ---------------------------------------------------------------------------
// DeleteUserPoolDomain
// ---------------------------------------------------------------------------

pub fn delete_user_pool_domain(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let domain = input["Domain"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "Domain is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    pool.domain = None;
    state.domain_pool_map.remove(domain);

    info!(domain = %domain, pool_id = %pool_id, "Cognito: deleted user pool domain");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UpdateUserPoolDomain
// ---------------------------------------------------------------------------

pub fn update_user_pool_domain(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let domain = input["Domain"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "Domain is required"))?;

    // Verify pool exists
    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;
    drop(pool);

    // Update domain_pool_map — remove old mapping if any
    let old_domain = state.user_pools.get(pool_id).and_then(|p| p.domain.clone());
    if let Some(old) = old_domain
        && old != domain
    {
        state.domain_pool_map.remove(&old);
    }

    let mut pool_mut = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;
    pool_mut.domain = Some(domain.to_string());
    drop(pool_mut);

    state
        .domain_pool_map
        .insert(domain.to_string(), pool_id.to_string());

    info!(domain = %domain, pool_id = %pool_id, "Cognito: updated user pool domain");
    Ok(json!({
        "CloudFrontDomain": format!("{domain}.auth.{}.amazoncognito.com", pool_region(pool_id))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::pools::create_user_pool;
    use serde_json::json;

    #[test]
    fn domain_cloudfront_uses_pool_region_not_request_region() {
        let state = CognitoState::default();
        // Pool created in eu-west-1.
        let create_ctx = RequestContext::new("cognito-idp", "eu-west-1");
        create_user_pool(&state, &json!({ "PoolName": "p" }), &create_ctx).unwrap();
        let pool_id = state.user_pools.iter().next().unwrap().id.clone();
        create_user_pool_domain(
            &state,
            &json!({ "UserPoolId": pool_id, "Domain": "myapp" }),
            &create_ctx,
        )
        .unwrap();
        // Describe from a different request region: hostname keeps eu-west-1.
        let other_ctx = RequestContext::new("cognito-idp", "us-east-1");
        let resp =
            describe_user_pool_domain(&state, &json!({ "Domain": "myapp" }), &other_ctx).unwrap();
        assert_eq!(
            resp["DomainDescription"]["CloudFrontDistribution"],
            "myapp.auth.eu-west-1.amazoncognito.com"
        );
    }
}
