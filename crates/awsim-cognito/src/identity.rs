//! Cognito Identity Pools (Federated Identities)
//!
//! Signing name:  `cognito-identity`
//! Target prefix: `AWSCognitoIdentityService`

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use dashmap::DashMap;
use serde_json::{Value, json};
use tracing::debug;

// ---------------------------------------------------------------------------
// State types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CognitoProvider {
    pub client_id: String,
    /// e.g. `cognito-idp.us-east-1.amazonaws.com/us-east-1_XXXXX`
    pub provider_name: String,
    pub server_side_token_check: bool,
}

#[derive(Debug, Clone)]
pub struct IdentityPool {
    pub id: String,
    pub name: String,
    pub allow_unauthenticated: bool,
    pub cognito_identity_providers: Vec<CognitoProvider>,
    pub supported_login_providers: HashMap<String, String>,
    /// "authenticated" → role ARN, "unauthenticated" → role ARN
    pub roles: HashMap<String, String>,
    pub role_mappings: HashMap<String, Value>,
    pub developer_provider_name: Option<String>,
    pub created_date: String,
}

#[derive(Debug, Clone)]
pub struct Identity {
    pub identity_id: String,
    /// Provider names the identity has logged in with.
    pub logins: Vec<String>,
    pub creation_date: String,
    /// For developer identities: developer user identifiers.
    pub developer_user_identifiers: Vec<String>,
}

#[derive(Debug, Default)]
pub struct IdentityPoolState {
    /// pool_id → IdentityPool
    pub pools: DashMap<String, IdentityPool>,
    /// identity_id → Identity
    pub identities: DashMap<String, Identity>,
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

pub struct CognitoIdentityService {
    state: AccountRegionStore<IdentityPoolState>,
}

impl CognitoIdentityService {
    pub fn new() -> Self {
        Self {
            state: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<IdentityPoolState> {
        self.state.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for CognitoIdentityService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for CognitoIdentityService {
    fn service_name(&self) -> &str {
        "cognito-identity"
    }

    fn signing_name(&self) -> &str {
        "cognito-identity"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_1
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "CognitoIdentity request");
        let state = self.get_state(ctx);

        match operation {
            "CreateIdentityPool" => create_identity_pool(&state, &input, ctx),
            "DeleteIdentityPool" => delete_identity_pool(&state, &input),
            "DescribeIdentityPool" => describe_identity_pool(&state, &input),
            "UpdateIdentityPool" => update_identity_pool(&state, &input),
            "ListIdentityPools" => list_identity_pools(&state, &input),
            "GetId" => get_id(&state, &input, ctx),
            "GetCredentialsForIdentity" => get_credentials_for_identity(&state, &input),
            "GetOpenIdToken" => get_open_id_token(&state, &input, ctx),
            "GetOpenIdTokenForDeveloperIdentity" => {
                get_open_id_token_for_developer_identity(&state, &input, ctx)
            }
            "SetIdentityPoolRoles" => set_identity_pool_roles(&state, &input),
            "GetIdentityPoolRoles" => get_identity_pool_roles(&state, &input),
            "LookupDeveloperIdentity" => lookup_developer_identity(&state, &input),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: current ISO-8601 timestamp
// ---------------------------------------------------------------------------

fn now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    unix_to_iso8601(secs)
}

fn expiration_iso8601(duration_secs: u64) -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        + duration_secs;
    unix_to_iso8601(secs)
}

fn unix_to_iso8601(secs: u64) -> String {
    let mut remaining = secs;
    let seconds = remaining % 60;
    remaining /= 60;
    let minutes = remaining % 60;
    remaining /= 60;
    let hours = remaining % 24;
    remaining /= 24;
    let (year, month, day) = days_to_ymd(remaining);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z = days + 719468;
    let era = z / 146097;
    let doe = z % 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ---------------------------------------------------------------------------
// Helper: fake temporary credentials (same pattern as STS)
// ---------------------------------------------------------------------------

fn fake_access_key_id() -> String {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let suffix: String = id[..16].to_uppercase();
    format!("ASIA{suffix}")
}

fn fake_secret_access_key() -> String {
    let u1 = uuid::Uuid::new_v4().simple().to_string();
    let u2 = uuid::Uuid::new_v4().simple().to_string();
    format!("{u1}{u2}")[..40].to_string()
}

fn fake_session_token() -> String {
    let parts: Vec<String> = (0..4)
        .map(|_| uuid::Uuid::new_v4().simple().to_string())
        .collect();
    format!(
        "FwoGZXIvYXdzEAwaDAwsim{}//////////wEaD{}Aw{}Q{}",
        parts[0], parts[1], parts[2], parts[3]
    )
}

fn generate_credentials(duration_secs: u64) -> Value {
    json!({
        "AccessKeyId":     fake_access_key_id(),
        "SecretKey":       fake_secret_access_key(),
        "SessionToken":    fake_session_token(),
        "Expiration":      expiration_iso8601(duration_secs),
    })
}

// ---------------------------------------------------------------------------
// Helper: fake OpenID token (structurally valid JWT)
// ---------------------------------------------------------------------------

fn fake_open_id_token(identity_id: &str, pool_id: &str) -> String {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let header = json!({"alg": "RS256", "typ": "JWT"});
    let payload = json!({
        "sub": identity_id,
        "aud": pool_id,
        "iss": "https://cognito-identity.amazonaws.com",
        "iat": now,
        "exp": now + 3600,
    });

    let h = URL_SAFE_NO_PAD.encode(header.to_string().as_bytes());
    let p = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
    let sig = URL_SAFE_NO_PAD.encode(b"awsim-identity-signature");
    format!("{h}.{p}.{sig}")
}

// ---------------------------------------------------------------------------
// Helper: resolve pool or return error
// ---------------------------------------------------------------------------

fn get_pool<'a>(
    state: &'a IdentityPoolState,
    pool_id: &str,
) -> Result<dashmap::mapref::one::Ref<'a, String, IdentityPool>, AwsError> {
    state.pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity pool not found: {pool_id}"),
        )
    })
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

/// CreateIdentityPool
fn create_identity_pool(
    state: &IdentityPoolState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["IdentityPoolName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityPoolName is required"))?;

