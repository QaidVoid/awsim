use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{CognitoState, IdentityProvider};

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn idp_to_value(idp: &IdentityProvider) -> Value {
    json!({
        "UserPoolId": idp.user_pool_id,
        "ProviderName": idp.provider_name,
        "ProviderType": idp.provider_type,
        "ProviderDetails": idp.provider_details,
        "AttributeMapping": idp.attribute_mapping,
        "IdpIdentifiers": idp.idp_identifiers,
        "CreationDate": idp.creation_date,
        "LastModifiedDate": idp.last_modified_date
    })
}

fn parse_string_map(v: &Value) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            if let Some(s) = val.as_str() {
                map.insert(k.clone(), s.to_string());
            }
        }
    }
    map
}

// ---------------------------------------------------------------------------
// CreateIdentityProvider
// ---------------------------------------------------------------------------

pub fn create_identity_provider(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let provider_name = input["ProviderName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ProviderName is required"))?;
    let provider_type = input["ProviderType"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ProviderType is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if pool
        .identity_providers
        .iter()
        .any(|idp| idp.provider_name == provider_name)
    {
        return Err(AwsError::conflict(
            "DuplicateProviderException",
            format!("Identity provider already exists: {provider_name}"),
        ));
    }

    let now = now_epoch();
    let idp_identifiers: Vec<String> = input["IdpIdentifiers"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let idp = IdentityProvider {
        provider_name: provider_name.to_string(),
        provider_type: provider_type.to_string(),
        provider_details: parse_string_map(&input["ProviderDetails"]),
        attribute_mapping: parse_string_map(&input["AttributeMapping"]),
        idp_identifiers,
        creation_date: now,
        last_modified_date: now,
        user_pool_id: pool_id.to_string(),
    };

    let idp_value = idp_to_value(&idp);
    pool.identity_providers.push(idp);
    info!(provider_name = %provider_name, pool_id = %pool_id, "Cognito: created identity provider");

    Ok(json!({ "IdentityProvider": idp_value }))
}

// ---------------------------------------------------------------------------
// DescribeIdentityProvider
// ---------------------------------------------------------------------------

pub fn describe_identity_provider(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let provider_name = input["ProviderName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ProviderName is required"))?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let idp = pool
        .identity_providers
        .iter()
        .find(|idp| idp.provider_name == provider_name)
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Identity provider not found: {provider_name}"),
            )
        })?;

    Ok(json!({ "IdentityProvider": idp_to_value(idp) }))
}

// ---------------------------------------------------------------------------
// UpdateIdentityProvider
// ---------------------------------------------------------------------------

pub fn update_identity_provider(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let provider_name = input["ProviderName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ProviderName is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let now = now_epoch();
    let idp = pool
        .identity_providers
        .iter_mut()
        .find(|idp| idp.provider_name == provider_name)
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Identity provider not found: {provider_name}"),
            )
        })?;

    if !input["ProviderDetails"].is_null() {
        idp.provider_details = parse_string_map(&input["ProviderDetails"]);
    }
    if !input["AttributeMapping"].is_null() {
        idp.attribute_mapping = parse_string_map(&input["AttributeMapping"]);
    }
    if let Some(identifiers) = input["IdpIdentifiers"].as_array() {
        idp.idp_identifiers = identifiers
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    idp.last_modified_date = now;

    let idp_value = idp_to_value(idp);
    info!(provider_name = %provider_name, pool_id = %pool_id, "Cognito: updated identity provider");

    Ok(json!({ "IdentityProvider": idp_value }))
}

// ---------------------------------------------------------------------------
// DeleteIdentityProvider
// ---------------------------------------------------------------------------

pub fn delete_identity_provider(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let provider_name = input["ProviderName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ProviderName is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let len_before = pool.identity_providers.len();
    pool.identity_providers
        .retain(|idp| idp.provider_name != provider_name);

    if pool.identity_providers.len() == len_before {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity provider not found: {provider_name}"),
        ));
    }

    info!(provider_name = %provider_name, pool_id = %pool_id, "Cognito: deleted identity provider");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListIdentityProviders
// ---------------------------------------------------------------------------

pub fn list_identity_providers(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let providers: Vec<Value> = pool
        .identity_providers
        .iter()
        .map(|idp| {
            json!({
                "ProviderName": idp.provider_name,
                "ProviderType": idp.provider_type,
                "CreationDate": idp.creation_date,
                "LastModifiedDate": idp.last_modified_date
            })
        })
        .collect();

    Ok(json!({ "Providers": providers }))
}

// ---------------------------------------------------------------------------
// GetIdentityProviderByIdentifier
// ---------------------------------------------------------------------------

pub fn get_identity_provider_by_identifier(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let idp_identifier = input["IdpIdentifier"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdpIdentifier is required"))?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let idp = pool
        .identity_providers
        .iter()
        .find(|idp| {
            idp.idp_identifiers
                .iter()
                .any(|id| id == idp_identifier)
                || idp.provider_name == idp_identifier
        })
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Identity provider not found for identifier: {idp_identifier}"),
            )
        })?;

    Ok(json!({ "IdentityProvider": idp_to_value(idp) }))
}
