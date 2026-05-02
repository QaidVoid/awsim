/// OAuth2/OIDC endpoints for Cognito hosted UI auth flow.
///
/// These are standard HTTP endpoints (not AWS API calls) mounted directly
/// on the Axum router, accessible without SigV4 auth.
///
/// Endpoints:
///   GET  /cognito/{pool_id}/.well-known/openid-configuration
///   GET  /cognito/{pool_id}/.well-known/jwks.json
///   GET  /cognito/{pool_id}/oauth2/authorize
///   POST /cognito/{pool_id}/oauth2/authorize   (login form submission)
///   POST /cognito/{pool_id}/oauth2/token
///   GET  /cognito/{pool_id}/oauth2/userInfo
///   POST /cognito/{pool_id}/oauth2/revoke
///   GET  /cognito/{pool_id}/logout
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Redirect, Response};
use dashmap::DashMap;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::jwt::{self, GroupRolePair};
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
    /// PKCE code_challenge (base64url-encoded SHA256 or plain).
    pub code_challenge: Option<String>,
    /// "S256" or "plain".
    pub code_challenge_method: Option<String>,
    /// Requested scopes.
    pub scopes: Vec<String>,
    /// OIDC nonce (passed through to ID token).
    pub nonce: Option<String>,
}

/// Shared state for the OAuth/OIDC router.
#[derive(Clone)]
pub struct CognitoOAuthState {
    pub cognito: Arc<CognitoState>,
    pub default_region: String,
    pub default_account_id: String,
    pub auth_codes: Arc<DashMap<String, AuthCodeEntry>>,
    /// Revoked refresh tokens (token string → ()).
    pub revoked_refresh_tokens: Arc<DashMap<String, ()>>,
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
            axum::routing::get(authorize_get).post(authorize_post),
        )
        .route(
            "/cognito/{pool_id}/oauth2/token",
            axum::routing::post(token),
        )
        .route(
            "/cognito/{pool_id}/oauth2/userInfo",
            axum::routing::get(userinfo),
        )
        .route(
            "/cognito/{pool_id}/oauth2/revoke",
            axum::routing::post(revoke),
        )
        .route("/cognito/{pool_id}/logout", axum::routing::get(logout))
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

/// Parse scopes from a space-separated string.
fn parse_scopes(scope_str: &str) -> Vec<String> {
    scope_str.split_whitespace().map(String::from).collect()
}

/// Build GroupRolePair list for a user from pool group data.
fn user_group_role_pairs(
    pool: &crate::state::UserPool,
    user_groups: &[String],
) -> Vec<GroupRolePair> {
    user_groups
        .iter()
        .filter_map(|gname| {
            pool.groups.get(gname).map(|g| GroupRolePair {
                group_name: g.group_name.clone(),
                role_arn: g.role_arn.clone(),
                precedence: g.precedence,
            })
        })
        .collect()
}

/// Verify a PKCE code_verifier against a stored code_challenge.
fn verify_pkce(code_verifier: &str, code_challenge: &str, method: &str) -> bool {
    match method {
        "S256" => {
            use sha2::{Digest, Sha256};
            let hash = Sha256::digest(code_verifier.as_bytes());
            let encoded = base64url_encode(&hash);
            encoded == code_challenge
        }
        "plain" => code_verifier == code_challenge,
        _ => false,
    }
}

fn base64url_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

