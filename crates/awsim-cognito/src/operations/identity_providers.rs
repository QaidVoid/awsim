use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{CognitoState, IdentityProvider, LinkedProvider};

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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let provider_name = input["ProviderName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ProviderName is required")
    })?;
    let provider_type = input["ProviderType"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ProviderType is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if pool
        .identity_providers
        .iter()
        .any(|idp| idp.provider_name == provider_name)
    {
        return Err(AwsError::bad_request(
            "DuplicateProviderException",
            format!("Identity provider already exists: {provider_name}"),
        ));
    }

    let now = now_epoch();
    let idp_identifiers: Vec<String> = input["IdpIdentifiers"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let provider_name = input["ProviderName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ProviderName is required")
    })?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let idp = pool
        .identity_providers
        .iter()
        .find(|idp| idp.provider_name == provider_name)
        .ok_or_else(|| {
            AwsError::service_not_found(
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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let provider_name = input["ProviderName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ProviderName is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
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
            AwsError::service_not_found(
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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let provider_name = input["ProviderName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ProviderName is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let len_before = pool.identity_providers.len();
    pool.identity_providers
        .retain(|idp| idp.provider_name != provider_name);

    if pool.identity_providers.len() == len_before {
        return Err(AwsError::service_not_found(
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
    use awsim_core::pagination::{cap_max_results, paginate};

    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;

    let limit = cap_max_results(input["MaxResults"].as_i64(), 60, 60);

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let mut providers: Vec<IdentityProvider> = pool.identity_providers.clone();
    providers.sort_by(|a, b| a.provider_name.cmp(&b.provider_name));

    let token = input["NextToken"].as_str();
    let page = paginate(providers, limit, token, |idp| idp.provider_name.clone())?;
    let provider_values: Vec<Value> = page
        .items
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

    let mut resp = json!({ "Providers": provider_values });
    if let Some(next) = page.next_token {
        resp["NextToken"] = json!(next);
    }
    Ok(resp)
}

// ---------------------------------------------------------------------------
// GetIdentityProviderByIdentifier
// ---------------------------------------------------------------------------

pub fn get_identity_provider_by_identifier(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let idp_identifier = input["IdpIdentifier"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "IdpIdentifier is required")
    })?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let idp = pool
        .identity_providers
        .iter()
        .find(|idp| {
            idp.idp_identifiers.iter().any(|id| id == idp_identifier)
                || idp.provider_name == idp_identifier
        })
        .ok_or_else(|| {
            AwsError::service_not_found(
                "ResourceNotFoundException",
                format!("Identity provider not found for identifier: {idp_identifier}"),
            )
        })?;

    Ok(json!({ "IdentityProvider": idp_to_value(idp) }))
}

// ---------------------------------------------------------------------------
// AdminLinkProviderForUser
// ---------------------------------------------------------------------------

pub fn admin_link_provider_for_user(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let dest = &input["DestinationUser"];
    let src = &input["SourceUser"];

    let dest_value = dest["ProviderAttributeValue"].as_str().ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterException",
            "DestinationUser.ProviderAttributeValue is required",
        )
    })?;
    let src_provider = src["ProviderName"].as_str().ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterException",
            "SourceUser.ProviderName is required",
        )
    })?;
    let src_attr_name = src["ProviderAttributeName"]
        .as_str()
        .unwrap_or("Cognito_Subject");
    let src_attr_value = src["ProviderAttributeValue"].as_str().ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterException",
            "SourceUser.ProviderAttributeValue is required",
        )
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    // Find destination user (by username or sub)
    let username = pool
        .users
        .keys()
        .find(|k| {
            k.as_str() == dest_value || pool.users.get(*k).is_some_and(|u| u.sub == dest_value)
        })
        .cloned()
        .ok_or_else(|| {
            AwsError::service_not_found(
                "UserNotFoundException",
                format!("Destination user not found: {dest_value}"),
            )
        })?;

    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;

    // Remove any existing link for this provider
    user.linked_providers
        .retain(|lp| lp.provider_name != src_provider);
    user.linked_providers.push(LinkedProvider {
        provider_name: src_provider.to_string(),
        provider_attribute_name: src_attr_name.to_string(),
        provider_attribute_value: src_attr_value.to_string(),
    });

    info!(username = %username, pool_id = %pool_id, provider = %src_provider, "Cognito: linked provider for user");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminDisableProviderForUser
// ---------------------------------------------------------------------------

pub fn admin_disable_provider_for_user(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let user_input = &input["User"];
    let provider_name = user_input["ProviderName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "User.ProviderName is required")
    })?;
    let provider_attr_value = user_input["ProviderAttributeValue"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterException",
                "User.ProviderAttributeValue is required",
            )
        })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    // Find user that has this provider link
    let mut found = false;
    for user in pool.users.values_mut() {
        let len_before = user.linked_providers.len();
        user.linked_providers.retain(|lp| {
            !(lp.provider_name == provider_name
                && lp.provider_attribute_value == provider_attr_value)
        });
        if user.linked_providers.len() < len_before {
            found = true;
            break;
        }
    }

    if !found {
        return Err(AwsError::service_not_found(
            "UserNotFoundException",
            format!("No user found with provider {provider_name} value {provider_attr_value}"),
        ));
    }

    info!(pool_id = %pool_id, provider = %provider_name, "Cognito: disabled provider for user");
    Ok(json!({}))
}