    let allow_unauthenticated = input["AllowUnauthenticatedIdentities"]
        .as_bool()
        .unwrap_or(false);

    let pool_uuid = uuid::Uuid::new_v4().to_string();
    let pool_id = format!("{}:{}", ctx.region, pool_uuid);

    // Parse CognitoIdentityProviders
    let providers: Vec<CognitoProvider> = input["CognitoIdentityProviders"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|p| CognitoProvider {
            client_id: p["ClientId"].as_str().unwrap_or("").to_string(),
            provider_name: p["ProviderName"].as_str().unwrap_or("").to_string(),
            server_side_token_check: p["ServerSideTokenCheck"].as_bool().unwrap_or(false),
        })
        .collect();

    // Parse SupportedLoginProviders (map of provider → app id/key)
    let supported_login_providers: HashMap<String, String> = input["SupportedLoginProviders"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default();

    let developer_provider_name = input["DeveloperProviderName"]
        .as_str()
        .map(String::from);

    let created_date = now_iso8601();

    let pool = IdentityPool {
        id: pool_id.clone(),
        name: name.to_string(),
        allow_unauthenticated,
        cognito_identity_providers: providers.clone(),
        supported_login_providers: supported_login_providers.clone(),
        roles: HashMap::new(),
        role_mappings: HashMap::new(),
        developer_provider_name: developer_provider_name.clone(),
        created_date: created_date.clone(),
    };

    state.pools.insert(pool_id.clone(), pool);

    let providers_json: Vec<Value> = providers
        .iter()
        .map(|p| {
            json!({
                "ClientId": p.client_id,
                "ProviderName": p.provider_name,
                "ServerSideTokenCheck": p.server_side_token_check,
            })
        })
        .collect();

    let slp_json: Value = supported_login_providers
        .iter()
        .fold(json!({}), |mut acc, (k, v)| {
            acc[k] = Value::String(v.clone());
            acc
        });

    let mut resp = json!({
        "IdentityPoolId":                    pool_id,
        "IdentityPoolName":                  name,
        "AllowUnauthenticatedIdentities":    allow_unauthenticated,
        "CognitoIdentityProviders":          providers_json,
        "SupportedLoginProviders":           slp_json,
        "CreationDate":                      created_date,
    });

    if let Some(dpn) = developer_provider_name {
        resp["DeveloperProviderName"] = Value::String(dpn);
    }

    Ok(resp)
}

/// DeleteIdentityPool
fn delete_identity_pool(state: &IdentityPoolState, input: &Value) -> Result<Value, AwsError> {
    let pool_id = input["IdentityPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityPoolId is required"))?;

