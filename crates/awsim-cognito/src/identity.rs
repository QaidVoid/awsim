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
pub struct PrincipalTagMapping {
    pub use_defaults: bool,
    pub principal_tags: HashMap<String, String>,
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
    /// Resource tags for this identity pool.
    pub tags: HashMap<String, String>,
    /// provider_name → PrincipalTagMapping
    pub principal_tag_maps: HashMap<String, PrincipalTagMapping>,
}

#[derive(Debug, Clone)]
pub struct Identity {
    pub identity_id: String,
    /// Provider names the identity has logged in with.
    pub logins: Vec<String>,
    /// Provider name → token map (used by UnlinkIdentity).
    pub login_tokens: HashMap<String, String>,
    pub creation_date: String,
    pub last_modified_date: String,
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
            // Identity management
            "DescribeIdentity" => describe_identity(&state, &input),
            "ListIdentities" => list_identities(&state, &input),
            "DeleteIdentities" => delete_identities(&state, &input),
            // Developer identity
            "MergeDeveloperIdentities" => merge_developer_identities(&state, &input, ctx),
            "UnlinkDeveloperIdentity" => unlink_developer_identity(&state, &input),
            // Federation
            "UnlinkIdentity" => unlink_identity(&state, &input),
            // Principal tags
            "GetPrincipalTagAttributeMap" => get_principal_tag_attribute_map(&state, &input),
            "SetPrincipalTagAttributeMap" => set_principal_tag_attribute_map(&state, &input),
            // Tagging
            "TagResource" => tag_resource(&state, &input),
            "UntagResource" => untag_resource(&state, &input),
            "ListTagsForResource" => list_tags_for_resource(&state, &input),
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
        tags: HashMap::new(),
        principal_tag_maps: HashMap::new(),
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

    let now = now_iso8601();
    let identity = Identity {
        identity_id: identity_id.clone(),
        logins: login_providers,
        login_tokens: logins
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        creation_date: now.clone(),
        last_modified_date: now,
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
            .or_insert_with(|| {
                let now = now_iso8601();
                Identity {
                    identity_id: identity_id.clone(),
                    logins: logins.keys().cloned().collect(),
                    login_tokens: logins
                        .iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                        .collect(),
                    creation_date: now.clone(),
                    last_modified_date: now,
                    developer_user_identifiers: vec![],
                }
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
// New operations (identity management, developer identity, federation, tags)
// ---------------------------------------------------------------------------

/// DescribeIdentity
fn describe_identity(state: &IdentityPoolState, input: &Value) -> Result<Value, AwsError> {
    let identity_id = input["IdentityId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityId is required"))?;

    let identity = state.identities.get(identity_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity not found: {identity_id}"),
        )
    })?;

    Ok(json!({
        "IdentityId":       identity.identity_id,
        "Logins":           identity.logins,
        "CreationDate":     identity.creation_date,
        "LastModifiedDate": identity.last_modified_date,
    }))
}

/// ListIdentities
fn list_identities(state: &IdentityPoolState, input: &Value) -> Result<Value, AwsError> {
    let pool_id = input["IdentityPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityPoolId is required"))?;

    get_pool(state, pool_id)?;

    let max_results = input["MaxResults"].as_u64().unwrap_or(60) as usize;

    // Filter identities that belong to this pool (identity_id starts with pool region prefix)
    // In our simulator all identities share the same region store so we return all of them
    // with a simple pagination stub.
    let identities: Vec<Value> = state
        .identities
        .iter()
        .take(max_results)
        .map(|e| {
            json!({
                "IdentityId":       e.value().identity_id,
                "Logins":           e.value().logins,
                "CreationDate":     e.value().creation_date,
                "LastModifiedDate": e.value().last_modified_date,
            })
        })
        .collect();

    Ok(json!({
        "IdentityPoolId": pool_id,
        "Identities":     identities,
    }))
}