/// Extract the username AND secret from HTTP Basic Auth credentials (client_id:client_secret).
fn basic_auth_credentials(headers: &HeaderMap) -> Option<(String, String)> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let auth = headers.get("authorization")?.to_str().ok()?;
    let encoded = auth.strip_prefix("Basic ")?;
    let decoded = STANDARD.decode(encoded).ok()?;
    let s = String::from_utf8(decoded).ok()?;
    let mut parts = s.splitn(2, ':');
    let username = parts.next()?.to_string();
    let password = parts.next().unwrap_or("").to_string();
    Some((username, password))
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
                let mut buf = [0u8; 4];
                let bytes = c.encode_utf8(&mut buf);
                bytes
                    .bytes()
                    .flat_map(|b| {
                        // SAFETY: b >> 4 and b & 0xf are always in 0..=15, which are valid base-16 digits.
                        // to_uppercase() on single ASCII hex chars always yields at least one char.
                        let hi = char::from_digit((b >> 4) as u32, 16)
                            .expect("0..=15 is a valid base-16 digit")
                            .to_uppercase()
                            .next()
                            .expect("to_uppercase always yields at least one char for ASCII hex");
                        let lo = char::from_digit((b & 0xf) as u32, 16)
                            .expect("0..=15 is a valid base-16 digit")
                            .to_uppercase()
                            .next()
                            .expect("to_uppercase always yields at least one char for ASCII hex");
                        vec!['%', hi, lo]
                    })
                    .collect()
            }
        })
        .collect()
}

