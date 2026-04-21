/// OAuth2/OIDC endpoints for Cognito hosted UI auth flow.
///
/// These are standard HTTP endpoints (not AWS API calls) mounted directly
/// on the Axum router, accessible without SigV4 auth.
///
/// Endpoints:
///   GET  /cognito/{pool_id}/.well-known/openid-configuration
///   GET  /cognito/{pool_id}/.well-known/jwks.json
///   GET  /cognito/{pool_id}/oauth2/authorize
///   POST /cognito/{pool_id}/oauth2/token
///   GET  /cognito/{pool_id}/oauth2/userInfo
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Redirect, Response};
use dashmap::DashMap;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::jwt;
use crate::state::CognitoState;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// An authorization code stored in the pending-codes map.
#[derive(Clone)]
pub struct AuthCodeEntry {
    pub pool_id: String,
    pub client_id: String,
    pub redirect_uri: String,
    /// Sub of the user the code was issued for.
    pub user_sub: String,
    pub username: String,
    /// Unix timestamp of when the code was issued (5-minute TTL).
    pub issued_at: u64,
}

/// Shared state for the OAuth/OIDC router.
///
/// The `cognito` field is an `Arc<CognitoState>` for the default account+region.
/// Because `CognitoState` uses `DashMap` internally (which is `Send + Sync`),
/// sharing it across the OAuth router is safe without cloning its contents.
#[derive(Clone)]
pub struct CognitoOAuthState {
    /// Cognito state for the default account/region (shared with CognitoService).
    pub cognito: Arc<CognitoState>,
    /// Default region (stored for JWT issuer construction).
    pub default_region: String,
    /// Default account ID.
    pub default_account_id: String,
    /// Pending authorization codes: code → entry.
    pub auth_codes: Arc<DashMap<String, AuthCodeEntry>>,
    /// Port the server is listening on (for constructing URLs).
    pub port: u16,
}

impl CognitoOAuthState {
    fn base_url(&self) -> String {
        format!("http://localhost:{}", self.port)
    }