    state.pools.remove(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity pool not found: {pool_id}"),
        )
    })?;

    Ok(json!({}))
}

/// DescribeIdentityPool
fn describe_identity_pool(state: &IdentityPoolState, input: &Value) -> Result<Value, AwsError> {
    let pool_id = input["IdentityPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityPoolId is required"))?;

    let pool = get_pool(state, pool_id)?;
    Ok(pool_to_json(&pool))
}

/// UpdateIdentityPool
fn update_identity_pool(state: &IdentityPoolState, input: &Value) -> Result<Value, AwsError> {
    let pool_id = input["IdentityPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityPoolId is required"))?;

    let mut pool = state.pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity pool not found: {pool_id}"),
        )
    })?;

    if let Some(name) = input["IdentityPoolName"].as_str() {
        pool.name = name.to_string();
    }
    if let Some(allow) = input["AllowUnauthenticatedIdentities"].as_bool() {
        pool.allow_unauthenticated = allow;
    }
    if let Some(providers) = input["CognitoIdentityProviders"].as_array() {
        pool.cognito_identity_providers = providers
            .iter()
            .map(|p| CognitoProvider {
                client_id: p["ClientId"].as_str().unwrap_or("").to_string(),
                provider_name: p["ProviderName"].as_str().unwrap_or("").to_string(),
                server_side_token_check: p["ServerSideTokenCheck"].as_bool().unwrap_or(false),
            })
            .collect();
    }
    if let Some(slp) = input["SupportedLoginProviders"].as_object() {
        pool.supported_login_providers = slp
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
            .collect();
    }
    if let Some(dpn) = input["DeveloperProviderName"].as_str() {
        pool.developer_provider_name = Some(dpn.to_string());
    }

    Ok(pool_to_json(&pool))
}

/// ListIdentityPools
fn list_identity_pools(state: &IdentityPoolState, input: &Value) -> Result<Value, AwsError> {
    let max_results = input["MaxResults"].as_u64().unwrap_or(60) as usize;

    let pools: Vec<Value> = state
        .pools
        .iter()
        .take(max_results)
        .map(|e| {
            json!({
                "IdentityPoolId":   e.value().id,
                "IdentityPoolName": e.value().name,
            })
        })
        .collect();

    Ok(json!({ "IdentityPools": pools }))
}

/// GetId — get or create an identity for the caller.
fn get_id(
    state: &IdentityPoolState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["IdentityPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityPoolId is required"))?;

    let pool = get_pool(state, pool_id)?;

    let logins = input["Logins"].as_object();
    let is_authenticated = logins.map(|m| !m.is_empty()).unwrap_or(false);

    if !is_authenticated && !pool.allow_unauthenticated {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Unauthenticated access is not supported for this identity pool",
        ));
    }

    // Collect provider names from logins map
    let login_providers: Vec<String> = logins
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();

    // For authenticated identities, check if one already exists for this provider set.
    // For simplicity, always create a new identity (real Cognito deduplicates by token).
    let identity_id = format!("{}:{}", ctx.region, uuid::Uuid::new_v4());

    let identity = Identity {
        identity_id: identity_id.clone(),
        logins: login_providers,
        creation_date: now_iso8601(),
        developer_user_identifiers: vec![],
    };

    state.identities.insert(identity_id.clone(), identity);

    Ok(json!({ "IdentityId": identity_id }))
}

/// GetCredentialsForIdentity
fn get_credentials_for_identity(
    state: &IdentityPoolState,
    input: &Value,
) -> Result<Value, AwsError> {
    let identity_id = input["IdentityId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityId is required"))?;

    // Validate the identity exists
    if !state.identities.contains_key(identity_id) {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity not found: {identity_id}"),
        ));
    }

    let credentials = generate_credentials(3600);

    Ok(json!({
        "IdentityId":  identity_id,
        "Credentials": credentials,
    }))
}

