//! Cognito-side wiring for the OIDC federation flow.
//!
//! When a hosted-UI authorize request arrives with an
//! `identity_provider=Foo` query parameter, this module:
//!   1. resolves `Foo` against `pool.identity_providers`,
//!   2. fetches the IdP's discovery document (cached),
//!   3. stashes the original Cognito-side authorize parameters
//!      keyed by a fresh `state` token,
//!   4. redirects the user's browser to the IdP's authorize URL
//!      with our `idpresponse` URL as the `redirect_uri`.
//!
//! When the IdP redirects back to `/cognito/{pool_id}/oauth2/idpresponse`,
//! the callback handler:
//!   1. recovers the original Cognito authorize parameters via the
//!      `state` token,
//!   2. exchanges the IdP authorization code for an ID token,
//!   3. parses the ID token claims (signature is verified against
//!      the IdP's JWKS),
//!   4. applies `AttributeMapping` from the IdP config to translate
//!      the IdP's claims into Cognito user attributes,
//!   5. upserts the federated user in the pool (creating one if it's
//!      a first-time sign-in, linking the external provider on the
//!      user record),
//!   6. mints a Cognito authorization code and redirects the
//!      browser to the original app `redirect_uri` with `?code=&state=`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::AwsError;
use dashmap::DashMap;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use serde_json::Value;
use tracing::info;
use uuid::Uuid;

use crate::oauth::{AuthCodeEntry, CognitoOAuthState};
use crate::state::{CognitoUser, IdentityProvider, LinkedProvider, UserPool};

const FEDERATION_STATE_TTL_SECS: u64 = 600;
const HTTP_TIMEOUT_SECS: u64 = 5;

/// In-flight federation request. Lives only between the initial
/// authorize redirect to the IdP and the matching `idpresponse`
/// callback (a few seconds usually).
#[derive(Clone)]
pub struct PendingFederation {
    pub pool_id: String,
    pub client_id: String,
    /// Where the *app* expects the final code to land (carried over
    /// from the original authorize request).
    pub redirect_uri: String,
    pub scope: String,
    /// State the *app* sent on the original authorize request -
    /// echoed back unchanged when we redirect to it with the final
    /// code.
    pub app_state: String,
    pub nonce: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub provider_name: String,
    pub issued_at: u64,
}

/// Cached IdP discovery doc so the federation handlers don't fetch
/// it on every authorize hit.
#[derive(Clone, Debug)]
pub struct IdpDiscovery {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: Option<String>,
    pub jwks_uri: String,
    pub issuer: String,
}

#[derive(Default)]
pub struct FederationState {
    pub pending: DashMap<String, PendingFederation>,
    /// `oidc_issuer` URL -> resolved discovery.
    pub discovery_cache: DashMap<String, IdpDiscovery>,
}

impl FederationState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Pull `oidc_issuer` / `client_id` / `client_secret` (and friends)
/// out of an `IdentityProvider` of type `OIDC`, returning a
/// structured view. Errors out early with an AWS-style
/// `InvalidParameterException` when the user-supplied
/// `ProviderDetails` map is missing required keys.
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub authorize_scopes: String,
}

pub fn parse_oidc_config(idp: &IdentityProvider) -> Result<OidcConfig, AwsError> {
    if !idp.provider_type.eq_ignore_ascii_case("OIDC") {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!(
                "identity_provider {} is type {} - federation supports OIDC only in tier 1",
                idp.provider_name, idp.provider_type
            ),
        ));
    }
    let need = |k: &str| -> Result<String, AwsError> {
        idp.provider_details.get(k).cloned().ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterException",
                format!(
                    "identity_provider {} is missing ProviderDetails.{k}",
                    idp.provider_name
                ),
            )
        })
    };
    Ok(OidcConfig {
        issuer: need("oidc_issuer")?,
        client_id: need("client_id")?,
        client_secret: need("client_secret")?,
        authorize_scopes: idp
            .provider_details
            .get("authorize_scopes")
            .cloned()
            .unwrap_or_else(|| "openid email profile".to_string()),
    })
}