    fn issuer(&self, pool_id: &str) -> String {
        format!("{}/cognito/{}", self.base_url(), pool_id)
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router(state: Arc<CognitoOAuthState>) -> axum::Router {
    axum::Router::new()
        .route(
            "/cognito/{pool_id}/.well-known/openid-configuration",
            axum::routing::get(openid_config),
        )
        .route(
            "/cognito/{pool_id}/.well-known/jwks.json",
            axum::routing::get(jwks),
        )
        .route(
            "/cognito/{pool_id}/oauth2/authorize",
            axum::routing::get(authorize),
        )
        .route(
            "/cognito/{pool_id}/oauth2/token",
            axum::routing::post(token),
        )
        .route(
            "/cognito/{pool_id}/oauth2/userInfo",
            axum::routing::get(userinfo),
        )
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn new_code() -> String {
    uuid::Uuid::new_v4().to_string().replace('-', "")
}

/// Purge expired codes (older than 5 minutes).
fn purge_expired_codes(codes: &DashMap<String, AuthCodeEntry>) {
    let cutoff = now_epoch().saturating_sub(300);
    codes.retain(|_, v| v.issued_at >= cutoff);
}

// ---------------------------------------------------------------------------
// 1. OIDC Discovery
// ---------------------------------------------------------------------------

async fn openid_config(
    State(state): State<Arc<CognitoOAuthState>>,
    Path(pool_id): Path<String>,
) -> Json<Value> {
    let base = state.issuer(&pool_id);
    Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/oauth2/authorize"),
        "token_endpoint": format!("{base}/oauth2/token"),
        "userinfo_endpoint": format!("{base}/oauth2/userInfo"),
        "jwks_uri": format!("{}/cognito/{pool_id}/.well-known/jwks.json", state.base_url()),
        "response_types_supported": ["code", "token", "id_token"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
        "scopes_supported": ["openid", "email", "phone", "profile"],
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"],
        "grant_types_supported": ["authorization_code", "implicit", "client_credentials", "refresh_token"]
    }))
}

// ---------------------------------------------------------------------------
// 2. JWKS
// ---------------------------------------------------------------------------

async fn jwks() -> Json<Value> {
    // Fixed dummy RSA public key. The modulus (n) is a 2048-bit value encoded
    // as base64url. Clients that fetch JWKS for structural validation will
    // accept this; cryptographic verification will fail (by design for a local
    // emulator that uses dummy signatures in jwt.rs).
    Json(json!({
        "keys": [{
            "kty": "RSA",
            "alg": "RS256",
            "use": "sig",
            "kid": "awsim-key-1",
            "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAt\
                  VT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn6\
                  4tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_F\
                  DW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n\
                  91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksIN\
                  HaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw",
            "e": "AQAB"
        }]
    }))
}

// ---------------------------------------------------------------------------
// 3. Authorization endpoint
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AuthorizeParams {
    response_type: String,
    client_id: String,
    redirect_uri: Option<String>,
    scope: Option<String>,
    state: Option<String>,
    #[serde(rename = "nonce")]
    _nonce: Option<String>,
}

async fn authorize(
    State(oauth_state): State<Arc<CognitoOAuthState>>,
    Path(pool_id): Path<String>,
    Query(params): Query<AuthorizeParams>,
) -> Response {
    let redirect_uri = match &params.redirect_uri {
        Some(u) => u.clone(),
        None => {
            return (StatusCode::BAD_REQUEST, "redirect_uri is required").into_response();
        }
    };

    let cognito = &oauth_state.cognito;

    // Find the pool.
    let pool_ref = match cognito.user_pools.get(&pool_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                format!("User pool not found: {pool_id}"),
            )
                .into_response();
        }
    };

    // Validate client_id exists in this pool.
    if !pool_ref.clients.contains_key(&params.client_id) {
        warn!(
            client_id = %params.client_id,
            pool_id = %pool_id,
            "OAuth authorize: unknown client_id"
        );
        // Still continue — in local dev we're lenient.
    }

    // Pick the first confirmed user in the pool to auto-authenticate.
    // In a real Cognito setup there would be a login page; here we skip it.
    let user = pool_ref
        .users
        .values()
        .find(|u| u.status == "CONFIRMED" && u.enabled)
        .or_else(|| pool_ref.users.values().next())
        .cloned();

    let (user_sub, username, attributes) = match user {
        Some(u) => (u.sub.clone(), u.username.clone(), u.attributes.clone()),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                format!("No users in pool {pool_id} — create a user first"),
            )
                .into_response();
        }
    };

    let state_param = params.state.as_deref().unwrap_or("").to_string();
    let _scope = params.scope.as_deref().unwrap_or("openid");

    match params.response_type.as_str() {
        "code" => {
            // Authorization code flow: generate a code and redirect.
            purge_expired_codes(&oauth_state.auth_codes);
            let code = new_code();
            oauth_state.auth_codes.insert(
                code.clone(),
                AuthCodeEntry {
                    pool_id: pool_id.clone(),
                    client_id: params.client_id.clone(),
                    redirect_uri: redirect_uri.clone(),
                    user_sub,
                    username,
                    issued_at: now_epoch(),
                },
            );

            info!(
                pool_id = %pool_id,
                client_id = %params.client_id,
                "OAuth: issued authorization code"
            );

            let mut url = format!("{redirect_uri}?code={code}");
            if !state_param.is_empty() {
                url.push_str(&format!("&state={}", urlencoding(&state_param)));
            }
            Redirect::to(&url).into_response()
        }
        "token" => {
            // Implicit flow: return tokens directly in the fragment.
            let access_tok = jwt::access_token(
                &user_sub,
                &oauth_state.default_region,
                &pool_id,
                &params.client_id,
                &username,
            );
            let id_tok = jwt::id_token(
                &user_sub,
                &oauth_state.default_region,
                &pool_id,
                &params.client_id,
                &username,
                &attributes,
            );

            info!(
                pool_id = %pool_id,
                client_id = %params.client_id,
                "OAuth: implicit flow — issued tokens"
            );

            let mut fragment = format!(
                "access_token={access_tok}&id_token={id_tok}&token_type=Bearer&expires_in=3600"
            );
            if !state_param.is_empty() {
                fragment.push_str(&format!("&state={}", urlencoding(&state_param)));
            }
            let url = format!("{redirect_uri}#{fragment}");
            Redirect::to(&url).into_response()
        }
        other => (
            StatusCode::BAD_REQUEST,
            format!("Unsupported response_type: {other}"),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// 4. Token endpoint
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct TokenForm {
    grant_type: Option<String>,
    code: Option<String>,
    client_id: Option<String>,
    #[allow(dead_code)]
    client_secret: Option<String>,
    #[allow(dead_code)]
    redirect_uri: Option<String>,
    refresh_token: Option<String>,
    #[allow(dead_code)]
    scope: Option<String>,
}

async fn token(
    State(oauth_state): State<Arc<CognitoOAuthState>>,
    Path(pool_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<TokenForm>,
) -> Response {
    // client_id may come from Basic Auth or form body.
    let client_id = form
        .client_id
        .clone()
        .or_else(|| basic_auth_username(&headers))
        .unwrap_or_default();

    let grant_type = form.grant_type.as_deref().unwrap_or("authorization_code");
    let cognito = &oauth_state.cognito;

    match grant_type {
        "authorization_code" => {
            let code = match &form.code {
                Some(c) => c.clone(),
                None => {
                    return error_response(StatusCode::BAD_REQUEST, "invalid_request", "code is required");
                }
            };

            purge_expired_codes(&oauth_state.auth_codes);

            let entry = match oauth_state.auth_codes.remove(&code) {
                Some((_, e)) => e,
                None => {
                    return error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_grant",
                        "Authorization code not found or expired",
                    );
                }
            };

            if entry.pool_id != pool_id {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "Code does not belong to this pool",
                );
            }

            // Look up user by sub.
            let pool = match cognito.user_pools.get(&pool_id) {
                Some(p) => p,
                None => {
                    return error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "User pool not found",
                    );
                }
            };

            let user = pool
                .users
                .values()
                .find(|u| u.sub == entry.user_sub)
                .cloned();

            let (sub, username, attributes) = match user {
                Some(u) => (u.sub.clone(), u.username.clone(), u.attributes.clone()),
                None => {
                    // Fall back to what was in the code entry.
                    (entry.user_sub.clone(), entry.username.clone(), HashMap::new())
                }
            };

            let effective_client_id = if client_id.is_empty() {
                entry.client_id.clone()
            } else {
                client_id.clone()
            };

            let access_tok = jwt::access_token(
                &sub,
                &oauth_state.default_region,
                &pool_id,
                &effective_client_id,
                &username,
            );
            let id_tok = jwt::id_token(
                &sub,
                &oauth_state.default_region,
                &pool_id,
                &effective_client_id,
                &username,
                &attributes,
            );
            let refresh_tok = jwt::refresh_token(&sub);

            info!(
                pool_id = %pool_id,
                username = %username,
                "OAuth: code exchange successful"
            );

            Json(json!({
                "access_token": access_tok,
                "id_token": id_tok,
                "refresh_token": refresh_tok,
                "token_type": "Bearer",
                "expires_in": 3600
            }))
            .into_response()
        }

        "client_credentials" => {
            // Machine-to-machine: return an access token only (no user context).
            let effective_client_id = if client_id.is_empty() {
                "unknown-client".to_string()
            } else {
                client_id.clone()
            };

            let access_tok = jwt::access_token(
                &effective_client_id,
                &oauth_state.default_region,
                &pool_id,
                &effective_client_id,
                &effective_client_id,
            );

            info!(
                pool_id = %pool_id,
                client_id = %effective_client_id,
                "OAuth: client_credentials token issued"
            );

            Json(json!({
                "access_token": access_tok,
                "token_type": "Bearer",
                "expires_in": 3600
            }))
            .into_response()
        }

        "refresh_token" => {
            let refresh_tok = match &form.refresh_token {
                Some(t) => t.clone(),
                None => {
                    return error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "refresh_token is required",
                    );
                }
            };

            // Parse our opaque format: "refresh-{sub}-{uuid}"
            let sub = refresh_tok
                .strip_prefix("refresh-")
                .and_then(|s| s.splitn(2, '-').next())
                .unwrap_or("unknown")
                .to_string();

            let pool = match cognito.user_pools.get(&pool_id) {
                Some(p) => p,
                None => {
                    return error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "User pool not found",
                    );
                }
            };

            let user = pool
                .users
                .values()
                .find(|u| u.sub == sub || refresh_tok.contains(&u.sub))
                .cloned();

            let (user_sub, username, attributes) = match user {
                Some(u) => (u.sub.clone(), u.username.clone(), u.attributes.clone()),
                None => {
                    // Lenient: issue generic tokens for unknown sub.
                    (sub.clone(), sub.clone(), HashMap::new())
                }
            };

            let effective_client_id = if client_id.is_empty() {
                "unknown-client".to_string()
            } else {
                client_id.clone()
            };

            let access_tok = jwt::access_token(
                &user_sub,
                &oauth_state.default_region,
                &pool_id,
                &effective_client_id,
                &username,
            );
            let id_tok = jwt::id_token(
                &user_sub,
                &oauth_state.default_region,
                &pool_id,
                &effective_client_id,
                &username,
                &attributes,
            );
            let new_refresh = jwt::refresh_token(&user_sub);

            info!(
                pool_id = %pool_id,
                username = %username,
                "OAuth: refresh_token exchange successful"
            );

            Json(json!({
                "access_token": access_tok,
                "id_token": id_tok,
                "refresh_token": new_refresh,
                "token_type": "Bearer",
                "expires_in": 3600
            }))
            .into_response()
        }

        other => error_response(
            StatusCode::BAD_REQUEST,
            "unsupported_grant_type",
            &format!("Unsupported grant_type: {other}"),
        ),
    }
}