/// DeleteIdentities
fn delete_identities(state: &IdentityPoolState, input: &Value) -> Result<Value, AwsError> {
    let ids = input["IdentityIdsToDelete"]
        .as_array()
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameter", "IdentityIdsToDelete is required")
        })?;

    for id_val in ids {
        if let Some(id) = id_val.as_str() {
            state.identities.remove(id);
        }
    }

    Ok(json!({ "UnprocessedIdentityIds": [] }))
}

/// MergeDeveloperIdentities — merge source into destination, delete source.
fn merge_developer_identities(
    state: &IdentityPoolState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["IdentityPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityPoolId is required"))?;
    let source_id = input["SourceUserIdentifier"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameter", "SourceUserIdentifier is required")
        })?;
    let dest_id = input["DestinationUserIdentifier"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameter", "DestinationUserIdentifier is required")
        })?;
    let dev_provider = input["DeveloperProviderName"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameter", "DeveloperProviderName is required")
        })?;

    get_pool(state, pool_id)?;

    // Find source identity by developer user identifier
    let source_identity_id = {
        let mut found = None;
        for entry in state.identities.iter() {
            if entry
                .developer_user_identifiers
                .iter()
                .any(|d| d == source_id)
            {
                found = Some(entry.identity_id.clone());
                break;
            }
        }
        found
    };

    // Find or create destination identity
    let dest_identity_id = {
        let mut found = None;
        for entry in state.identities.iter() {
            if entry
                .developer_user_identifiers
                .iter()
                .any(|d| d == dest_id)
            {
                found = Some(entry.identity_id.clone());
                break;
            }
        }
        found.unwrap_or_else(|| format!("{}:{}", ctx.region, uuid::Uuid::new_v4()))
    };

    // Transfer logins from source to destination
    let source_logins: Vec<String> = if let Some(src_id) = &source_identity_id {
        state
            .identities
            .get(src_id)
            .map(|e| e.developer_user_identifiers.clone())
            .unwrap_or_default()
    } else {
        vec![]
    };

    {
        let now = now_iso8601();
        let mut dest = state
            .identities
            .entry(dest_identity_id.clone())
            .or_insert_with(|| Identity {
                identity_id: dest_identity_id.clone(),
                logins: vec![dev_provider.to_string()],
                login_tokens: HashMap::new(),
                creation_date: now.clone(),
                last_modified_date: now.clone(),
                developer_user_identifiers: vec![dest_id.to_string()],
            });

        for l in &source_logins {
            if !dest.developer_user_identifiers.contains(l) {
                dest.developer_user_identifiers.push(l.clone());
            }
        }
        dest.last_modified_date = now;
    }

    // Remove source identity
    if let Some(src_id) = &source_identity_id {
        state.identities.remove(src_id);
    }

    Ok(json!({ "IdentityId": dest_identity_id }))
}

/// UnlinkDeveloperIdentity — remove a developer user identifier from an identity.
fn unlink_developer_identity(state: &IdentityPoolState, input: &Value) -> Result<Value, AwsError> {
    let identity_id = input["IdentityId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityId is required"))?;
    let pool_id = input["IdentityPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityPoolId is required"))?;
    let dev_user_identifier = input["DeveloperUserIdentifier"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameter", "DeveloperUserIdentifier is required")
        })?;

    get_pool(state, pool_id)?;

    let mut identity = state.identities.get_mut(identity_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity not found: {identity_id}"),
        )
    })?;

    identity
        .developer_user_identifiers
        .retain(|d| d != dev_user_identifier);
    identity.last_modified_date = now_iso8601();

    Ok(json!({}))
}

/// UnlinkIdentity — remove federated logins from an identity.
fn unlink_identity(state: &IdentityPoolState, input: &Value) -> Result<Value, AwsError> {
    let identity_id = input["IdentityId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityId is required"))?;
    let logins_to_remove = input["LoginsToRemove"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "LoginsToRemove is required"))?;

    let mut identity = state.identities.get_mut(identity_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity not found: {identity_id}"),
        )
    })?;

    let providers_to_remove: Vec<&str> = logins_to_remove
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    identity
        .logins
        .retain(|l| !providers_to_remove.contains(&l.as_str()));
    for p in &providers_to_remove {
        identity.login_tokens.remove(*p);
    }
    identity.last_modified_date = now_iso8601();

    Ok(json!({}))
}