/// Resolve the IdP's discovery doc, hitting the network only once
/// per (process, issuer). We allow the user to pre-supply explicit
/// endpoint URLs in `provider_details` which short-circuits the
/// fetch.
pub async fn resolve_discovery(
    fed: &FederationState,
    idp: &IdentityProvider,
    cfg: &OidcConfig,
) -> Result<IdpDiscovery, AwsError> {
    if let Some(d) = fed.discovery_cache.get(&cfg.issuer) {
        return Ok(d.clone());
    }

    let explicit_authorize = idp.provider_details.get("authorize_url").cloned();
    let explicit_token = idp.provider_details.get("token_url").cloned();
    let explicit_jwks = idp.provider_details.get("jwks_uri").cloned();
    let explicit_userinfo = idp.provider_details.get("attributes_url").cloned();

    let discovery = if let (Some(a), Some(t), Some(j)) = (
        explicit_authorize.clone(),
        explicit_token.clone(),
        explicit_jwks.clone(),
    ) {
        IdpDiscovery {
            authorization_endpoint: a,
            token_endpoint: t,
            userinfo_endpoint: explicit_userinfo,
            jwks_uri: j,
            issuer: cfg.issuer.clone(),
        }
    } else {
        let url = format!(
            "{}/.well-known/openid-configuration",
            cfg.issuer.trim_end_matches('/')
        );
        let client = http_client()?;
        let resp = client.get(&url).send().await.map_err(|e| {
            AwsError::bad_request(
                "InvalidParameterException",
                format!("failed to GET {url}: {e}"),
            )
        })?;
        if !resp.status().is_success() {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                format!("discovery {url} returned HTTP {}", resp.status()),
            ));
        }
        let body: Value = resp.json().await.map_err(|e| {
            AwsError::bad_request(
                "InvalidParameterException",
                format!("discovery {url} not JSON: {e}"),
            )
        })?;
        IdpDiscovery {
            authorization_endpoint: explicit_authorize.unwrap_or_else(|| {
                body.get("authorization_endpoint")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            }),
            token_endpoint: explicit_token.unwrap_or_else(|| {
                body.get("token_endpoint")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            }),
            userinfo_endpoint: explicit_userinfo.or_else(|| {
                body.get("userinfo_endpoint")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            }),
            jwks_uri: explicit_jwks.unwrap_or_else(|| {
                body.get("jwks_uri")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            }),
            issuer: cfg.issuer.clone(),
        }
    };

    if discovery.authorization_endpoint.is_empty()
        || discovery.token_endpoint.is_empty()
        || discovery.jwks_uri.is_empty()
    {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!(
                "incomplete OIDC config for {}: missing authorize / token / jwks endpoint",
                idp.provider_name
            ),
        ));
    }

    fed.discovery_cache
        .insert(cfg.issuer.clone(), discovery.clone());
    Ok(discovery)
}

/// Stash the original Cognito-side authorize parameters under a fresh
/// state token. The caller embeds this token as the `state` parameter
/// when redirecting to the IdP, and we recover the original on the
/// IdP's callback.
pub fn stash(fed: &FederationState, pending: PendingFederation) -> String {
    purge_expired(fed);
    let token = format!("fed_{}", Uuid::new_v4().simple());
    fed.pending.insert(token.clone(), pending);
    token
}

fn purge_expired(fed: &FederationState) {
    let cutoff = now_epoch().saturating_sub(FEDERATION_STATE_TTL_SECS);
    fed.pending.retain(|_, v| v.issued_at >= cutoff);
}

pub fn take(fed: &FederationState, state: &str) -> Option<PendingFederation> {
    fed.pending.remove(state).map(|(_, v)| v)
}

// -------------------------------------------------------------------
// HTTP client
// -------------------------------------------------------------------

fn http_client() -> Result<reqwest::Client, AwsError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        // Loopback-only is the supported config; we don't need (and
        // don't want) cert validation to interfere with self-signed
        // CAs the team might wire up via the bundled cert path.
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| crate::error::internal_error(format!("failed to build reqwest client: {e}")))
}

// -------------------------------------------------------------------
// Token exchange + ID-token validation
// -------------------------------------------------------------------

#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
    #[allow(dead_code)]
    access_token: Option<String>,
}

pub async fn exchange_code(
    discovery: &IdpDiscovery,
    cfg: &OidcConfig,
    code: &str,
    redirect_uri: &str,
) -> Result<String, AwsError> {
    let client = http_client()?;
    let resp = client
        .post(&discovery.token_endpoint)
        .basic_auth(&cfg.client_id, Some(&cfg.client_secret))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .await
        .map_err(|e| {
            AwsError::bad_request(
                "InvalidParameterException",
                format!("token endpoint POST failed: {e}"),
            )
        })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!("token endpoint returned HTTP {status}: {body}"),
        ));
    }
    let token_resp: TokenResponse = resp.json().await.map_err(|e| {
        AwsError::bad_request(
            "InvalidParameterException",
            format!("token response not JSON: {e}"),
        )
    })?;
    Ok(token_resp.id_token)
}