// ---------------------------------------------------------------------------
// 5. UserInfo endpoint
// ---------------------------------------------------------------------------

async fn userinfo(
    State(oauth_state): State<Arc<CognitoOAuthState>>,
    Path(pool_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let token = match bearer_token(&headers) {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                [("WWW-Authenticate", "Bearer")],
                "Authorization header with Bearer token required",
            )
                .into_response();
        }
    };

    // Extract username from the JWT payload (no sig verification in emulator).
    let username = match jwt::extract_username_from_access_token(&token) {
        Some(u) => u,
        None => {
            return (StatusCode::UNAUTHORIZED, "Invalid access token").into_response();
        }
    };

    let cognito = &oauth_state.cognito;
    let pool = match cognito.user_pools.get(&pool_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                format!("User pool not found: {pool_id}"),
            )
                .into_response();
        }
    };

    let user = match pool.users.get(&username) {
        Some(u) => u.clone(),
        None => {
            // Try by sub (client_credentials tokens use sub=client_id).
            let found = pool
                .users
                .values()
                .find(|u| u.sub == username)
                .cloned();
            match found {
                Some(u) => u,
                None => {
                    return (
                        StatusCode::UNAUTHORIZED,
                        format!("User not found: {username}"),
                    )
                        .into_response();
                }
            }
        }
    };

    let mut claims = json!({
        "sub": user.sub,
        "username": user.username,
    });

    // Merge user attributes.
    if let Some(obj) = claims.as_object_mut() {
        for (k, v) in &user.attributes {
            obj.insert(k.clone(), Value::String(v.clone()));
        }
    }

    Json(claims).into_response()
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Extract the Bearer token from an Authorization header.
fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    auth.strip_prefix("Bearer ").map(|t| t.trim().to_string())
}