/// GetPrincipalTagAttributeMap
fn get_principal_tag_attribute_map(
    state: &IdentityPoolState,
    input: &Value,
) -> Result<Value, AwsError> {
    let pool_id = input["IdentityPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityPoolId is required"))?;
    let provider_name = input["IdentityProviderName"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameter", "IdentityProviderName is required")
        })?;

    let pool = get_pool(state, pool_id)?;

    let (use_defaults, principal_tags) = pool
        .principal_tag_maps
        .get(provider_name)
        .map(|m| (m.use_defaults, m.principal_tags.clone()))
        .unwrap_or((true, HashMap::new()));

    let tags_json: Value = principal_tags
        .iter()
        .fold(json!({}), |mut acc, (k, v)| {
            acc[k] = Value::String(v.clone());
            acc
        });

    Ok(json!({
        "IdentityPoolId":       pool_id,
        "IdentityProviderName": provider_name,
        "UseDefaults":          use_defaults,
        "PrincipalTags":        tags_json,
    }))
}

/// SetPrincipalTagAttributeMap
fn set_principal_tag_attribute_map(
    state: &IdentityPoolState,
    input: &Value,
) -> Result<Value, AwsError> {
    let pool_id = input["IdentityPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityPoolId is required"))?;
    let provider_name = input["IdentityProviderName"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameter", "IdentityProviderName is required")
        })?;
    let use_defaults = input["UseDefaults"].as_bool().unwrap_or(true);

    let principal_tags: HashMap<String, String> = input["PrincipalTags"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default();

    let mut pool = state.pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity pool not found: {pool_id}"),
        )
    })?;

    pool.principal_tag_maps.insert(
        provider_name.to_string(),
        PrincipalTagMapping {
            use_defaults,
            principal_tags: principal_tags.clone(),
        },
    );

    let tags_json: Value = principal_tags
        .iter()
        .fold(json!({}), |mut acc, (k, v)| {
            acc[k] = Value::String(v.clone());
            acc
        });

    Ok(json!({
        "IdentityPoolId":       pool_id,
        "IdentityProviderName": provider_name,
        "UseDefaults":          use_defaults,
        "PrincipalTags":        tags_json,
    }))
}

// ---------------------------------------------------------------------------
// Helper: pool id from ARN
// ---------------------------------------------------------------------------

fn pool_id_from_arn<'a>(arn: &'a str) -> Option<&'a str> {
    // arn:aws:cognito-identity:region:account:identitypool/pool_id
    arn.split('/').nth(1)
}

/// TagResource
fn tag_resource(state: &IdentityPoolState, input: &Value) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceArn is required"))?;

    let new_tags: HashMap<String, String> = input["Tags"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default();

    // Resolve the pool from ARN
    let pool_id_raw = pool_id_from_arn(resource_arn).ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "Invalid ResourceArn format")
    })?;
    // Pool ids use ':' but we stored them with '_' in the ARN; convert back
    let pool_id = pool_id_raw.replace('_', ":");

    let mut pool = state.pools.get_mut(&pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity pool not found: {pool_id}"),
        )
    })?;

    for (k, v) in new_tags {
        pool.tags.insert(k, v);
    }

    Ok(json!({}))
}

/// UntagResource
fn untag_resource(state: &IdentityPoolState, input: &Value) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceArn is required"))?;

    let tag_keys: Vec<&str> = input["TagKeys"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TagKeys is required"))?
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    let pool_id_raw = pool_id_from_arn(resource_arn).ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "Invalid ResourceArn format")
    })?;
    let pool_id = pool_id_raw.replace('_', ":");

    let mut pool = state.pools.get_mut(&pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity pool not found: {pool_id}"),
        )
    })?;

    for key in tag_keys {
        pool.tags.remove(key);
    }

    Ok(json!({}))
}