/// GetOpenIdToken
fn get_open_id_token(
    state: &IdentityPoolState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity_id = input["IdentityId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityId is required"))?;

    if !state.identities.contains_key(identity_id) {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity not found: {identity_id}"),
        ));
    }

    // The pool_id can be inferred from the identity_id region prefix
    let pool_id = format!("{}:pool", ctx.region);
    let token = fake_open_id_token(identity_id, &pool_id);

    Ok(json!({
        "Token":      token,
        "IdentityId": identity_id,
    }))
}

/// GetOpenIdTokenForDeveloperIdentity
fn get_open_id_token_for_developer_identity(
    state: &IdentityPoolState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["IdentityPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityPoolId is required"))?;

    get_pool(state, pool_id)?;

    let logins = input["Logins"]
        .as_object()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Logins is required"))?;

    // Extract developer user identifier from the logins map
    let dev_user_identifier = logins
        .values()
        .next()
        .and_then(|v| v.as_str())
        .unwrap_or("developer-user");

    // Use the provided IdentityId or generate a new one
    let identity_id = if let Some(existing_id) = input["IdentityId"].as_str() {
        existing_id.to_string()
    } else {
        format!("{}:{}", ctx.region, uuid::Uuid::new_v4())
    };

    // Upsert the identity with developer user identifiers
    {
        let mut identity = state
            .identities
            .entry(identity_id.clone())
            .or_insert_with(|| Identity {
                identity_id: identity_id.clone(),
                logins: logins.keys().cloned().collect(),
                creation_date: now_iso8601(),
                developer_user_identifiers: vec![],
            });

        if !identity
            .developer_user_identifiers
            .contains(&dev_user_identifier.to_string())
        {
            identity
                .developer_user_identifiers
                .push(dev_user_identifier.to_string());
        }
    }

    let token = fake_open_id_token(&identity_id, pool_id);

    Ok(json!({
        "Token":      token,
        "IdentityId": identity_id,
    }))
}

/// SetIdentityPoolRoles
fn set_identity_pool_roles(state: &IdentityPoolState, input: &Value) -> Result<Value, AwsError> {
    let pool_id = input["IdentityPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityPoolId is required"))?;

    let mut pool = state.pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity pool not found: {pool_id}"),
        )
    })?;

    if let Some(roles_obj) = input["Roles"].as_object() {
        for (role_type, arn) in roles_obj {
            if let Some(arn_str) = arn.as_str() {
                pool.roles.insert(role_type.clone(), arn_str.to_string());
            }
        }
    }

    if let Some(rm_obj) = input["RoleMappings"].as_object() {
        for (provider, mapping) in rm_obj {
            pool.role_mappings.insert(provider.clone(), mapping.clone());
        }
    }

    Ok(json!({}))
}

/// GetIdentityPoolRoles
fn get_identity_pool_roles(state: &IdentityPoolState, input: &Value) -> Result<Value, AwsError> {
    let pool_id = input["IdentityPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityPoolId is required"))?;

    let pool = get_pool(state, pool_id)?;

    let roles_json: Value = pool
        .roles
        .iter()
        .fold(json!({}), |mut acc, (k, v)| {
            acc[k] = Value::String(v.clone());
            acc
        });

    let rm_json: Value = pool
        .role_mappings
        .iter()
        .fold(json!({}), |mut acc, (k, v)| {
            acc[k] = v.clone();
            acc
        });

    Ok(json!({
        "IdentityPoolId": pool_id,
        "Roles":          roles_json,
        "RoleMappings":   rm_json,
    }))
}

/// LookupDeveloperIdentity
fn lookup_developer_identity(
    state: &IdentityPoolState,
    input: &Value,
) -> Result<Value, AwsError> {
    let pool_id = input["IdentityPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityPoolId is required"))?;

    get_pool(state, pool_id)?;

    // Look up by IdentityId
    if let Some(identity_id) = input["IdentityId"].as_str() {
        if let Some(identity) = state.identities.get(identity_id) {
            return Ok(json!({
                "IdentityId":                identity.identity_id,
                "DeveloperUserIdentifierList": identity.developer_user_identifiers,
            }));
        }
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity not found: {identity_id}"),
        ));
    }

    // Look up by DeveloperUserIdentifier
    if let Some(dev_id) = input["DeveloperUserIdentifier"].as_str() {
        for entry in state.identities.iter() {
            if entry
                .developer_user_identifiers
                .iter()
                .any(|d| d == dev_id)
            {
                return Ok(json!({
                    "IdentityId":                entry.identity_id,
                    "DeveloperUserIdentifierList": entry.developer_user_identifiers,
                }));
            }
        }
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Developer identity not found: {dev_id}"),
        ));
    }

    Err(AwsError::bad_request(
        "InvalidParameter",
        "Either IdentityId or DeveloperUserIdentifier is required",
    ))
}

