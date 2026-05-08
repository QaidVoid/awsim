//! Cognito Identity Pools (Federated Identities)
//!
//! Signing name:  `cognito-identity`
//! Target prefix: `AWSCognitoIdentityService`

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use awsim_sts::{AssumedRoleSession, StsSessionStore};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use dashmap::DashMap;
use serde_json::{Value, json};
use tracing::debug;

// ---------------------------------------------------------------------------
// State types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CognitoProvider {
    pub client_id: String,
    /// e.g. `cognito-idp.us-east-1.amazonaws.com/us-east-1_XXXXX`
    pub provider_name: String,
    pub server_side_token_check: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrincipalTagMapping {
    pub use_defaults: bool,
    pub principal_tags: HashMap<String, String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Identity {
    pub identity_id: String,
    /// The identity pool this identity belongs to.
    pub pool_id: String,
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
    /// Shared with [`awsim_sts::StsService`]: every credential
    /// vended by `GetCredentialsForIdentity` is recorded here so
    /// downstream services see the assumed-role principal under IAM
    /// enforcement and so `sts:GetCallerIdentity` reports the right
    /// ARN. Default-constructed empty; replace via
    /// [`set_session_store`] for cross-service sharing.
    sessions: Arc<StsSessionStore>,
}

impl CognitoIdentityService {
    pub fn new() -> Self {
        Self::with_session_store(Arc::new(StsSessionStore::new()))
    }

    /// Construct the service backed by an externally-owned session
    /// store. Pair with [`awsim_sts::StsService::with_session_store`]
    /// passing the same `Arc` so credentials vended here are
    /// recognised by the principal-lookup chain.
    pub fn with_session_store(sessions: Arc<StsSessionStore>) -> Self {
        Self {
            state: AccountRegionStore::new(),
            sessions,
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
            "GetCredentialsForIdentity" => {
                get_credentials_for_identity(&state, &input, &self.sessions, &ctx.account_id)
            }
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
            "DeletePrincipalTagAttributeMap" => delete_principal_tag_attribute_map(&state, &input),
            // Tagging
            "TagResource" => tag_resource(&state, &input),
            "UntagResource" => untag_resource(&state, &input),
            "ListTagsForResource" => list_tags_for_resource(&state, &input),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let entries = self.state.iter_all();
        let snap: Vec<(String, String, IdentityPoolSnapshot)> = entries
            .into_iter()
            .map(|((account, region), state)| {
                let pools: std::collections::HashMap<String, IdentityPool> = state
                    .pools
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect();
                let identities: std::collections::HashMap<String, Identity> = state
                    .identities
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect();
                (account, region, IdentityPoolSnapshot { pools, identities })
            })
            .collect();
        serde_json::to_vec(&snap).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: Vec<(String, String, IdentityPoolSnapshot)> =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;
        for (account, region, s) in snap {
            let state = self.state.get(&account, &region);
            state.pools.clear();
            state.identities.clear();
            for (id, pool) in s.pools {
                state.pools.insert(id, pool);
            }
            for (id, identity) in s.identities {
                state.identities.insert(id, identity);
            }
        }
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct IdentityPoolSnapshot {
    pools: std::collections::HashMap<String, IdentityPool>,
    identities: std::collections::HashMap<String, Identity>,
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

fn expiration_epoch(duration_secs: u64) -> f64 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        + duration_secs;
    secs as f64
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

    let developer_provider_name = input["DeveloperProviderName"].as_str().map(String::from);

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

    // For authenticated identities, deduplicate by matching provider + token.
    if is_authenticated {
        let login_map: HashMap<String, String> = logins
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default();

        // Look for an existing identity in this pool with the same login providers and tokens.
        for entry in state.identities.iter() {
            let ident = entry.value();
            if ident.pool_id != pool_id {
                continue;
            }
            let mut match_count = 0;
            for (provider, token) in &login_map {
                if let Some(existing_token) = ident.login_tokens.get(provider)
                    && existing_token == token
                {
                    match_count += 1;
                }
            }
            if match_count == login_map.len() && !login_map.is_empty() {
                return Ok(json!({ "IdentityId": ident.identity_id }));
            }
        }
    }

    let identity_id = format!("{}:{}", ctx.region, uuid::Uuid::new_v4());

    let now = now_iso8601();
    let identity = Identity {
        identity_id: identity_id.clone(),
        pool_id: pool_id.to_string(),
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
    sessions: &StsSessionStore,
    account_id: &str,
) -> Result<Value, AwsError> {
    let identity_id = input["IdentityId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityId is required"))?;

    // Validate the identity exists
    let identity = state.identities.get(identity_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity not found: {identity_id}"),
        )
    })?;

    // Look up the pool this identity belongs to
    let pool = state.pools.get(&identity.pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity pool not found: {}", identity.pool_id),
        )
    })?;

    // Determine the IAM role to assume
    let role_arn = determine_role(&pool, &identity, input).ok_or_else(|| {
        AwsError::bad_request(
            "NotAuthorizedException",
            "No role configured for this identity's authentication state",
        )
    })?;

    // Drop the dashmap guards before generating credentials (avoids holding locks)
    let role_arn = role_arn.clone();
    drop(pool);
    drop(identity);

    let credentials = generate_credentials_for_role(&role_arn, identity_id);

    // Record the issued credential so the principal-lookup chain
    // resolves the caller back to the assumed role on follow-up
    // requests, and so `GetCallerIdentity` reports the assumed-role
    // ARN. AWS uses an hour as the default; we don't currently
    // surface the duration in the response so we just match it here.
    let access_key = credentials["AccessKeyId"]
        .as_str()
        .expect("generate_credentials_for_role always emits AccessKeyId")
        .to_string();
    let role_name = awsim_sts::sessions::role_name_from_arn(&role_arn);
    let assumed_role_id = format!(
        "AROA{}:{identity_id}",
        uuid::Uuid::new_v4().simple().to_string()[..20].to_uppercase()
    );
    sessions.record(AssumedRoleSession {
        access_key,
        role_arn: role_arn.clone(),
        role_name,
        // The session name AWS reports here is the IdentityId.
        session_name: identity_id.to_string(),
        account_id: account_id.to_string(),
        assumed_role_id,
        expiry: AssumedRoleSession::expiry_from_duration(3_600),
    });

    Ok(json!({
        "IdentityId":  identity_id,
        "Credentials": credentials,
    }))
}