/// Verify the ID token signature against the IdP's JWKS and return
/// the parsed claims. Validates `iss` (must equal the configured
/// issuer), `aud` (must include the IdP-side client_id) and `exp`.
pub async fn verify_id_token(
    discovery: &IdpDiscovery,
    cfg: &OidcConfig,
    id_token: &str,
) -> Result<HashMap<String, Value>, AwsError> {
    let client = http_client()?;
    let jwks_resp = client.get(&discovery.jwks_uri).send().await.map_err(|e| {
        AwsError::bad_request("InvalidParameterException", format!("JWKS GET failed: {e}"))
    })?;
    let jwks: Value = jwks_resp.json().await.map_err(|e| {
        AwsError::bad_request("InvalidParameterException", format!("JWKS not JSON: {e}"))
    })?;

    let header = jsonwebtoken::decode_header(id_token).map_err(|e| {
        AwsError::bad_request(
            "InvalidParameterException",
            format!("malformed ID token header: {e}"),
        )
    })?;

    let kid = header.kid.as_deref();
    let key = jwks
        .get("keys")
        .and_then(|k| k.as_array())
        .and_then(|keys| {
            keys.iter()
                .find(|k| kid.is_none_or(|kid| k.get("kid").and_then(|v| v.as_str()) == Some(kid)))
        })
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterException",
                format!("no matching JWK for kid={:?}", kid),
            )
        })?;

    let n = key
        .get("n")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "JWK missing n"))?;
    let e = key
        .get("e")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "JWK missing e"))?;
    let decoding = DecodingKey::from_rsa_components(n, e).map_err(|err| {
        AwsError::bad_request(
            "InvalidParameterException",
            format!("malformed JWK RSA components: {err}"),
        )
    })?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[cfg.issuer.as_str()]);
    validation.set_audience(&[cfg.client_id.as_str()]);
    let data = jsonwebtoken::decode::<HashMap<String, Value>>(id_token, &decoding, &validation)
        .map_err(|e| {
            AwsError::bad_request(
                "InvalidParameterException",
                format!("ID token failed verification: {e}"),
            )
        })?;

    Ok(data.claims)
}

// -------------------------------------------------------------------
// Attribute mapping + user upsert
// -------------------------------------------------------------------

/// Translate an IdP's claim payload into a Cognito-shape user
/// attribute map by applying `AttributeMapping` (Cognito attribute
/// name -> IdP claim name).
pub fn map_attributes(
    idp: &IdentityProvider,
    idp_claims: &HashMap<String, Value>,
) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    for (cognito_attr, idp_claim) in &idp.attribute_mapping {
        if let Some(v) = idp_claims.get(idp_claim) {
            if let Some(s) = v.as_str() {
                out.insert(cognito_attr.clone(), s.to_string());
            } else if let Some(b) = v.as_bool() {
                out.insert(cognito_attr.clone(), b.to_string());
            } else if v.is_number() {
                out.insert(cognito_attr.clone(), v.to_string());
            }
        }
    }
    // Cognito's default mapping if none is configured: pull email +
    // name straight through. Real Cognito does this when the IdP is
    // first registered without an explicit AttributeMapping.
    if idp.attribute_mapping.is_empty() {
        for std_claim in ["email", "name", "given_name", "family_name", "phone_number"] {
            if let Some(v) = idp_claims.get(std_claim).and_then(|v| v.as_str()) {
                out.insert(std_claim.to_string(), v.to_string());
            }
        }
    }
    out
}

