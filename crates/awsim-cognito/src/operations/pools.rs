use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use rand::Rng;
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{CognitoState, PasswordPolicy, UserPool, UserPoolClient};

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn pool_arn(region: &str, account_id: &str, pool_id: &str) -> String {
    format!("arn:aws:cognito-idp:{region}:{account_id}:userpool/{pool_id}")
}

/// Generate a random 52-char alphanumeric client secret.
fn generate_client_secret() -> String {
    let mut rng = rand::thread_rng();
    (0..52)
        .map(|_| {
            let idx = rng.gen_range(0..62);
            match idx {
                0..=25 => (b'a' + idx) as char,
                26..=51 => (b'A' + idx - 26) as char,
                _ => (b'0' + idx - 52) as char,
            }
        })
        .collect()
}

fn password_policy_to_value(p: &PasswordPolicy) -> Value {
    json!({
        "MinimumLength": p.minimum_length,
        "RequireLowercase": p.require_lowercase,
        "RequireUppercase": p.require_uppercase,
        "RequireNumbers": p.require_numbers,
        "RequireSymbols": p.require_symbols,
        "TemporaryPasswordValidityDays": p.temporary_password_validity_days
    })
}

fn client_to_value(client: &UserPoolClient) -> Value {
    json!({
        "UserPoolId": client.user_pool_id,
        "ClientName": client.client_name,
        "ClientId": client.client_id,
        "ClientSecret": client.client_secret,
        "ExplicitAuthFlows": client.explicit_auth_flows,
        "CallbackURLs": client.callback_urls,
        "LogoutURLs": client.logout_urls,
        "AllowedOAuthFlows": client.allowed_oauth_flows,
        "AllowedOAuthScopes": client.allowed_oauth_scopes,
        "SupportedIdentityProviders": client.supported_identity_providers,
        "AccessTokenValidity": client.access_token_validity,
        "IdTokenValidity": client.id_token_validity,
        "RefreshTokenValidity": client.refresh_token_validity,
        "CreationDate": client.created_date,
        "LastModifiedDate": client.created_date
    })
}

// ---------------------------------------------------------------------------
// CreateUserPool
// ---------------------------------------------------------------------------

pub fn create_user_pool(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_name = input["PoolName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PoolName is required"))?;

    let random = &Uuid::new_v4().to_string()[..8];
    let pool_id = format!("{0}_{1}", ctx.region, random);
    let arn = pool_arn(&ctx.region, &ctx.account_id, &pool_id);
    let now = now_epoch();

    // Parse policies from input
    let policies = parse_password_policy(&input["Policies"]["PasswordPolicy"]);

    let mfa_configuration = input["MfaConfiguration"]
        .as_str()
        .unwrap_or("OFF")
        .to_string();

    let auto_verified_attributes: Vec<String> = input["AutoVerifiedAttributes"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let lambda_config = parse_lambda_config(&input["LambdaConfig"]);

    let pool = UserPool {
        id: pool_id.clone(),
        name: pool_name.to_string(),
        arn: arn.clone(),
        clients: HashMap::new(),
        users: HashMap::new(),
        groups: HashMap::new(),
        created_date: now,
        policies,
        mfa_configuration,
        software_token_mfa_enabled: false,
        auto_verified_attributes,
        lambda_config,
        schema: Vec::new(),
        email_configuration: None,
        domain: None,
        resource_servers: Vec::new(),
        identity_providers: Vec::new(),
        tags: HashMap::new(),
        ui_customizations: HashMap::new(),
        managed_login_brandings: Vec::new(),
        risk_configurations: Vec::new(),
        import_jobs: Vec::new(),
        log_delivery_configuration: None,
    };

    info!(pool_id = %pool_id, "Cognito: created user pool");
    state.user_pools.insert(pool_id.clone(), pool);

    Ok(json!({
        "UserPool": {
            "Id": pool_id,
            "Name": pool_name,
            "Arn": arn,
            "Status": "Active",
            "CreationDate": now,
            "LastModifiedDate": now
        }
    }))
}

// ---------------------------------------------------------------------------
// DeleteUserPool
// ---------------------------------------------------------------------------

pub fn delete_user_pool(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    if state.user_pools.remove(pool_id).is_none() {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        ));
    }

    info!(pool_id = %pool_id, "Cognito: deleted user pool");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DescribeUserPool
// ---------------------------------------------------------------------------

pub fn describe_user_pool(
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

    Ok(json!({
        "UserPool": {
            "Id": pool.id,
            "Name": pool.name,
            "Arn": pool.arn,
            "Status": "Active",
            "CreationDate": pool.created_date,
            "LastModifiedDate": pool.created_date,
            "MfaConfiguration": pool.mfa_configuration,
            "AutoVerifiedAttributes": pool.auto_verified_attributes,
            "Policies": {
                "PasswordPolicy": password_policy_to_value(&pool.policies)
            },
            "Domain": pool.domain
        }
    }))
}

// ---------------------------------------------------------------------------
// ListUserPools
// ---------------------------------------------------------------------------

