use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{CognitoState, ManagedLoginBranding, UiCustomization};

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn branding_to_value(b: &ManagedLoginBranding) -> Value {
    json!({
        "ManagedLoginBrandingId": b.branding_id,
        "UserPoolId": b.user_pool_id,
        "ClientId": b.client_id,
        "Settings": b.settings,
        "Assets": b.assets,
        "CreationDate": b.creation_date,
        "LastModifiedDate": b.last_modified_date
    })
}

// ---------------------------------------------------------------------------
// SetUICustomization
// ---------------------------------------------------------------------------

pub fn set_ui_customization(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let client_key = input["ClientId"].as_str().unwrap_or("pool").to_string();
    let css = input["CSS"].as_str().map(String::from);
    let image_url = input["ImageFile"].as_str().map(String::from);
    let now = now_epoch();

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let entry = pool.ui_customizations.entry(client_key.clone()).or_insert(UiCustomization {
        css: None,
        image_url: None,
        creation_date: now,
        last_modified_date: now,
    });
    if css.is_some() { entry.css = css; }
    if image_url.is_some() { entry.image_url = image_url; }
    entry.last_modified_date = now;

    info!(pool_id = %pool_id, client_key = %client_key, "Cognito: set UI customization");
    Ok(json!({
        "UICustomization": {
            "UserPoolId": pool_id,
            "ClientId": if client_key == "pool" { Value::Null } else { Value::String(client_key) },
            "CSS": entry.css,
            "ImageUrl": entry.image_url,
            "CreationDate": entry.creation_date,
            "LastModifiedDate": entry.last_modified_date
        }
    }))
}

// ---------------------------------------------------------------------------
// GetUICustomization
// ---------------------------------------------------------------------------

pub fn get_ui_customization(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let client_key = input["ClientId"].as_str().unwrap_or("pool").to_string();

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let (css, image_url, creation_date, last_modified_date) = if let Some(c) = pool.ui_customizations.get(&client_key) {
        (c.css.clone(), c.image_url.clone(), c.creation_date, c.last_modified_date)
    } else {
        (None, None, 0u64, 0u64)
    };

    Ok(json!({
        "UICustomization": {
            "UserPoolId": pool_id,
            "ClientId": if client_key == "pool" { Value::Null } else { Value::String(client_key) },
            "CSS": css,
            "ImageUrl": image_url,
            "CreationDate": creation_date,
            "LastModifiedDate": last_modified_date
        }
    }))
}

// ---------------------------------------------------------------------------
// CreateManagedLoginBranding
// ---------------------------------------------------------------------------

pub fn create_managed_login_branding(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let client_id = input["ClientId"].as_str().map(String::from);
    let settings = input["Settings"].clone();
    let assets: Vec<Value> = input["Assets"].as_array().cloned().unwrap_or_default();
    let now = now_epoch();
    let branding_id = Uuid::new_v4().to_string();

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let branding = ManagedLoginBranding {
        branding_id: branding_id.clone(),
        user_pool_id: pool_id.to_string(),
        client_id,
        settings,
        assets,
        creation_date: now,
        last_modified_date: now,
    };

    let val = branding_to_value(&branding);
    pool.managed_login_brandings.push(branding);

    info!(pool_id = %pool_id, branding_id = %branding_id, "Cognito: created managed login branding");
    Ok(json!({ "ManagedLoginBranding": val }))
}

// ---------------------------------------------------------------------------
// DescribeManagedLoginBranding
// ---------------------------------------------------------------------------

pub fn describe_managed_login_branding(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let branding_id = input["ManagedLoginBrandingId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ManagedLoginBrandingId is required"))?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let branding = pool.managed_login_brandings.iter().find(|b| b.branding_id == branding_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Branding not found: {branding_id}")))?;

    Ok(json!({ "ManagedLoginBranding": branding_to_value(branding) }))
}

// ---------------------------------------------------------------------------
// DescribeManagedLoginBrandingByClient
// ---------------------------------------------------------------------------

pub fn describe_managed_login_branding_by_client(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let branding = pool.managed_login_brandings.iter()
        .find(|b| b.client_id.as_deref() == Some(client_id))
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("No branding found for client: {client_id}")))?;

    Ok(json!({ "ManagedLoginBranding": branding_to_value(branding) }))
}

// ---------------------------------------------------------------------------
// UpdateManagedLoginBranding
// ---------------------------------------------------------------------------

pub fn update_managed_login_branding(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let branding_id = input["ManagedLoginBrandingId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ManagedLoginBrandingId is required"))?;
    let now = now_epoch();

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let branding = pool.managed_login_brandings.iter_mut().find(|b| b.branding_id == branding_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Branding not found: {branding_id}")))?;

    if !input["Settings"].is_null() {
        branding.settings = input["Settings"].clone();
    }
    if let Some(assets) = input["Assets"].as_array() {
        branding.assets = assets.clone();
    }
    branding.last_modified_date = now;

    let val = branding_to_value(branding);
    info!(pool_id = %pool_id, branding_id = %branding_id, "Cognito: updated managed login branding");
    Ok(json!({ "ManagedLoginBranding": val }))
}

// ---------------------------------------------------------------------------
// DeleteManagedLoginBranding
// ---------------------------------------------------------------------------

pub fn delete_managed_login_branding(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let branding_id = input["ManagedLoginBrandingId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ManagedLoginBrandingId is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let len_before = pool.managed_login_brandings.len();
    pool.managed_login_brandings.retain(|b| b.branding_id != branding_id);
    if pool.managed_login_brandings.len() == len_before {
        return Err(AwsError::not_found("ResourceNotFoundException", format!("Branding not found: {branding_id}")));
    }

    info!(pool_id = %pool_id, branding_id = %branding_id, "Cognito: deleted managed login branding");
    Ok(json!({}))
}