/// Upsert a federated user. The username is derived from the
/// IdP's `sub` claim prefixed with the provider name to avoid
/// collisions with native users (matches Cognito's "ProviderName_sub"
/// convention). Returns the resolved `(username, sub)`.
pub fn upsert_user(
    pool: &mut UserPool,
    provider_name: &str,
    idp_sub: &str,
    attributes: HashMap<String, String>,
) -> (String, String) {
    let federated_username = format!("{provider_name}_{idp_sub}");

    if let Some(user) = pool.users.get_mut(&federated_username) {
        // Refresh attributes that came down on this sign-in. We only
        // overwrite mapped attrs; auto-managed `sub` and unrelated
        // attrs are left alone.
        for (k, v) in attributes {
            if k == "sub" {
                continue;
            }
            user.attributes.insert(k, v);
        }
        // Idempotent link record.
        let already_linked = user.linked_providers.iter().any(|lp| {
            lp.provider_name == provider_name
                && lp.provider_attribute_name == "Cognito_Subject"
                && lp.provider_attribute_value == idp_sub
        });
        if !already_linked {
            user.linked_providers.push(LinkedProvider {
                provider_name: provider_name.to_string(),
                provider_attribute_name: "Cognito_Subject".to_string(),
                provider_attribute_value: idp_sub.to_string(),
            });
        }
        return (user.username.clone(), user.sub.clone());
    }

    let sub = Uuid::new_v4().to_string();
    let mut attrs = attributes;
    attrs.insert("sub".to_string(), sub.clone());
    let user = CognitoUser {
        username: federated_username.clone(),
        sub: sub.clone(),
        // Federated users never have a local password. Storing an
        // unusable hash + no SRP material is intentional - any local
        // password-flow attempt against them fails closed.
        password_hash: String::new(),
        srp_salt: None,
        srp_verifier: None,
        attributes: attrs,
        status: "EXTERNAL_PROVIDER".to_string(),
        enabled: true,
        groups: Vec::new(),
        created_date: now_epoch(),
        last_modified_date: now_epoch(),
        pending_verifications: HashMap::new(),
        pending_verifications_issued: HashMap::new(),
        code_failed_attempts: 0,
        code_locked_until_secs: None,
        revoked_refresh_tokens: Vec::new(),
        signed_out_at: None,
        mfa_enabled: false,
        mfa_preferred: None,
        totp_secret: None,
        totp_verified: false,
        devices: Vec::new(),
        linked_providers: vec![LinkedProvider {
            provider_name: provider_name.to_string(),
            provider_attribute_name: "Cognito_Subject".to_string(),
            provider_attribute_value: idp_sub.to_string(),
        }],
        mfa_options: Vec::new(),
        webauthn_credentials: Vec::new(),
        webauthn_pending_challenge: None,
        failed_login_attempts: 0,
        locked_until_secs: None,
        auth_events: Vec::new(),
    };
    pool.users.insert(federated_username.clone(), user);
    info!(provider = %provider_name, sub = %idp_sub, "Cognito federation: created federated user");
    (federated_username, sub)
}

// -------------------------------------------------------------------
// Mint Cognito auth code at the end of the flow
// -------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn mint_cognito_code(
    oauth_state: &CognitoOAuthState,
    pool_id: &str,
    client_id: &str,
    redirect_uri: &str,
    user_sub: &str,
    username: &str,
    scopes: Vec<String>,
    nonce: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
) -> String {
    let code = Uuid::new_v4().simple().to_string();
    oauth_state.auth_codes.insert(
        code.clone(),
        AuthCodeEntry {
            pool_id: pool_id.to_string(),
            client_id: client_id.to_string(),
            redirect_uri: redirect_uri.to_string(),
            user_sub: user_sub.to_string(),
            username: username.to_string(),
            issued_at: now_epoch(),
            code_challenge,
            code_challenge_method,
            scopes,
            nonce,
        },
    );
    code
}

// -------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------

pub fn build_idp_authorize_url(
    discovery: &IdpDiscovery,
    cfg: &OidcConfig,
    cognito_redirect_uri: &str,
    state: &str,
    nonce: Option<&str>,
) -> String {
    let mut url = discovery.authorization_endpoint.clone();
    let pairs = [
        ("response_type", "code"),
        ("client_id", cfg.client_id.as_str()),
        ("redirect_uri", cognito_redirect_uri),
        ("scope", cfg.authorize_scopes.as_str()),
        ("state", state),
    ];
    let mut query = String::new();
    for (k, v) in pairs {
        if !query.is_empty() {
            query.push('&');
        }
        query.push_str(k);
        query.push('=');
        query.push_str(&urlencode(v));
    }
    if let Some(n) = nonce.filter(|s| !s.is_empty()) {
        query.push_str(&format!("&nonce={}", urlencode(n)));
    }
    url.push(if url.contains('?') { '&' } else { '?' });
    url.push_str(&query);
    url
}

pub fn urlencode(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
                vec![c]
            } else {
                let mut buf = [0u8; 4];
                let bytes = c.encode_utf8(&mut buf);
                bytes
                    .bytes()
                    .flat_map(|b| {
                        let hi = char::from_digit((b >> 4) as u32, 16)
                            .expect("0..=15 valid base-16")
                            .to_ascii_uppercase();
                        let lo = char::from_digit((b & 0xf) as u32, 16)
                            .expect("0..=15 valid base-16")
                            .to_ascii_uppercase();
                        vec!['%', hi, lo]
                    })
                    .collect()
            }
        })
        .collect()
}

/// Extract the IdP's `sub` claim. Required - Cognito federation
/// can't link a user without one.
pub fn extract_idp_sub(claims: &HashMap<String, Value>) -> Result<String, AwsError> {
    claims
        .get("sub")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterException",
                "ID token missing required `sub` claim",
            )
        })
}
