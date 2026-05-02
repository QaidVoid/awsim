use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::CognitoState;

// ---------------------------------------------------------------------------
// CreateUserPoolDomain
// ---------------------------------------------------------------------------

pub fn create_user_pool_domain(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let domain = input["Domain"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Domain is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if state.domain_pool_map.contains_key(domain) {
        return Err(AwsError::conflict(
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
        "CloudFrontDomain": format!("{domain}.auth.{}.amazoncognito.com", ctx.region)
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
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Domain is required"))?;

    let pool_id_entry = state.domain_pool_map.get(domain);

    if let Some(pool_id_ref) = pool_id_entry {
        let pool_id = pool_id_ref.clone();
        drop(pool_id_ref);

        let pool = state.user_pools.get(&pool_id).ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("User pool not found for domain: {domain}"),
            )
        })?;

        Ok(json!({
            "DomainDescription": {
                "Domain": domain,
                "UserPoolId": pool.id,
                "AWSAccountId": "",
                "CloudFrontDistribution": format!("{domain}.auth.{}.amazoncognito.com", ctx.region),
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
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let domain = input["Domain"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Domain is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
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
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let domain = input["Domain"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Domain is required"))?;

    // Verify pool exists
    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
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
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;
    pool_mut.domain = Some(domain.to_string());
    drop(pool_mut);

    state
        .domain_pool_map
        .insert(domain.to_string(), pool_id.to_string());

    info!(domain = %domain, pool_id = %pool_id, "Cognito: updated user pool domain");
    Ok(json!({
        "CloudFrontDomain": format!("{domain}.auth.{}.amazoncognito.com", ctx.region)
    }))
}