/// Render the login page HTML.
// SAFETY: each parameter is an independent OAuth/OIDC field that must be embedded into the form.
#[allow(clippy::too_many_arguments)]
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[allow(clippy::too_many_arguments)]
fn login_page_html(
    pool_id: &str,
    response_type: &str,
    client_id: &str,
    redirect_uri: &str,
    scope: &str,
    state_param: &str,
    nonce: &str,
    code_challenge: &str,
    code_challenge_method: &str,
    error_msg: Option<&str>,
) -> Response {
    let error_html = error_msg
        .map(|e| format!(r#"<div class="error">{}</div>"#, escape_html(e)))
        .unwrap_or_default();

    let pool_id_e = escape_html(pool_id);
    let response_type_e = escape_html(response_type);
    let client_id_e = escape_html(client_id);
    let redirect_uri_e = escape_html(redirect_uri);
    let scope_e = escape_html(scope);
    let state_param_e = escape_html(state_param);
    let nonce_e = escape_html(nonce);
    let code_challenge_e = escape_html(code_challenge);
    let code_challenge_method_e = escape_html(code_challenge_method);

    let html = format!(
        r#"<!DOCTYPE html>
<html><head><title>AWSim Login</title>
<style>
body {{ font-family: sans-serif; background: #18181b; color: #e4e4e7; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; }}
.card {{ background: #27272a; border: 1px solid #3f3f46; border-radius: 12px; padding: 32px; width: 360px; }}
h2 {{ margin-top: 0; color: #fb923c; }}
input {{ width: 100%; padding: 10px; margin: 8px 0; background: #18181b; border: 1px solid #3f3f46; border-radius: 6px; color: #e4e4e7; box-sizing: border-box; }}
button {{ width: 100%; padding: 10px; background: #ea580c; border: none; border-radius: 6px; color: white; font-weight: bold; cursor: pointer; margin-top: 12px; }}
button:hover {{ background: #f97316; }}
.pool {{ color: #71717a; font-size: 12px; margin-bottom: 16px; }}
.error {{ background: #450a0a; border: 1px solid #991b1b; border-radius: 6px; padding: 10px; margin-bottom: 12px; color: #fca5a5; font-size: 14px; }}
</style></head>
<body>
<div class="card">
<h2>Sign In</h2>
<div class="pool">Pool: {pool_id_e}</div>
{error_html}
<form method="POST" action="/cognito/{pool_id_e}/oauth2/authorize">
<input type="hidden" name="response_type" value="{response_type_e}">
<input type="hidden" name="client_id" value="{client_id_e}">
<input type="hidden" name="redirect_uri" value="{redirect_uri_e}">
<input type="hidden" name="scope" value="{scope_e}">
<input type="hidden" name="state" value="{state_param_e}">
<input type="hidden" name="nonce" value="{nonce_e}">
<input type="hidden" name="code_challenge" value="{code_challenge_e}">
<input type="hidden" name="code_challenge_method" value="{code_challenge_method_e}">
<input type="text" name="username" placeholder="Username" required autofocus>
<input type="password" name="password" placeholder="Password" required>
<button type="submit">Sign In</button>
</form>
</div></body></html>"#
    );

    // SAFETY: Response::builder() only fails on invalid header values, and we're using
    // well-known static strings for status and content-type.
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(Body::from(html))
        .expect("well-known header values cannot fail")
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
        "revocation_endpoint": format!("{base}/oauth2/revoke"),
        "jwks_uri": format!("{}/cognito/{pool_id}/.well-known/jwks.json", state.base_url()),
        "response_types_supported": ["code", "token", "id_token"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
        "scopes_supported": ["openid", "email", "phone", "profile"],
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"],
        "grant_types_supported": ["authorization_code", "implicit", "client_credentials", "refresh_token"],
        "code_challenge_methods_supported": ["S256", "plain"]
    }))
}

// ---------------------------------------------------------------------------
// 2. JWKS
// ---------------------------------------------------------------------------

async fn jwks() -> Json<Value> {
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
// 3a. Authorization endpoint — GET (show login page)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AuthorizeParams {
    response_type: String,
    client_id: String,
    redirect_uri: Option<String>,
    scope: Option<String>,
    state: Option<String>,
    nonce: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
}

async fn authorize_get(
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

    // Validate pool exists.
    if !oauth_state.cognito.user_pools.contains_key(&pool_id) {
        return (
            StatusCode::BAD_REQUEST,
            format!("User pool not found: {pool_id}"),
        )
            .into_response();
    }

    // Validate redirect_uri against client's callback_urls.
    if let Some(pool_ref) = oauth_state.cognito.user_pools.get(&pool_id)
        && let Some(client) = pool_ref.clients.get(&params.client_id)
        && !client.callback_urls.is_empty()
        && !client.callback_urls.contains(&redirect_uri)
    {
        warn!(
            client_id = %params.client_id,
            redirect_uri = %redirect_uri,
            "OAuth authorize: redirect_uri not in callback_urls"
        );
        return (
            StatusCode::BAD_REQUEST,
            "redirect_uri does not match any registered callback URL",
        )
            .into_response();
    }

    login_page_html(
        &pool_id,
        &params.response_type,
        &params.client_id,
        &redirect_uri,
        params.scope.as_deref().unwrap_or("openid"),
        params.state.as_deref().unwrap_or(""),
        params.nonce.as_deref().unwrap_or(""),
        params.code_challenge.as_deref().unwrap_or(""),
        params.code_challenge_method.as_deref().unwrap_or(""),
        None,
    )
}

// ---------------------------------------------------------------------------
// 3b. Authorization endpoint — POST (login form submission)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct AuthorizeForm {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    scope: Option<String>,
    state: Option<String>,
    nonce: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    username: Option<String>,
    password: Option<String>,
}

async fn authorize_post(
    State(oauth_state): State<Arc<CognitoOAuthState>>,
    Path(pool_id): Path<String>,
    Form(form): Form<AuthorizeForm>,
) -> Response {
    let response_type = form.response_type.as_deref().unwrap_or("code").to_string();
    let client_id = form.client_id.as_deref().unwrap_or("").to_string();
    let redirect_uri = match &form.redirect_uri {
        Some(u) if !u.is_empty() => u.clone(),
        _ => {
            return (StatusCode::BAD_REQUEST, "redirect_uri is required").into_response();
        }
    };
    let scope_str = form.scope.as_deref().unwrap_or("openid").to_string();
    let state_param = form.state.as_deref().unwrap_or("").to_string();
    let nonce = form.nonce.clone();
    let code_challenge = form.code_challenge.clone();
    let code_challenge_method = form.code_challenge_method.clone();

    let username = match &form.username {
        Some(u) if !u.is_empty() => u.clone(),
        _ => {
            return login_page_html(
                &pool_id,
                &response_type,
                &client_id,
                &redirect_uri,
                &scope_str,
                &state_param,
                nonce.as_deref().unwrap_or(""),
                code_challenge.as_deref().unwrap_or(""),
                code_challenge_method.as_deref().unwrap_or(""),
                Some("Username is required"),
            );
        }
    };
    let password = form.password.as_deref().unwrap_or("");

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

    // Validate redirect_uri against client callback_urls.
    if let Some(client) = pool_ref.clients.get(&client_id)
        && !client.callback_urls.is_empty()
        && !client.callback_urls.contains(&redirect_uri)
    {
        return (
            StatusCode::BAD_REQUEST,
            "redirect_uri does not match any registered callback URL",
        )
            .into_response();
    }

    // Authenticate user.
    let user = pool_ref.users.get(&username).cloned();
    let user = match user {
        Some(u) => u,
        None => {
            return login_page_html(
                &pool_id,
                &response_type,
                &client_id,
                &redirect_uri,
                &scope_str,
                &state_param,
                nonce.as_deref().unwrap_or(""),
                code_challenge.as_deref().unwrap_or(""),
                code_challenge_method.as_deref().unwrap_or(""),
                Some("Invalid username or password"),
            );
        }
    };

    if user.password != password {
        return login_page_html(
            &pool_id,
            &response_type,
            &client_id,
            &redirect_uri,
            &scope_str,
            &state_param,
            nonce.as_deref().unwrap_or(""),
            code_challenge.as_deref().unwrap_or(""),
            code_challenge_method.as_deref().unwrap_or(""),
            Some("Invalid username or password"),
        );
    }

    if !user.enabled {
        return login_page_html(
            &pool_id,
            &response_type,
            &client_id,
            &redirect_uri,
            &scope_str,
            &state_param,
            nonce.as_deref().unwrap_or(""),
            code_challenge.as_deref().unwrap_or(""),
            code_challenge_method.as_deref().unwrap_or(""),
            Some("User account is disabled"),
        );
    }

    if user.status == "UNCONFIRMED" {
        return login_page_html(
            &pool_id,
            &response_type,
            &client_id,
            &redirect_uri,
            &scope_str,
            &state_param,
            nonce.as_deref().unwrap_or(""),
            code_challenge.as_deref().unwrap_or(""),
            code_challenge_method.as_deref().unwrap_or(""),
            Some("User is not confirmed"),
        );
    }

    if user.status == "FORCE_CHANGE_PASSWORD" {
        return login_page_html(
            &pool_id,
            &response_type,
            &client_id,
            &redirect_uri,
            &scope_str,
            &state_param,
            nonce.as_deref().unwrap_or(""),
            code_challenge.as_deref().unwrap_or(""),
            code_challenge_method.as_deref().unwrap_or(""),
            Some(
                "Password reset required — use AdminSetUserPassword or InitiateAuth with NEW_PASSWORD_REQUIRED",
            ),
        );
    }

    // Validate scopes against client's allowed_oauth_scopes (if configured).
    let requested_scopes = parse_scopes(&scope_str);
    let effective_scopes = if let Some(client) = pool_ref.clients.get(&client_id) {
        if client.allowed_oauth_scopes.is_empty() {
            requested_scopes.clone()
        } else {
            requested_scopes
                .iter()
                .filter(|s| client.allowed_oauth_scopes.contains(s))
                .cloned()
                .collect()
        }
    } else {
        requested_scopes.clone()
    };

    // Collect group role pairs before dropping pool_ref.
    let group_pairs = user_group_role_pairs(&pool_ref, &user.groups);
    let issuer_url = oauth_state.issuer(&pool_id);
    let token_validity = pool_ref
        .clients
        .get(&client_id)
        .map(|c| (c.access_token_validity, c.id_token_validity))
        .unwrap_or((3600, 3600));

    drop(pool_ref);

    match response_type.as_str() {
        "code" => {
            purge_expired_codes(&oauth_state.auth_codes);
            let code = new_code();
            oauth_state.auth_codes.insert(
                code.clone(),
                AuthCodeEntry {
                    pool_id: pool_id.clone(),
                    client_id: client_id.clone(),
                    redirect_uri: redirect_uri.clone(),
                    user_sub: user.sub.clone(),
                    username: user.username.clone(),
                    issued_at: now_epoch(),
                    code_challenge: code_challenge.filter(|s| !s.is_empty()),
                    code_challenge_method: code_challenge_method.filter(|s| !s.is_empty()),
                    scopes: effective_scopes,
                    nonce: nonce.filter(|s| !s.is_empty()),
                },
            );

            info!(
                pool_id = %pool_id,
                client_id = %client_id,
                username = %user.username,
                "OAuth: issued authorization code via login form"
            );

            let mut url = format!("{redirect_uri}?code={code}");
            if !state_param.is_empty() {
                url.push_str(&format!("&state={}", urlencoding(&state_param)));
            }
            Redirect::to(&url).into_response()
        }
        "token" => {
            let attributes = user.attributes.clone();
            let access_tok = jwt::access_token(
                &user.sub,
                &oauth_state.default_region,
                &pool_id,
                &client_id,
                &user.username,
                &effective_scopes,
                &group_pairs,
                Some(&issuer_url),
                token_validity.0,
            );
            let id_tok = jwt::id_token(
                &user.sub,
                &oauth_state.default_region,
                &pool_id,
                &client_id,
                &user.username,
                &attributes,
                &effective_scopes,
                nonce.as_deref(),
                &group_pairs,
                Some(&issuer_url),
                token_validity.1,
            );

            info!(
                pool_id = %pool_id,
                client_id = %client_id,
                username = %user.username,
                "OAuth: implicit flow via login form"
            );

            let mut fragment = format!(
                "access_token={access_tok}&id_token={id_tok}&token_type=Bearer&expires_in={}",
                token_validity.0
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
    client_secret: Option<String>,
    redirect_uri: Option<String>,
    refresh_token: Option<String>,
    scope: Option<String>,
    code_verifier: Option<String>,
}

async fn token(
    State(oauth_state): State<Arc<CognitoOAuthState>>,
    Path(pool_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<TokenForm>,
) -> Response {
    // client_id and client_secret may come from Basic Auth or form body.
    let (basic_client_id, basic_client_secret) = basic_auth_credentials(&headers)
        .map(|(id, sec)| (Some(id), Some(sec)))
        .unwrap_or((None, None));

    let client_id = form
        .client_id
        .clone()
        .or(basic_client_id.clone())
        .unwrap_or_default();

    let client_secret_provided = form.client_secret.clone().or(basic_client_secret.clone());

    let grant_type = form.grant_type.as_deref().unwrap_or("authorization_code");
    let cognito = &oauth_state.cognito;

    {
        let pool = cognito.user_pools.get(&pool_id);
        if let Some(pool) = pool
            && let Some(client) = pool.clients.get(client_id.as_str())
        {
            let flow_name = match grant_type {
                "authorization_code" => "code",
                "implicit" => "implicit",
                "client_credentials" => "client_credentials",
                "refresh_token" => "refresh_token",
                _ => grant_type,
            };
            if !client.allowed_oauth_flows.is_empty()
                && !client.allowed_oauth_flows.contains(&flow_name.to_string())
            {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "unauthorized_client",
                    &format!("Grant type '{}' is not allowed for this client", flow_name),
                );
            }
        }
    }

    match grant_type {
        "authorization_code" => {
            let code = match &form.code {
                Some(c) => c.clone(),
                None => {
                    return error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "code is required",
                    );
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

            let effective_client_id = if client_id.is_empty() {
                entry.client_id.clone()
            } else {
                client_id.clone()
            };

            // Validate redirect_uri if provided.
            if let Some(req_redirect) = &form.redirect_uri
                && !req_redirect.is_empty()
                && *req_redirect != entry.redirect_uri
            {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "redirect_uri mismatch",
                );
            }

            // Look up the pool and client for secret validation.
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

            // Validate client_secret for confidential clients.
            if let Some(client) = pool.clients.get(&effective_client_id)
                && let Some(expected_secret) = &client.client_secret
            {
                match &client_secret_provided {
                    Some(provided) if provided == expected_secret => {
                        // OK
                    }
                    Some(_) => {
                        return error_response(
                            StatusCode::UNAUTHORIZED,
                            "invalid_client",
                            "Invalid client_secret",
                        );
                    }
                    None => {
                        return error_response(
                            StatusCode::UNAUTHORIZED,
                            "invalid_client",
                            "client_secret is required for this client",
                        );
                    }
                }
            }
            // Public clients (no client_secret) don't require it.

            // PKCE verification.
            if let Some(challenge) = &entry.code_challenge {
                let method = entry.code_challenge_method.as_deref().unwrap_or("plain");
                match &form.code_verifier {
                    Some(verifier) => {
                        if !verify_pkce(verifier, challenge, method) {
                            return error_response(
                                StatusCode::BAD_REQUEST,
                                "invalid_grant",
                                "PKCE code_verifier does not match code_challenge",
                            );
                        }
                    }
                    None => {
                        return error_response(
                            StatusCode::BAD_REQUEST,
                            "invalid_request",
                            "code_verifier is required (PKCE)",
                        );
                    }
                }
            }

            // Look up user by sub.
            let user = pool
                .users
                .values()
                .find(|u| u.sub == entry.user_sub)
                .cloned();

            let (sub, username, attributes, group_pairs) = match user {
                Some(ref u) => {
                    let pairs = user_group_role_pairs(&pool, &u.groups);
                    (
                        u.sub.clone(),
                        u.username.clone(),
                        u.attributes.clone(),
                        pairs,
                    )
                }
                None => (
                    entry.user_sub.clone(),
                    entry.username.clone(),
                    HashMap::new(),
                    vec![],
                ),
            };

            let scopes = entry.scopes.clone();
            let nonce = entry.nonce.clone();
            let issuer_url = oauth_state.issuer(&pool_id);

            let validity = oauth_state
                .cognito
                .user_pools
                .get(&pool_id)
                .and_then(|p| {
                    p.clients
                        .get(&effective_client_id)
                        .map(|c| (c.access_token_validity, c.id_token_validity))
                })
                .unwrap_or((3600, 3600));

            let access_tok = jwt::access_token(
                &sub,
                &oauth_state.default_region,
                &pool_id,
                &effective_client_id,
                &username,
                &scopes,
                &group_pairs,
                Some(&issuer_url),
                validity.0,
            );
            let id_tok = jwt::id_token(
                &sub,
                &oauth_state.default_region,
                &pool_id,
                &effective_client_id,
                &username,
                &attributes,
                &scopes,
                nonce.as_deref(),
                &group_pairs,
                Some(&issuer_url),
                validity.1,
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
                "expires_in": validity.0
            }))
            .into_response()
        }

        "client_credentials" => {
            // Machine-to-machine: client_secret is REQUIRED.
            let effective_client_id = if client_id.is_empty() {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "client_id is required for client_credentials",
                );
            } else {
                client_id.clone()
            };

            // Validate client and secret.
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

            if let Some(client) = pool.clients.get(&effective_client_id) {
                if let Some(expected_secret) = &client.client_secret {
                    match &client_secret_provided {
                        Some(provided) if provided == expected_secret => {}
                        Some(_) => {
                            return error_response(
                                StatusCode::UNAUTHORIZED,
                                "invalid_client",
                                "Invalid client_secret",
                            );
                        }
                        None => {
                            return error_response(
                                StatusCode::UNAUTHORIZED,
                                "invalid_client",
                                "client_secret is required",
                            );
                        }
                    }
                } else {
                    // client_credentials grant requires a client with a secret.
                    return error_response(
                        StatusCode::UNAUTHORIZED,
                        "invalid_client",
                        "client_credentials grant requires a confidential client (with client_secret)",
                    );
                }
            } else {
                return error_response(
                    StatusCode::UNAUTHORIZED,
                    "invalid_client",
                    "Client not found in this user pool",
                );
            }

            let requested_scopes = parse_scopes(form.scope.as_deref().unwrap_or(""));
            let effective_scopes = if let Some(client) = pool.clients.get(&effective_client_id) {
                if client.allowed_oauth_scopes.is_empty() {
                    requested_scopes
                } else {
                    requested_scopes
                        .into_iter()
                        .filter(|s| client.allowed_oauth_scopes.contains(s))
                        .collect()
                }
            } else {
                requested_scopes
            };

            let client_access_validity = pool
                .clients
                .get(&effective_client_id)
                .map(|c| c.access_token_validity)
                .unwrap_or(3600);

            // client_credentials is machine-to-machine — no user groups.
            let issuer_url = oauth_state.issuer(&pool_id);
            let access_tok = jwt::access_token(
                &effective_client_id,
                &oauth_state.default_region,
                &pool_id,
                &effective_client_id,
                &effective_client_id,
                &effective_scopes,
                &[],
                Some(&issuer_url),
                client_access_validity,
            );

            info!(
                pool_id = %pool_id,
                client_id = %effective_client_id,
                "OAuth: client_credentials token issued"
            );

            Json(json!({
                "access_token": access_tok,
                "token_type": "Bearer",
                "expires_in": client_access_validity
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

            // Check if token has been revoked.
            if oauth_state
                .revoked_refresh_tokens
                .contains_key(&refresh_tok)
            {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "Refresh token has been revoked",
                );
            }

            // Parse our opaque format: "refresh-{sub}-{uuid}"
            let sub = refresh_tok
                .strip_prefix("refresh-")
                .and_then(|s| s.split('.').next())
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

            let (user_sub, username, attributes, group_pairs) = match user {
                Some(ref u) => {
                    let pairs = user_group_role_pairs(&pool, &u.groups);
                    (
                        u.sub.clone(),
                        u.username.clone(),
                        u.attributes.clone(),
                        pairs,
                    )
                }
                None => (sub.clone(), sub.clone(), HashMap::new(), vec![]),
            };

            let effective_client_id = if client_id.is_empty() {
                "unknown-client".to_string()
            } else {
                client_id.clone()
            };

            let scopes = parse_scopes(form.scope.as_deref().unwrap_or("openid"));
            let issuer_url = oauth_state.issuer(&pool_id);

            let validity = pool
                .clients
                .get(&effective_client_id)
                .map(|c| (c.access_token_validity, c.id_token_validity))
                .unwrap_or((3600, 3600));

            let access_tok = jwt::access_token(
                &user_sub,
                &oauth_state.default_region,
                &pool_id,
                &effective_client_id,
                &username,
                &scopes,
                &group_pairs,
                Some(&issuer_url),
                validity.0,
            );
            let id_tok = jwt::id_token(
                &user_sub,
                &oauth_state.default_region,
                &pool_id,
                &effective_client_id,
                &username,
                &attributes,
                &scopes,
                None,
                &group_pairs,
                Some(&issuer_url),
                validity.1,
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
                "expires_in": validity.0
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
            let found = pool.users.values().find(|u| u.sub == username).cloned();
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

    if let Some(obj) = claims.as_object_mut() {
        for (k, v) in &user.attributes {
            obj.insert(k.clone(), Value::String(v.clone()));
        }
    }

    Json(claims).into_response()
}

// ---------------------------------------------------------------------------
// 6. Revoke endpoint
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct RevokeForm {
    token: Option<String>,
    client_id: Option<String>,
    #[allow(dead_code)]
    client_secret: Option<String>,
}

async fn revoke(
    State(oauth_state): State<Arc<CognitoOAuthState>>,
    Path(pool_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<RevokeForm>,
) -> Response {
    let _ = pool_id; // may be used for client validation in the future

    let (basic_client_id, _basic_client_secret) = basic_auth_credentials(&headers)
        .map(|(id, sec)| (Some(id), Some(sec)))
        .unwrap_or((None, None));

    let _client_id = form.client_id.or(basic_client_id).unwrap_or_default();

    let token = match &form.token {
        Some(t) if !t.is_empty() => t.clone(),
        _ => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "token is required",
            );
        }
    };

    // Add to revocation store.
    oauth_state.revoked_refresh_tokens.insert(token.clone(), ());

    info!("OAuth: revoked refresh token");

    // Per RFC 7009, return 200 with empty body on success.
    StatusCode::OK.into_response()
}

// ---------------------------------------------------------------------------
// 7. Logout endpoint (hosted UI)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct LogoutParams {
    client_id: String,
    logout_uri: Option<String>,
    redirect_uri: Option<String>,
    response_type: Option<String>,
    scope: Option<String>,
    state: Option<String>,
}

async fn logout(
    State(oauth_state): State<Arc<CognitoOAuthState>>,
    Path(pool_id): Path<String>,
    Query(params): Query<LogoutParams>,
) -> Response {
    let pool_ref = match oauth_state.cognito.user_pools.get(&pool_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                format!("User pool not found: {pool_id}"),
            )
                .into_response();
        }
    };

    let client = match pool_ref.clients.get(&params.client_id) {
        Some(c) => c.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Client not found: {}", params.client_id),
            )
                .into_response();
        }
    };
    drop(pool_ref);

    if let Some(logout_uri) = params.logout_uri.as_deref().filter(|s| !s.is_empty()) {
        if !client.logout_urls.is_empty() && !client.logout_urls.contains(&logout_uri.to_string()) {
            warn!(
                client_id = %params.client_id,
                logout_uri = %logout_uri,
                "OAuth logout: logout_uri not in LogoutURLs"
            );
            return (
                StatusCode::BAD_REQUEST,
                "logout_uri does not match any registered LogoutURL",
            )
                .into_response();
        }
        info!(pool_id = %pool_id, client_id = %params.client_id, "OAuth: logout → logout_uri");
        return Redirect::to(logout_uri).into_response();
    }

    if let Some(redirect_uri) = params.redirect_uri.as_deref().filter(|s| !s.is_empty()) {
        if !client.callback_urls.is_empty()
            && !client.callback_urls.contains(&redirect_uri.to_string())
        {
            return (
                StatusCode::BAD_REQUEST,
                "redirect_uri does not match any registered callback URL",
            )
                .into_response();
        }
        let response_type = params.response_type.as_deref().unwrap_or("code");
        let scope = params.scope.as_deref().unwrap_or("openid");
        let state_param = params.state.as_deref().unwrap_or("");
        let mut url = format!(
            "{}/cognito/{pool_id}/oauth2/authorize?client_id={}&response_type={}&redirect_uri={}&scope={}",
            oauth_state.base_url(),
            urlencoding(&params.client_id),
            urlencoding(response_type),
            urlencoding(redirect_uri),
            urlencoding(scope),
        );
        if !state_param.is_empty() {
            url.push_str(&format!("&state={}", urlencoding(state_param)));
        }
        info!(pool_id = %pool_id, client_id = %params.client_id, "OAuth: logout → re-authorize");
        return Redirect::to(&url).into_response();
    }

    (
        StatusCode::BAD_REQUEST,
        "logout requires either logout_uri or redirect_uri",
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    auth.strip_prefix("Bearer ").map(|t| t.trim().to_string())
}