/// ListTagsForResource
fn list_tags_for_resource(state: &IdentityPoolState, input: &Value) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceArn is required"))?;

    let pool_id_raw = pool_id_from_arn(resource_arn).ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "Invalid ResourceArn format")
    })?;
    let pool_id = pool_id_raw.replace('_', ":");

    let pool = get_pool(state, &pool_id)?;

    let tags_json: Value = pool
        .tags
        .iter()
        .fold(json!({}), |mut acc, (k, v)| {
            acc[k] = Value::String(v.clone());
            acc
        });

    Ok(json!({ "Tags": tags_json }))
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

    // ------------------------------------------------------------------
    // Tests for new operations
    // ------------------------------------------------------------------

    #[test]
    fn test_describe_identity() {
        let state = make_state();
        let ctx = make_ctx();
        let create_result = create_identity_pool(
            &state,
            &json!({
                "IdentityPoolName": "desc-id-pool",
                "AllowUnauthenticatedIdentities": true,
            }),
            &ctx,
        )
        .unwrap();
        let pool_id = create_result["IdentityPoolId"].as_str().unwrap();
        let id_result = get_id(&state, &json!({ "IdentityPoolId": pool_id }), &ctx).unwrap();
        let identity_id = id_result["IdentityId"].as_str().unwrap();

        let desc = describe_identity(&state, &json!({ "IdentityId": identity_id })).unwrap();
        assert_eq!(desc["IdentityId"], identity_id);
        assert!(desc["CreationDate"].as_str().is_some());
        assert!(desc["LastModifiedDate"].as_str().is_some());
    }

    #[test]
    fn test_list_identities() {
        let state = make_state();
        let ctx = make_ctx();
        let create_result = create_identity_pool(
            &state,
            &json!({
                "IdentityPoolName": "list-id-pool",
                "AllowUnauthenticatedIdentities": true,
            }),
            &ctx,
        )
        .unwrap();
        let pool_id = create_result["IdentityPoolId"].as_str().unwrap();

        get_id(&state, &json!({ "IdentityPoolId": pool_id }), &ctx).unwrap();
        get_id(&state, &json!({ "IdentityPoolId": pool_id }), &ctx).unwrap();

        let result = list_identities(
            &state,
            &json!({ "IdentityPoolId": pool_id, "MaxResults": 60 }),
        )
        .unwrap();
        assert_eq!(result["IdentityPoolId"], pool_id);
        assert_eq!(result["Identities"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_delete_identities() {
        let state = make_state();
        let ctx = make_ctx();
        let create_result = create_identity_pool(
            &state,
            &json!({
                "IdentityPoolName": "del-id-pool",
                "AllowUnauthenticatedIdentities": true,
            }),
            &ctx,
        )
        .unwrap();
        let pool_id = create_result["IdentityPoolId"].as_str().unwrap();
        let id_result = get_id(&state, &json!({ "IdentityPoolId": pool_id }), &ctx).unwrap();
        let identity_id = id_result["IdentityId"].as_str().unwrap().to_string();

        let result = delete_identities(
            &state,
            &json!({ "IdentityIdsToDelete": [identity_id.clone()] }),
        )
        .unwrap();
        assert_eq!(result["UnprocessedIdentityIds"].as_array().unwrap().len(), 0);

        let err =
            describe_identity(&state, &json!({ "IdentityId": identity_id })).unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn test_unlink_developer_identity() {
        let state = make_state();
        let ctx = make_ctx();
        let create_result = create_identity_pool(
            &state,
            &json!({
                "IdentityPoolName": "unlink-dev-pool",
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
                "Logins": { "login.my.app": "user-abc" }
            }),
            &ctx,
        )
        .unwrap();
        let identity_id = token_result["IdentityId"].as_str().unwrap().to_string();

        unlink_developer_identity(
            &state,
            &json!({
                "IdentityId": identity_id,
                "IdentityPoolId": pool_id,
                "DeveloperProviderName": "login.my.app",
                "DeveloperUserIdentifier": "user-abc",
            }),
        )
        .unwrap();

        let lookup = lookup_developer_identity(
            &state,
            &json!({
                "IdentityPoolId": pool_id,
                "IdentityId": identity_id,
            }),
        )
        .unwrap();
        assert!(lookup["DeveloperUserIdentifierList"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_unlink_identity() {
        let state = make_state();
        let ctx = make_ctx();
        let create_result = create_identity_pool(
            &state,
            &json!({
                "IdentityPoolName": "unlink-fed-pool",
                "AllowUnauthenticatedIdentities": true,
            }),
            &ctx,
        )
        .unwrap();
        let pool_id = create_result["IdentityPoolId"].as_str().unwrap();
        let id_result = get_id(
            &state,
            &json!({
                "IdentityPoolId": pool_id,
                "Logins": { "accounts.google.com": "google-token-xyz" }
            }),
            &ctx,
        )
        .unwrap();
        let identity_id = id_result["IdentityId"].as_str().unwrap().to_string();

        unlink_identity(
            &state,
            &json!({
                "IdentityId": identity_id,
                "Logins": { "accounts.google.com": "google-token-xyz" },
                "LoginsToRemove": ["accounts.google.com"],
            }),
        )
        .unwrap();

        let desc = describe_identity(&state, &json!({ "IdentityId": identity_id })).unwrap();
        assert!(desc["Logins"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_principal_tag_operations() {
        let state = make_state();
        let ctx = make_ctx();
        let create_result = create_identity_pool(
            &state,
            &json!({
                "IdentityPoolName": "ptag-pool",
                "AllowUnauthenticatedIdentities": false,
            }),
            &ctx,
        )
        .unwrap();
        let pool_id = create_result["IdentityPoolId"].as_str().unwrap();

        // Set principal tags
        set_principal_tag_attribute_map(
            &state,
            &json!({
                "IdentityPoolId": pool_id,
                "IdentityProviderName": "accounts.google.com",
                "UseDefaults": false,
                "PrincipalTags": { "email": "email", "sub": "sub" },
            }),
        )
        .unwrap();

        // Get and verify
        let result = get_principal_tag_attribute_map(
            &state,
            &json!({
                "IdentityPoolId": pool_id,
                "IdentityProviderName": "accounts.google.com",
            }),
        )
        .unwrap();
        assert_eq!(result["UseDefaults"], false);
        assert_eq!(result["PrincipalTags"]["email"], "email");
        assert_eq!(result["PrincipalTags"]["sub"], "sub");
    }

    #[test]
    fn test_resource_tagging() {
        let state = make_state();
        let ctx = make_ctx();
        let create_result = create_identity_pool(
            &state,
            &json!({
                "IdentityPoolName": "tag-pool",
                "AllowUnauthenticatedIdentities": false,
            }),
            &ctx,
        )
        .unwrap();
        let pool_id = create_result["IdentityPoolId"].as_str().unwrap();
        // Build the ARN the same way our tag_resource helper expects it
        let arn = format!(
            "arn:aws:cognito-identity:us-east-1:123456789012:identitypool/{}",
            pool_id.replace(':', "_")
        );

        tag_resource(
            &state,
            &json!({
                "ResourceArn": arn,
                "Tags": { "env": "test", "team": "infra" },
            }),
        )
        .unwrap();

        let list_result =
            list_tags_for_resource(&state, &json!({ "ResourceArn": arn })).unwrap();
        assert_eq!(list_result["Tags"]["env"], "test");
        assert_eq!(list_result["Tags"]["team"], "infra");

        untag_resource(
            &state,
            &json!({
                "ResourceArn": arn,
                "TagKeys": ["team"],
            }),
        )
        .unwrap();

        let list_result2 =
            list_tags_for_resource(&state, &json!({ "ResourceArn": arn })).unwrap();
        assert!(list_result2["Tags"]["team"].is_null());
        assert_eq!(list_result2["Tags"]["env"], "test");
    }
}