// ---------------------------------------------------------------------------
// Role determination
// ---------------------------------------------------------------------------

/// Evaluate a single rules-based mapping rule against a claim value.
fn evaluate_rule(claim_value: &str, match_type: &str, expected: &str) -> bool {
    match match_type {
        "Equals" => claim_value == expected,
        "Contains" => claim_value.contains(expected),
        "StartsWith" => claim_value.starts_with(expected),
        "NotEqual" => claim_value != expected,
        _ => false,
    }
}

/// Determine the IAM role ARN to use for credential vending.
///
/// Priority:
/// 1. Provider-specific role mapping rules (Token or Rules type)
/// 2. Default authenticated / unauthenticated role from pool config
fn determine_role(pool: &IdentityPool, identity: &Identity, input: &Value) -> Option<String> {
    // Merge logins from the stored identity and the request input.
    let input_logins = input.get("Logins").and_then(|l| l.as_object());
    let has_logins = !identity.logins.is_empty() || input_logins.is_some_and(|m| !m.is_empty());

    if has_logins {
        // Check provider-specific role mappings first.
        if let Some(logins_map) = input_logins {
            for (provider, token) in logins_map {
                let mapping = lookup_role_mapping(&pool.role_mappings, provider.as_str());
                let claims = token
                    .as_str()
                    .and_then(decode_jwt_payload)
                    .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

                // No explicit mapping configured for this provider:
                // for Cognito User Pool federation, honour the
                // `cognito:preferred_role` claim if the token carries
                // one. Real AWS requires `Type: "Token"` to be set
                // explicitly; we accept it implicitly because the
                // common case in local dev is "I assigned a role to
                // the group, why isn't it being used?" — and that
                // intent is unambiguous when the only IdP is the
                // user pool that minted the JWT in the first place.
                if mapping.is_none() && provider.starts_with("cognito-idp.") {
                    if let Some(preferred) = claims
                        .get("cognito:preferred_role")
                        .and_then(|v| v.as_str())
                    {
                        debug!(
                            provider = %provider,
                            role = %preferred,
                            "GetCredentialsForIdentity: using cognito:preferred_role from ID token (no RoleMappings configured)"
                        );
                        return Some(preferred.to_string());
                    }
                    if let Some(role) = claims
                        .get("cognito:roles")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.iter().find_map(|r| r.as_str()))
                    {
                        debug!(
                            provider = %provider,
                            role = %role,
                            "GetCredentialsForIdentity: using first cognito:roles entry from ID token"
                        );
                        return Some(role.to_string());
                    }
                }

                if let Some(mapping) = mapping
                    && let Some(mapping_obj) = mapping.as_object()
                {
                    let mapping_type = mapping_obj
                        .get("Type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("");

                    match mapping_type {
                        "Token" => {
                            // Token-based: pick the role from
                            // `cognito:preferred_role`, falling back to
                            // the first entry in `cognito:roles` when
                            // no preferred role is signalled. AWS uses
                            // these exact claim names in the ID token
                            // so the local JWT we mint slots straight
                            // in.
                            if let Some(preferred) = claims
                                .get("cognito:preferred_role")
                                .and_then(|v| v.as_str())
                            {
                                return Some(preferred.to_string());
                            }
                            if let Some(roles) =
                                claims.get("cognito:roles").and_then(|v| v.as_array())
                                && let Some(first) = roles.iter().find_map(|r| r.as_str())
                            {
                                return Some(first.to_string());
                            }
                            // No role claim. AmbiguousRoleResolution
                            // governs the fallback: `Deny` means no
                            // creds (return None so the caller raises
                            // NotAuthorizedException), `AuthenticatedRole`
                            // means use the pool default — same as the
                            // implicit fallback below, so just break.
                            let resolution = mapping_obj
                                .get("AmbiguousRoleResolution")
                                .and_then(|r| r.as_str())
                                .unwrap_or("AuthenticatedRole");
                            if resolution == "Deny" {
                                return None;
                            }
                        }
                        "Rules" => {
                            // Rules-based: evaluate each rule against
                            // the decoded JWT claims. The fallback for
                            // a missing claim follows the same
                            // AmbiguousRoleResolution semantics as
                            // Token mode.
                            if let Some(rules_config) = mapping_obj.get("RulesConfiguration")
                                && let Some(rules) =
                                    rules_config.get("Rules").and_then(|r| r.as_array())
                            {
                                for rule in rules {
                                    let claim =
                                        rule.get("Claim").and_then(|c| c.as_str()).unwrap_or("");
                                    let match_type = rule
                                        .get("MatchType")
                                        .and_then(|m| m.as_str())
                                        .unwrap_or("");
                                    let expected =
                                        rule.get("Value").and_then(|v| v.as_str()).unwrap_or("");

                                    let claim_value = claim_string(&claims, claim).or_else(|| {
                                        // Real ID tokens always carry `iss`,
                                        // but our local mint may have skipped
                                        // it on synthetic tokens; fall back to
                                        // the provider name so existing tests
                                        // and bare-bones flows keep working.
                                        (claim == "iss").then(|| provider.to_string())
                                    });

                                    let Some(claim_value) = claim_value else {
                                        continue;
                                    };

                                    if evaluate_rule(&claim_value, match_type, expected)
                                        && let Some(role) =
                                            rule.get("RoleARN").and_then(|r| r.as_str())
                                    {
                                        return Some(role.to_string());
                                    }
                                }
                            }
                            let resolution = mapping_obj
                                .get("AmbiguousRoleResolution")
                                .and_then(|r| r.as_str())
                                .unwrap_or("AuthenticatedRole");
                            if resolution == "Deny" {
                                return None;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Fall back to default authenticated role.
        pool.roles.get("authenticated").cloned()
    } else {
        // Unauthenticated path.
        if !pool.allow_unauthenticated {
            // Should have been caught during GetId, but guard here too.
            return None;
        }
        pool.roles.get("unauthenticated").cloned()
    }
}

/// Find a role mapping for a Logins-map provider key. AWS documents
/// the `RoleMappings` key as `<provider>:<client_id>` for Cognito
/// User Pool federation, but the `Logins` map only carries the bare
/// `<provider>`. Match exact-key first, then accept any
/// `<provider>:CLIENT_ID` suffixed key — same intent, more forgiving.
fn lookup_role_mapping<'a>(
    mappings: &'a HashMap<String, Value>,
    provider: &str,
) -> Option<&'a Value> {
    if let Some(m) = mappings.get(provider) {
        return Some(m);
    }
    let prefix = format!("{provider}:");
    mappings
        .iter()
        .find_map(|(k, v)| k.starts_with(&prefix).then_some(v))
}

/// Decode the payload section of a JWT without verifying its
/// signature. Identity Pool role-mapping consumes claims our own
/// authorization step minted, so re-verifying signatures here would
/// cost crypto for no security gain on a single-process simulator.
/// Returns `None` for anything that isn't a three-part dot-separated
/// token whose middle segment is a valid base64url JSON object.
fn decode_jwt_payload(token: &str) -> Option<Value> {
    let payload_b64 = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Pull a JWT claim out as a string. Strings are returned verbatim;
/// arrays of strings collapse to the first entry (matches how AWS
/// describes Rules evaluation against multi-value claims —
/// `cognito:groups` and `cognito:roles` arrive as arrays). Other
/// shapes return `None` so the rule simply doesn't fire.
fn claim_string(claims: &Value, key: &str) -> Option<String> {
    let v = claims.get(key)?;
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    if let Some(arr) = v.as_array() {
        return arr.iter().find_map(|x| x.as_str().map(str::to_string));
    }
    None
}

// ---------------------------------------------------------------------------
// Role-scoped credential generation
// ---------------------------------------------------------------------------

/// Generate temporary credentials scoped to the given IAM role ARN.
///
/// The credentials are structurally identical to what AWS returns from
/// AssumeRoleWithWebIdentity — fake but realistic for local simulation.
/// The `role_arn` is embedded in the session token prefix so callers can
/// correlate credentials back to the assumed role.
fn generate_credentials_for_role(role_arn: &str, _identity_id: &str) -> Value {
    // Derive a short role identifier used as a token infix (truncated, URL-safe).
    let role_name = role_arn
        .split('/')
        .next_back()
        .unwrap_or("role")
        .replace(|c: char| !c.is_alphanumeric(), "");
    let role_infix = &role_name[..role_name.len().min(16)];

    let access_key = {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let suffix: String = id[..16].to_uppercase();
        format!("ASIA{suffix}")
    };

    let secret_key = {
        let u1 = uuid::Uuid::new_v4().simple().to_string();
        let u2 = uuid::Uuid::new_v4().simple().to_string();
        format!("{u1}{u2}")[..40].to_string()
    };

    let session_token = {
        let parts: Vec<String> = (0..4)
            .map(|_| uuid::Uuid::new_v4().simple().to_string())
            .collect();
        // Embed role_infix so the token is identifiably scoped to the assumed role.
        format!(
            "FwoGZXIvYXdzE{role_infix}{}//////////{}Aw{}Q{}",
            parts[0], parts[1], parts[2], parts[3]
        )
    };

    json!({
        "AccessKeyId":  access_key,
        "SecretKey":    secret_key,
        "SessionToken": session_token,
        "Expiration":   expiration_epoch(3600),
    })
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

    let pool_id = state
        .identities
        .get(identity_id)
        .map(|e| e.value().pool_id.clone())
        .unwrap_or_else(|| format!("{}:pool", ctx.region));
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
        let pool_id_owned = pool_id.to_string();
        let mut identity = state
            .identities
            .entry(identity_id.clone())
            .or_insert_with(|| {
                let now = now_iso8601();
                Identity {
                    identity_id: identity_id.clone(),
                    pool_id: pool_id_owned,
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

    let roles_json: Value = pool.roles.iter().fold(json!({}), |mut acc, (k, v)| {
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
fn lookup_developer_identity(state: &IdentityPoolState, input: &Value) -> Result<Value, AwsError> {
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
            if entry.developer_user_identifiers.iter().any(|d| d == dev_id) {
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

    let identities: Vec<Value> = state
        .identities
        .iter()
        .filter(|e| e.value().pool_id == pool_id)
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
    let ids = input["IdentityIdsToDelete"].as_array().ok_or_else(|| {
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
    let source_id = input["SourceUserIdentifier"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "SourceUserIdentifier is required")
    })?;
    let dest_id = input["DestinationUserIdentifier"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "DestinationUserIdentifier is required")
    })?;
    let dev_provider = input["DeveloperProviderName"].as_str().ok_or_else(|| {
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
        let pool_id_owned = pool_id.to_string();
        let mut dest = state
            .identities
            .entry(dest_identity_id.clone())
            .or_insert_with(|| Identity {
                identity_id: dest_identity_id.clone(),
                pool_id: pool_id_owned,
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
    let dev_user_identifier = input["DeveloperUserIdentifier"].as_str().ok_or_else(|| {
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

    let providers_to_remove: Vec<&str> =
        logins_to_remove.iter().filter_map(|v| v.as_str()).collect();

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
    let provider_name = input["IdentityProviderName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "IdentityProviderName is required")
    })?;

    let pool = get_pool(state, pool_id)?;

    let (use_defaults, principal_tags) = pool
        .principal_tag_maps
        .get(provider_name)
        .map(|m| (m.use_defaults, m.principal_tags.clone()))
        .unwrap_or((true, HashMap::new()));

    let tags_json: Value = principal_tags.iter().fold(json!({}), |mut acc, (k, v)| {
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
    let provider_name = input["IdentityProviderName"].as_str().ok_or_else(|| {
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

    let tags_json: Value = principal_tags.iter().fold(json!({}), |mut acc, (k, v)| {
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

/// DeletePrincipalTagAttributeMap
fn delete_principal_tag_attribute_map(
    state: &IdentityPoolState,
    input: &Value,
) -> Result<Value, AwsError> {
    let pool_id = input["IdentityPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IdentityPoolId is required"))?;
    let provider_name = input["IdentityProviderName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "IdentityProviderName is required")
    })?;

    let mut pool = state.pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Identity pool not found: {pool_id}"),
        )
    })?;

    pool.principal_tag_maps.remove(provider_name);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// Helper: pool id from ARN
// ---------------------------------------------------------------------------

fn pool_id_from_arn(arn: &str) -> Option<&str> {
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
    let pool_id = pool_id_from_arn(resource_arn)
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Invalid ResourceArn format"))?;

    let mut pool = state.pools.get_mut(pool_id).ok_or_else(|| {
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

    let pool_id = pool_id_from_arn(resource_arn)
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Invalid ResourceArn format"))?;

    let mut pool = state.pools.get_mut(pool_id).ok_or_else(|| {
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

    let pool_id = pool_id_from_arn(resource_arn)
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Invalid ResourceArn format"))?;

    let pool = get_pool(state, pool_id)?;

    let tags_json: Value = pool.tags.iter().fold(json!({}), |mut acc, (k, v)| {
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

    let slp_json: Value =
        pool.supported_login_providers
            .iter()
            .fold(json!({}), |mut acc, (k, v)| {
                acc[k] = Value::String(v.clone());
                acc
            });

    let roles_json: Value = pool.roles.iter().fold(json!({}), |mut acc, (k, v)| {
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
        assert!(
            result["IdentityPoolId"]
                .as_str()
                .unwrap()
                .starts_with("us-east-1:")
        );
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

        let err =
            describe_identity_pool(&state, &json!({ "IdentityPoolId": pool_id })).unwrap_err();
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

        // Configure an unauthenticated role so credential vending can select it.
        set_identity_pool_roles(
            &state,
            &json!({
                "IdentityPoolId": pool_id,
                "Roles": {
                    "unauthenticated": "arn:aws:iam::000000000000:role/UnauthRole",
                }
            }),
        )
        .unwrap();

        let id_result = get_id(&state, &json!({ "IdentityPoolId": pool_id }), &ctx).unwrap();
        let identity_id = id_result["IdentityId"].as_str().unwrap();

        let sessions = StsSessionStore::new();
        let creds_result = get_credentials_for_identity(
            &state,
            &json!({ "IdentityId": identity_id }),
            &sessions,
            "000000000000",
        )
        .unwrap();

        let creds = &creds_result["Credentials"];
        assert!(creds["AccessKeyId"].as_str().unwrap().starts_with("ASIA"));
        assert_eq!(creds["SecretKey"].as_str().unwrap().len(), 40);
        assert!(!creds["SessionToken"].as_str().unwrap().is_empty());
        assert!(creds["Expiration"].as_f64().unwrap() > 0.0);
        // Vending creds must record a session under the issued ASIA key.
        let asia = creds["AccessKeyId"].as_str().unwrap();
        assert!(sessions.lookup(asia).is_some());
    }

    #[test]
    fn test_get_credentials_no_role_configured() {
        let state = make_state();
        let ctx = make_ctx();
        let create_result = create_identity_pool(
            &state,
            &json!({
                "IdentityPoolName": "no-role-pool",
                "AllowUnauthenticatedIdentities": true,
            }),
            &ctx,
        )
        .unwrap();
        let pool_id = create_result["IdentityPoolId"].as_str().unwrap();

        // No roles set — should fail with NotAuthorizedException
        let id_result = get_id(&state, &json!({ "IdentityPoolId": pool_id }), &ctx).unwrap();
        let identity_id = id_result["IdentityId"].as_str().unwrap();

        let sessions = StsSessionStore::new();
        let err = get_credentials_for_identity(
            &state,
            &json!({ "IdentityId": identity_id }),
            &sessions,
            "000000000000",
        )
        .unwrap_err();
        assert_eq!(err.code, "NotAuthorizedException");
    }

    #[test]
    fn test_get_credentials_authenticated_role() {
        let state = make_state();
        let ctx = make_ctx();
        let create_result = create_identity_pool(
            &state,
            &json!({
                "IdentityPoolName": "auth-creds-pool",
                "AllowUnauthenticatedIdentities": false,
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
                    "authenticated": "arn:aws:iam::000000000000:role/AuthRole",
                }
            }),
        )
        .unwrap();

        // Create an authenticated identity (with logins)
        let id_result = get_id(
            &state,
            &json!({
                "IdentityPoolId": pool_id,
                "Logins": { "accounts.google.com": "google-token-xyz" }
            }),
            &ctx,
        )
        .unwrap();
        let identity_id = id_result["IdentityId"].as_str().unwrap();

        let sessions = StsSessionStore::new();
        let creds_result = get_credentials_for_identity(
            &state,
            &json!({ "IdentityId": identity_id }),
            &sessions,
            "000000000000",
        )
        .unwrap();

        let creds = &creds_result["Credentials"];
        assert!(creds["AccessKeyId"].as_str().unwrap().starts_with("ASIA"));
        assert_eq!(creds["SecretKey"].as_str().unwrap().len(), 40);
        assert!(!creds["SessionToken"].as_str().unwrap().is_empty());
        assert!(creds["Expiration"].as_f64().unwrap() > 0.0);
        assert_eq!(creds_result["IdentityId"], identity_id);
    }

    #[test]
    fn test_get_credentials_rules_based_role_mapping() {
        let state = make_state();
        let ctx = make_ctx();
        let create_result = create_identity_pool(
            &state,
            &json!({
                "IdentityPoolName": "rules-pool",
                "AllowUnauthenticatedIdentities": false,
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
                    "authenticated": "arn:aws:iam::000000000000:role/DefaultAuthRole",
                },
                "RoleMappings": {
                    "accounts.google.com": {
                        "Type": "Rules",
                        "AmbiguousRoleResolution": "Deny",
                        "RulesConfiguration": {
                            "Rules": [
                                {
                                    "Claim": "iss",
                                    "MatchType": "StartsWith",
                                    "Value": "accounts.google",
                                    "RoleARN": "arn:aws:iam::000000000000:role/GoogleRole"
                                }
                            ]
                        }
                    }
                }
            }),
        )
        .unwrap();

        let id_result = get_id(
            &state,
            &json!({
                "IdentityPoolId": pool_id,
                "Logins": { "accounts.google.com": "google-token-xyz" }
            }),
            &ctx,
        )
        .unwrap();
        let identity_id = id_result["IdentityId"].as_str().unwrap();

        // Pass logins so the rules mapping can evaluate the provider
        let sessions = StsSessionStore::new();
        let creds_result = get_credentials_for_identity(
            &state,
            &json!({
                "IdentityId": identity_id,
                "Logins": { "accounts.google.com": "google-token-xyz" }
            }),
            &sessions,
            "000000000000",
        )
        .unwrap();

        let creds = &creds_result["Credentials"];
        assert!(creds["AccessKeyId"].as_str().unwrap().starts_with("ASIA"));
        assert_eq!(creds["SecretKey"].as_str().unwrap().len(), 40);
    }

    /// Build a fake but well-shaped JWT carrying the given JSON
    /// payload. The signature is a placeholder — Identity Pool role
    /// mapping consumes our own tokens locally and doesn't verify
    /// signatures in this simulator.
    fn fake_jwt(payload: &Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let body = URL_SAFE_NO_PAD.encode(serde_json::to_vec(payload).unwrap());
        format!("{header}.{body}.sig")
    }

    #[test]
    fn token_mapping_uses_cognito_preferred_role_claim() {
        let state = make_state();
        let ctx = make_ctx();
        let create_result = create_identity_pool(
            &state,
            &json!({
                "IdentityPoolName": "token-pool",
                "AllowUnauthenticatedIdentities": false,
            }),
            &ctx,
        )
        .unwrap();
        let pool_id = create_result["IdentityPoolId"].as_str().unwrap();

        let provider = "cognito-idp.us-east-1.amazonaws.com/us-east-1_ABCDEF";
        let admin_role = "arn:aws:iam::000000000000:role/AdminRole";
        set_identity_pool_roles(
            &state,
            &json!({
                "IdentityPoolId": pool_id,
                "Roles": {
                    "authenticated": "arn:aws:iam::000000000000:role/DefaultAuthRole",
                },
                "RoleMappings": {
                    provider: { "Type": "Token", "AmbiguousRoleResolution": "AuthenticatedRole" }
                }
            }),
        )
        .unwrap();

        let token = fake_jwt(&json!({
            "sub": "user-1",
            "cognito:preferred_role": admin_role,
            "cognito:roles": [admin_role],
            "cognito:groups": ["Administrators"],
        }));
        let id_result = get_id(
            &state,
            &json!({ "IdentityPoolId": pool_id, "Logins": { provider: token.clone() } }),
            &ctx,
        )
        .unwrap();
        let identity_id = id_result["IdentityId"].as_str().unwrap();

        let sessions = StsSessionStore::new();
        let creds = get_credentials_for_identity(
            &state,
            &json!({ "IdentityId": identity_id, "Logins": { provider: token } }),
            &sessions,
            "000000000000",
        )
        .unwrap();

        let asia = creds["Credentials"]["AccessKeyId"].as_str().unwrap();
        let session = sessions.lookup(asia).expect("session recorded");
        assert_eq!(
            session.role_arn, admin_role,
            "Token mapping must vend the cognito:preferred_role-named role"
        );
    }

    #[test]
    fn token_mapping_falls_back_to_first_cognito_roles_entry() {
        let state = make_state();
        let ctx = make_ctx();
        let pool = create_identity_pool(
            &state,
            &json!({ "IdentityPoolName": "p", "AllowUnauthenticatedIdentities": false }),
            &ctx,
        )
        .unwrap();
        let pool_id = pool["IdentityPoolId"].as_str().unwrap();

        let provider = "cognito-idp.us-east-1.amazonaws.com/us-east-1_FALLBK";
        let role = "arn:aws:iam::000000000000:role/SoleRole";
        set_identity_pool_roles(
            &state,
            &json!({
                "IdentityPoolId": pool_id,
                "Roles": { "authenticated": "arn:aws:iam::000000000000:role/Default" },
                "RoleMappings": { provider: { "Type": "Token" } }
            }),
        )
        .unwrap();

        // No cognito:preferred_role, only cognito:roles.
        let token = fake_jwt(&json!({ "sub": "u", "cognito:roles": [role] }));
        let id_result = get_id(
            &state,
            &json!({ "IdentityPoolId": pool_id, "Logins": { provider: token.clone() } }),
            &ctx,
        )
        .unwrap();
        let identity_id = id_result["IdentityId"].as_str().unwrap();
        let sessions = StsSessionStore::new();
        let creds = get_credentials_for_identity(
            &state,
            &json!({ "IdentityId": identity_id, "Logins": { provider: token } }),
            &sessions,
            "000000000000",
        )
        .unwrap();
        let asia = creds["Credentials"]["AccessKeyId"].as_str().unwrap();
        assert_eq!(sessions.lookup(asia).unwrap().role_arn, role);
    }

    #[test]
    fn token_mapping_deny_when_no_claim_and_resolution_is_deny() {
        let state = make_state();
        let ctx = make_ctx();
        let pool = create_identity_pool(
            &state,
            &json!({ "IdentityPoolName": "p", "AllowUnauthenticatedIdentities": false }),
            &ctx,
        )
        .unwrap();
        let pool_id = pool["IdentityPoolId"].as_str().unwrap();

        let provider = "cognito-idp.us-east-1.amazonaws.com/us-east-1_DENY";
        set_identity_pool_roles(
            &state,
            &json!({
                "IdentityPoolId": pool_id,
                "Roles": { "authenticated": "arn:aws:iam::000000000000:role/Default" },
                "RoleMappings": { provider: { "Type": "Token", "AmbiguousRoleResolution": "Deny" } }
            }),
        )
        .unwrap();

        let token = fake_jwt(&json!({ "sub": "u" })); // no role claims
        let id_result = get_id(
            &state,
            &json!({ "IdentityPoolId": pool_id, "Logins": { provider: token.clone() } }),
            &ctx,
        )
        .unwrap();
        let identity_id = id_result["IdentityId"].as_str().unwrap();
        let sessions = StsSessionStore::new();
        let err = get_credentials_for_identity(
            &state,
            &json!({ "IdentityId": identity_id, "Logins": { provider: token } }),
            &sessions,
            "000000000000",
        )
        .unwrap_err();
        assert_eq!(err.code, "NotAuthorizedException");
    }

    #[test]
    fn cognito_idp_token_uses_preferred_role_without_explicit_mapping() {
        // Real AWS would require the pool to have RoleMappings:
        // {Type: Token} configured for the provider. The local
        // simulator accepts the claim implicitly so a freshly-set
        // group RoleArn "just works" without forcing the user to
        // also call SetIdentityPoolRoles with a mapping.
        let state = make_state();
        let ctx = make_ctx();
        let pool = create_identity_pool(
            &state,
            &json!({ "IdentityPoolName": "p", "AllowUnauthenticatedIdentities": false }),
            &ctx,
        )
        .unwrap();
        let pool_id = pool["IdentityPoolId"].as_str().unwrap();
        set_identity_pool_roles(
            &state,
            &json!({
                "IdentityPoolId": pool_id,
                // No RoleMappings — only a default authenticated role.
                "Roles": { "authenticated": "arn:aws:iam::000000000000:role/Default" }
            }),
        )
        .unwrap();

        let provider = "cognito-idp.us-east-1.amazonaws.com/us-east-1_AUTOM";
        let admin_role = "arn:aws:iam::000000000000:role/Admin";
        let token = fake_jwt(&json!({
            "sub": "u",
            "cognito:preferred_role": admin_role,
            "cognito:roles": [admin_role]
        }));

        let id_result = get_id(
            &state,
            &json!({ "IdentityPoolId": pool_id, "Logins": { provider: token.clone() } }),
            &ctx,
        )
        .unwrap();
        let identity_id = id_result["IdentityId"].as_str().unwrap();
        let sessions = StsSessionStore::new();
        let creds = get_credentials_for_identity(
            &state,
            &json!({ "IdentityId": identity_id, "Logins": { provider: token } }),
            &sessions,
            "000000000000",
        )
        .unwrap();
        let asia = creds["Credentials"]["AccessKeyId"].as_str().unwrap();
        assert_eq!(
            sessions.lookup(asia).unwrap().role_arn,
            admin_role,
            "should follow preferred_role even without explicit RoleMappings"
        );
    }

    #[test]
    fn role_mapping_lookup_accepts_aws_style_client_suffix() {
        // AWS documents RoleMappings keys as `<provider>:CLIENT_ID`,
        // but the Logins map only carries `<provider>`. The lookup
        // helper must bridge that.
        let state = make_state();
        let ctx = make_ctx();
        let pool = create_identity_pool(
            &state,
            &json!({ "IdentityPoolName": "p", "AllowUnauthenticatedIdentities": false }),
            &ctx,
        )
        .unwrap();
        let pool_id = pool["IdentityPoolId"].as_str().unwrap();
        let provider = "cognito-idp.us-east-1.amazonaws.com/us-east-1_KEYFMT";
        // Stored key includes :CLIENT_ID per AWS docs.
        let mapping_key = format!("{provider}:abcd1234client");
        set_identity_pool_roles(
            &state,
            &json!({
                "IdentityPoolId": pool_id,
                "Roles": { "authenticated": "arn:aws:iam::000000000000:role/Default" },
                "RoleMappings": {
                    mapping_key: { "Type": "Token", "AmbiguousRoleResolution": "AuthenticatedRole" }
                }
            }),
        )
        .unwrap();

        let admin = "arn:aws:iam::000000000000:role/Admin";
        let token = fake_jwt(&json!({ "sub": "u", "cognito:preferred_role": admin }));
        let id_result = get_id(
            &state,
            &json!({ "IdentityPoolId": pool_id, "Logins": { provider: token.clone() } }),
            &ctx,
        )
        .unwrap();
        let identity_id = id_result["IdentityId"].as_str().unwrap();
        let sessions = StsSessionStore::new();
        let creds = get_credentials_for_identity(
            &state,
            &json!({ "IdentityId": identity_id, "Logins": { provider: token } }),
            &sessions,
            "000000000000",
        )
        .unwrap();
        let asia = creds["Credentials"]["AccessKeyId"].as_str().unwrap();
        assert_eq!(sessions.lookup(asia).unwrap().role_arn, admin);
    }

    #[test]
    fn token_mapping_falls_through_to_default_when_resolution_is_authenticated_role() {
        let state = make_state();
        let ctx = make_ctx();
        let pool = create_identity_pool(
            &state,
            &json!({ "IdentityPoolName": "p", "AllowUnauthenticatedIdentities": false }),
            &ctx,
        )
        .unwrap();
        let pool_id = pool["IdentityPoolId"].as_str().unwrap();
        let provider = "cognito-idp.us-east-1.amazonaws.com/us-east-1_FALLBK";
        let default = "arn:aws:iam::000000000000:role/Default";
        set_identity_pool_roles(
            &state,
            &json!({
                "IdentityPoolId": pool_id,
                "Roles": { "authenticated": default },
                "RoleMappings": {
                    provider: { "Type": "Token", "AmbiguousRoleResolution": "AuthenticatedRole" }
                }
            }),
        )
        .unwrap();
        let token = fake_jwt(&json!({ "sub": "u" }));
        let id_result = get_id(
            &state,
            &json!({ "IdentityPoolId": pool_id, "Logins": { provider: token.clone() } }),
            &ctx,
        )
        .unwrap();
        let identity_id = id_result["IdentityId"].as_str().unwrap();
        let sessions = StsSessionStore::new();
        let creds = get_credentials_for_identity(
            &state,
            &json!({ "IdentityId": identity_id, "Logins": { provider: token } }),
            &sessions,
            "000000000000",
        )
        .unwrap();
        let asia = creds["Credentials"]["AccessKeyId"].as_str().unwrap();
        assert_eq!(sessions.lookup(asia).unwrap().role_arn, default);
    }

    #[test]
    fn test_evaluate_rule() {
        assert!(evaluate_rule(
            "accounts.google.com",
            "Equals",
            "accounts.google.com"
        ));
        assert!(!evaluate_rule(
            "accounts.google.com",
            "Equals",
            "google.com"
        ));
        assert!(evaluate_rule("accounts.google.com", "Contains", "google"));
        assert!(!evaluate_rule(
            "accounts.google.com",
            "Contains",
            "facebook"
        ));
        assert!(evaluate_rule(
            "accounts.google.com",
            "StartsWith",
            "accounts"
        ));
        assert!(!evaluate_rule(
            "accounts.google.com",
            "StartsWith",
            "google"
        ));
        assert!(evaluate_rule(
            "accounts.google.com",
            "NotEqual",
            "facebook.com"
        ));
        assert!(!evaluate_rule(
            "accounts.google.com",
            "NotEqual",
            "accounts.google.com"
        ));
        assert!(!evaluate_rule("x", "Unknown", "x"));
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
        assert!(
            lookup_result["DeveloperUserIdentifierList"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "user-123")
        );
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
        assert_eq!(
            result["UnprocessedIdentityIds"].as_array().unwrap().len(),
            0
        );

        let err = describe_identity(&state, &json!({ "IdentityId": identity_id })).unwrap_err();
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
        assert!(
            lookup["DeveloperUserIdentifierList"]
                .as_array()
                .unwrap()
                .is_empty()
        );
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
        // Real Cognito identity pool ARNs preserve the colon inside the
        // pool id (`region:uuid`), so the resource segment carries that
        // colon literally rather than being mangled.
        let arn = format!("arn:aws:cognito-identity:us-east-1:123456789012:identitypool/{pool_id}");

        tag_resource(
            &state,
            &json!({
                "ResourceArn": arn,
                "Tags": { "env": "test", "team": "infra" },
            }),
        )
        .unwrap();

        let list_result = list_tags_for_resource(&state, &json!({ "ResourceArn": arn })).unwrap();
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

        let list_result2 = list_tags_for_resource(&state, &json!({ "ResourceArn": arn })).unwrap();
        assert!(list_result2["Tags"]["team"].is_null());
        assert_eq!(list_result2["Tags"]["env"], "test");
    }
}