// ---------------------------------------------------------------------------
// Serialization helper
// ---------------------------------------------------------------------------

fn pool_to_json(pool: &IdentityPool) -> Value {
    let providers_json: Vec<Value> = pool
        .cognito_identity_providers
        .iter()
        .map(|p| {
            json!({
                "ClientId":             p.client_id,
                "ProviderName":         p.provider_name,
                "ServerSideTokenCheck": p.server_side_token_check,
            })
        })
        .collect();

    let slp_json: Value = pool
        .supported_login_providers
        .iter()
        .fold(json!({}), |mut acc, (k, v)| {
            acc[k] = Value::String(v.clone());
            acc
        });

    let roles_json: Value = pool
        .roles
        .iter()
        .fold(json!({}), |mut acc, (k, v)| {
            acc[k] = Value::String(v.clone());
            acc
        });

    let mut resp = json!({
        "IdentityPoolId":                    pool.id,
        "IdentityPoolName":                  pool.name,
        "AllowUnauthenticatedIdentities":    pool.allow_unauthenticated,
        "CognitoIdentityProviders":          providers_json,
        "SupportedLoginProviders":           slp_json,
        "Roles":                             roles_json,
        "CreationDate":                      pool.created_date,
    });

    if let Some(dpn) = &pool.developer_provider_name {
        resp["DeveloperProviderName"] = Value::String(dpn.clone());
    }

    resp
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use awsim_core::RequestContext;

    fn make_ctx() -> RequestContext {
        RequestContext::new("cognito-identity", "us-east-1")
    }

    fn make_state() -> IdentityPoolState {
        IdentityPoolState::default()
    }

    #[test]
    fn test_create_and_describe_pool() {
        let state = make_state();
        let ctx = make_ctx();
        let input = json!({
            "IdentityPoolName": "my-pool",
            "AllowUnauthenticatedIdentities": true,
        });
        let result = create_identity_pool(&state, &input, &ctx).unwrap();
        assert!(result["IdentityPoolId"].as_str().unwrap().starts_with("us-east-1:"));
        assert_eq!(result["IdentityPoolName"], "my-pool");
        assert_eq!(result["AllowUnauthenticatedIdentities"], true);

        let pool_id = result["IdentityPoolId"].as_str().unwrap().to_string();
        let desc = describe_identity_pool(&state, &json!({ "IdentityPoolId": pool_id })).unwrap();
        assert_eq!(desc["IdentityPoolId"], pool_id);
    }

    #[test]
    fn test_delete_pool() {
        let state = make_state();
        let ctx = make_ctx();
        let input = json!({
            "IdentityPoolName": "del-pool",
            "AllowUnauthenticatedIdentities": false,
        });
        let result = create_identity_pool(&state, &input, &ctx).unwrap();
        let pool_id = result["IdentityPoolId"].as_str().unwrap().to_string();

        delete_identity_pool(&state, &json!({ "IdentityPoolId": pool_id })).unwrap();

        let err = describe_identity_pool(&state, &json!({ "IdentityPoolId": pool_id })).unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn test_get_id_unauthenticated() {
        let state = make_state();
        let ctx = make_ctx();
        let create_result = create_identity_pool(
            &state,
            &json!({
                "IdentityPoolName": "my-pool",
                "AllowUnauthenticatedIdentities": true,
            }),
            &ctx,
        )
        .unwrap();
        let pool_id = create_result["IdentityPoolId"].as_str().unwrap();

        let result = get_id(&state, &json!({ "IdentityPoolId": pool_id }), &ctx).unwrap();
        let identity_id = result["IdentityId"].as_str().unwrap();
        assert!(identity_id.starts_with("us-east-1:"));
    }

    #[test]
    fn test_get_id_unauthenticated_denied() {
        let state = make_state();
        let ctx = make_ctx();
        let create_result = create_identity_pool(
            &state,
            &json!({
                "IdentityPoolName": "my-pool",
                "AllowUnauthenticatedIdentities": false,
            }),
            &ctx,
        )
        .unwrap();
        let pool_id = create_result["IdentityPoolId"].as_str().unwrap();

        let err = get_id(&state, &json!({ "IdentityPoolId": pool_id }), &ctx).unwrap_err();
        assert_eq!(err.code, "NotAuthorizedException");
    }

    #[test]
    fn test_get_credentials_for_identity() {
        let state = make_state();
        let ctx = make_ctx();
        let create_result = create_identity_pool(
            &state,
            &json!({
                "IdentityPoolName": "creds-pool",
                "AllowUnauthenticatedIdentities": true,
            }),
            &ctx,
        )
        .unwrap();
        let pool_id = create_result["IdentityPoolId"].as_str().unwrap();

        let id_result = get_id(&state, &json!({ "IdentityPoolId": pool_id }), &ctx).unwrap();
        let identity_id = id_result["IdentityId"].as_str().unwrap();

        let creds_result = get_credentials_for_identity(
            &state,
            &json!({ "IdentityId": identity_id }),
        )
        .unwrap();

        let creds = &creds_result["Credentials"];
        assert!(creds["AccessKeyId"].as_str().unwrap().starts_with("ASIA"));
        assert_eq!(creds["SecretKey"].as_str().unwrap().len(), 40);
        assert!(!creds["SessionToken"].as_str().unwrap().is_empty());
        assert!(!creds["Expiration"].as_str().unwrap().is_empty());
    }

    #[test]
    fn test_set_and_get_identity_pool_roles() {
        let state = make_state();
        let ctx = make_ctx();
        let create_result = create_identity_pool(
            &state,
            &json!({
                "IdentityPoolName": "roles-pool",
                "AllowUnauthenticatedIdentities": true,
            }),
            &ctx,
        )
        .unwrap();
        let pool_id = create_result["IdentityPoolId"].as_str().unwrap();

        set_identity_pool_roles(
            &state,
            &json!({
                "IdentityPoolId": pool_id,
                "Roles": {
                    "authenticated":   "arn:aws:iam::000000000000:role/AuthRole",
                    "unauthenticated": "arn:aws:iam::000000000000:role/UnauthRole",
                }
            }),
        )
        .unwrap();

        let roles_result =
            get_identity_pool_roles(&state, &json!({ "IdentityPoolId": pool_id })).unwrap();

        assert_eq!(
            roles_result["Roles"]["authenticated"],
            "arn:aws:iam::000000000000:role/AuthRole"
        );
        assert_eq!(
            roles_result["Roles"]["unauthenticated"],
            "arn:aws:iam::000000000000:role/UnauthRole"
        );
    }

    #[test]
    fn test_list_identity_pools() {
        let state = make_state();
        let ctx = make_ctx();

        for i in 0..3 {
            create_identity_pool(
                &state,
                &json!({
                    "IdentityPoolName": format!("pool-{i}"),
                    "AllowUnauthenticatedIdentities": false,
                }),
                &ctx,
            )
            .unwrap();
        }

        let result = list_identity_pools(&state, &json!({ "MaxResults": 60 })).unwrap();
        assert_eq!(result["IdentityPools"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_lookup_developer_identity() {
        let state = make_state();
        let ctx = make_ctx();
        let create_result = create_identity_pool(
            &state,
            &json!({
                "IdentityPoolName": "dev-pool",
                "AllowUnauthenticatedIdentities": false,
                "DeveloperProviderName": "login.my.app",
            }),
            &ctx,
        )
        .unwrap();
        let pool_id = create_result["IdentityPoolId"].as_str().unwrap();

        let token_result = get_open_id_token_for_developer_identity(
            &state,
            &json!({
                "IdentityPoolId": pool_id,
                "Logins": {
                    "login.my.app": "user-123"
                }
            }),
            &ctx,
        )
        .unwrap();
        let identity_id = token_result["IdentityId"].as_str().unwrap();

        let lookup_result = lookup_developer_identity(
            &state,
            &json!({
                "IdentityPoolId": pool_id,
                "DeveloperUserIdentifier": "user-123",
            }),
        )
        .unwrap();

        assert_eq!(lookup_result["IdentityId"], identity_id);
        assert!(lookup_result["DeveloperUserIdentifierList"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "user-123"));
    }
}