/// Extract the username from HTTP Basic Auth credentials.
fn basic_auth_username(headers: &HeaderMap) -> Option<String> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let auth = headers.get("authorization")?.to_str().ok()?;
    let encoded = auth.strip_prefix("Basic ")?;
    let decoded = STANDARD.decode(encoded).ok()?;
    let s = String::from_utf8(decoded).ok()?;
    s.split(':').next().map(|u| u.to_string())
}

/// Build a JSON error response compatible with RFC 6749.
fn error_response(status: StatusCode, error: &str, description: &str) -> Response {
    (
        status,
        Json(json!({
            "error": error,
            "error_description": description
        })),
    )
        .into_response()
}

/// Percent-encode a string for use in a URL query/fragment.
fn urlencoding(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
                vec![c]
            } else {
                // Encode as %HH per byte.
                let mut buf = [0u8; 4];
                let bytes = c.encode_utf8(&mut buf);
                bytes
                    .bytes()
                    .flat_map(|b| {
                        let hi = char::from_digit((b >> 4) as u32, 16).unwrap().to_uppercase().next().unwrap();
                        let lo = char::from_digit((b & 0xf) as u32, 16).unwrap().to_uppercase().next().unwrap();
                        vec!['%', hi, lo]
                    })
                    .collect()
            }
        })
        .collect()
}