pub fn list_user_pools(
    state: &CognitoState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pools: Vec<Value> = state
        .user_pools
        .iter()
        .map(|e| {
            json!({
                "Id": e.id,
                "Name": e.name,
                "Status": "Active",
                "CreationDate": e.created_date,
                "LastModifiedDate": e.created_date
            })
        })
        .collect();

    Ok(json!({ "UserPools": pools }))
}

// ---------------------------------------------------------------------------
// UpdateUserPool
// ---------------------------------------------------------------------------

pub fn update_user_pool(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if !input["Policies"]["PasswordPolicy"].is_null() {
        pool.policies = parse_password_policy(&input["Policies"]["PasswordPolicy"]);
    }

    if let Some(mfa) = input["MfaConfiguration"].as_str() {
        pool.mfa_configuration = mfa.to_string();
    }

    if let Some(attrs) = input["AutoVerifiedAttributes"].as_array() {
        pool.auto_verified_attributes = attrs
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    if !input["LambdaConfig"].is_null() {
        pool.lambda_config = parse_lambda_config(&input["LambdaConfig"]);
    }

    info!(pool_id = %pool_id, "Cognito: updated user pool");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// CreateUserPoolClient
// ---------------------------------------------------------------------------

pub fn create_user_pool_client(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let client_name = input["ClientName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientName is required"))?;

    let explicit_auth_flows: Vec<String> = input["ExplicitAuthFlows"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let generate_secret = input["GenerateSecret"].as_bool().unwrap_or(false);
    let client_secret = if generate_secret {
        Some(generate_client_secret())
    } else {
        None
    };

    let callback_urls: Vec<String> = input["CallbackURLs"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let logout_urls: Vec<String> = input["LogoutURLs"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let allowed_oauth_flows: Vec<String> = input["AllowedOAuthFlows"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let allowed_oauth_scopes: Vec<String> = input["AllowedOAuthScopes"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let supported_identity_providers: Vec<String> = input["SupportedIdentityProviders"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let access_token_validity = input["AccessTokenValidity"].as_u64().unwrap_or(3600);
    let id_token_validity = input["IdTokenValidity"].as_u64().unwrap_or(3600);
    let refresh_token_validity = input["RefreshTokenValidity"].as_u64().unwrap_or(2_592_000);

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let client_id = Uuid::new_v4().to_string().replace('-', "")[..26].to_string();
    let now = now_epoch();

    let client = UserPoolClient {
        client_id: client_id.clone(),
        client_name: client_name.to_string(),
        user_pool_id: pool_id.to_string(),
        explicit_auth_flows,
        created_date: now,
        client_secret: client_secret.clone(),
        callback_urls,
        logout_urls,
        allowed_oauth_flows,
        allowed_oauth_scopes,
        supported_identity_providers,
        access_token_validity,
        id_token_validity,
        refresh_token_validity,
    };

    pool.clients.insert(client_id.clone(), client);

    info!(pool_id = %pool_id, client_id = %client_id, "Cognito: created user pool client");

    Ok(json!({
        "UserPoolClient": {
            "UserPoolId": pool_id,
            "ClientName": client_name,
            "ClientId": client_id,
            "ClientSecret": client_secret,
            "CreationDate": now,
            "LastModifiedDate": now
        }
    }))
}

// ---------------------------------------------------------------------------
// DescribeUserPoolClient
// ---------------------------------------------------------------------------

pub fn describe_user_pool_client(
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
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let client = pool.clients.get(client_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Client not found: {client_id}"),
        )
    })?;

    Ok(json!({ "UserPoolClient": client_to_value(client) }))
}

// ---------------------------------------------------------------------------
// DeleteUserPoolClient
// ---------------------------------------------------------------------------

pub fn delete_user_pool_client(
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

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if pool.clients.remove(client_id).is_none() {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Client not found: {client_id}"),
        ));
    }

    info!(pool_id = %pool_id, client_id = %client_id, "Cognito: deleted user pool client");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListUserPoolClients
// ---------------------------------------------------------------------------

pub fn list_user_pool_clients(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    let max_results = input["MaxResults"].as_u64().unwrap_or(60) as usize;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let clients: Vec<Value> = pool
        .clients
        .values()
        .take(max_results)
        .map(|c| {
            json!({
                "ClientId": c.client_id,
                "ClientName": c.client_name,
                "UserPoolId": c.user_pool_id
            })
        })
        .collect();

    Ok(json!({ "UserPoolClients": clients }))
}

// ---------------------------------------------------------------------------
// UpdateUserPoolClient
// ---------------------------------------------------------------------------

pub fn update_user_pool_client(
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

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let client = pool.clients.get_mut(client_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Client not found: {client_id}"),
        )
    })?;

    if let Some(name) = input["ClientName"].as_str() {
        client.client_name = name.to_string();
    }

    if let Some(flows) = input["ExplicitAuthFlows"].as_array() {
        client.explicit_auth_flows = flows
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    if let Some(urls) = input["CallbackURLs"].as_array() {
        client.callback_urls = urls
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    if let Some(urls) = input["LogoutURLs"].as_array() {
        client.logout_urls = urls
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    if let Some(flows) = input["AllowedOAuthFlows"].as_array() {
        client.allowed_oauth_flows = flows
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    if let Some(scopes) = input["AllowedOAuthScopes"].as_array() {
        client.allowed_oauth_scopes = scopes
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    if let Some(idps) = input["SupportedIdentityProviders"].as_array() {
        client.supported_identity_providers = idps
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    if let Some(v) = input["AccessTokenValidity"].as_u64() {
        client.access_token_validity = v;
    }
    if let Some(v) = input["IdTokenValidity"].as_u64() {
        client.id_token_validity = v;
    }
    if let Some(v) = input["RefreshTokenValidity"].as_u64() {
        client.refresh_token_validity = v;
    }

    let client_value = client_to_value(client);
    info!(pool_id = %pool_id, client_id = %client_id, "Cognito: updated user pool client");

    Ok(json!({ "UserPoolClient": client_value }))
}

// ---------------------------------------------------------------------------
// AddCustomAttributes
// ---------------------------------------------------------------------------

pub fn add_custom_attributes(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if let Some(attrs) = input["CustomAttributes"].as_array() {
        for attr in attrs {
            let name = attr["Name"]
                .as_str()
                .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Attribute Name is required"))?;
            let full_name = if name.starts_with("custom:") {
                name.to_string()
            } else {
                format!("custom:{name}")
            };

            pool.schema.push(crate::state::SchemaAttribute {
                name: full_name,
                attribute_data_type: attr["AttributeDataType"]
                    .as_str()
                    .unwrap_or("String")
                    .to_string(),
                required: attr["Required"].as_bool().unwrap_or(false),
                mutable: attr["Mutable"].as_bool().unwrap_or(true),
            });
        }
    }

    info!(pool_id = %pool_id, "Cognito: added custom attributes");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_password_policy(v: &Value) -> PasswordPolicy {
    let default = PasswordPolicy::default();
    PasswordPolicy {
        minimum_length: v["MinimumLength"].as_u64().unwrap_or(default.minimum_length as u64) as u32,
        require_lowercase: v["RequireLowercase"]
            .as_bool()
            .unwrap_or(default.require_lowercase),
        require_uppercase: v["RequireUppercase"]
            .as_bool()
            .unwrap_or(default.require_uppercase),
        require_numbers: v["RequireNumbers"]
            .as_bool()
            .unwrap_or(default.require_numbers),
        require_symbols: v["RequireSymbols"]
            .as_bool()
            .unwrap_or(default.require_symbols),
        temporary_password_validity_days: v["TemporaryPasswordValidityDays"]
            .as_u64()
            .unwrap_or(default.temporary_password_validity_days as u64)
            as u32,
    }
}

fn parse_lambda_config(v: &Value) -> HashMap<String, String> {
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
// GetSigningCertificate
// ---------------------------------------------------------------------------

pub fn get_signing_certificate(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    let _pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    // Return a stub PEM certificate
    let fake_pem = "-----BEGIN CERTIFICATE-----\n\
        MIICpDCCAYwCCQDU+pQ4pHgSpDANBgkqhkiG9w0BAQsFADAUMRIwEAYDVQQDDAls\n\
        b2NhbGhvc3QwHhcNMjQwMTAxMDAwMDAwWhcNMjUwMTAxMDAwMDAwWjAUMRIwEAYD\n\
        VQQDDAlsb2NhbGhvc3QwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQC7\n\
        o4qne60TB3pNjMEm9+MnEL4skPmNgBsixiPEOThqxhV2IVNkMcPGEMaFOfFsaXHf\n\
        awsim-fake-cognito-signing-certificate-for-local-development-only\n\
        -----END CERTIFICATE-----";

    Ok(json!({ "Certificate": fake_pem }))
}

// ---------------------------------------------------------------------------
// GetLogDeliveryConfiguration
// ---------------------------------------------------------------------------

pub fn get_log_delivery_configuration(
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

    let log_configs: Vec<serde_json::Value> = pool.log_delivery_configuration
        .as_ref()
        .map(|ldc| ldc.log_configurations.clone())
        .unwrap_or_default();

    Ok(json!({
        "LogDeliveryConfiguration": {
            "UserPoolId": pool_id,
            "LogConfigurations": log_configs
        }
    }))
}

// ---------------------------------------------------------------------------
// SetLogDeliveryConfiguration
// ---------------------------------------------------------------------------

pub fn set_log_delivery_configuration(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    use crate::state::LogDeliveryConfiguration;

    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let log_configs: Vec<serde_json::Value> = input["LogConfigurations"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    pool.log_delivery_configuration = Some(LogDeliveryConfiguration {
        log_configurations: log_configs.clone(),
    });

    info!(pool_id = %pool_id, "Cognito: set log delivery configuration");
    Ok(json!({
        "LogDeliveryConfiguration": {
            "UserPoolId": pool_id,
            "LogConfigurations": log_configs
        }
    }))
}
