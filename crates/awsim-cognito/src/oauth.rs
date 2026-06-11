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
    /// In-flight federation state for OIDC IdP redirects.
    pub federation: Arc<crate::federation::FederationState>,
    pub port: u16,
}

impl CognitoOAuthState {
    /// Effective public base URL for the request (`scheme://authority`).
    ///
    /// Derived per-request from `X-Forwarded-Proto` + `Host` so the
    /// issuer / endpoint URLs we emit match exactly what the caller
    /// used to reach us. That's load-bearing for OIDC: Auth.js (and
    /// every other RFC8414-conformant client) verifies that the
    /// `issuer` field of the discovery doc - and the `iss` claim on
    /// every issued JWT - equals the URL it requested. A mismatch
    /// (e.g. caller uses `https://localhost:4567` but we hardcode
    /// `http://localhost:4566`) hard-fails the auth flow.
    ///
    /// Falls back to `http://localhost:{port}` only when neither
    /// header is present, which only happens for synthesised
    /// requests in tests.
    fn base_url(&self, headers: &HeaderMap) -> String {
        let scheme = headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .map(str::to_ascii_lowercase)
            .unwrap_or_else(|| "http".to_string());
        let host = headers
            .get(axum::http::header::HOST)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .unwrap_or_else(|| format!("localhost:{}", self.port));
        format!("{scheme}://{host}")
    }

    fn issuer(&self, headers: &HeaderMap, pool_id: &str) -> String {
        format!("{}/cognito/{}", self.base_url(headers), pool_id)
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
            "/cognito/{pool_id}/oauth2/idpresponse",
            axum::routing::get(idpresponse),
        )
        .route(
            "/cognito/{pool_id}/saml2/idpresponse",
            axum::routing::post(saml_acs),
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
        .route(
            "/cognito/{pool_id}/oauth2/forgot-password",
            axum::routing::get(forgot_password_get).post(forgot_password_post),
        )
        .route(
            "/cognito/{pool_id}/oauth2/forgot-password/confirm",
            axum::routing::get(forgot_password_confirm_get).post(forgot_password_confirm_post),
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

/// Build a `&`-joined query string from `(key, value)` pairs, skipping
/// empty values. Used to forward OAuth params across hosted-UI pages
/// (login → forgot-password → confirm) so the user lands back on
/// `/authorize` with PKCE/state intact after a password reset.
fn build_oauth_query(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{}={}", urlencoding(k), urlencoding(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// The non-COGNITO identity providers a client offers on the hosted UI, so the
/// login page can render "Continue with <IdP>" buttons.
fn client_supported_idps(
    oauth_state: &CognitoOAuthState,
    pool_id: &str,
    client_id: &str,
) -> Vec<String> {
    oauth_state
        .cognito
        .user_pools
        .get(pool_id)
        .and_then(|p| {
            p.clients
                .get(client_id)
                .map(|c| c.supported_identity_providers.clone())
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|p| !p.eq_ignore_ascii_case("COGNITO"))
        .collect()
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
    prefill_username: Option<&str>,
    idps: &[String],
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
    let username_e = prefill_username.map(escape_html).unwrap_or_default();
    // Carry every OAuth parameter onto the forgot-password page so the
    // confirm step can hop back to /authorize without losing PKCE +
    // state.
    let forgot_query = build_oauth_query(&[
        ("response_type", response_type),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("scope", scope),
        ("state", state_param),
        ("nonce", nonce),
        ("code_challenge", code_challenge),
        ("code_challenge_method", code_challenge_method),
    ]);
    let forgot_query_e = escape_html(&forgot_query);

    // "Continue with <IdP>" buttons for each federated provider the client
    // supports, each linking to the authorize endpoint with identity_provider
    // set so the federation round-trip lands back on the app's redirect_uri.
    let idp_section = if idps.is_empty() {
        String::new()
    } else {
        let buttons: String = idps
            .iter()
            .map(|name| {
                let query = if forgot_query.is_empty() {
                    format!("identity_provider={}", urlencoding(name))
                } else {
                    format!("{forgot_query}&identity_provider={}", urlencoding(name))
                };
                format!(
                    r#"<a class="idp-btn" href="/cognito/{pool_id_e}/oauth2/authorize?{}">Continue with {}</a>"#,
                    escape_html(&query),
                    escape_html(name)
                )
            })
            .collect();
        format!(r#"<div class="idps">{buttons}</div><div class="divider"><span>or</span></div>"#)
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>AWSim Login</title>
<style>
* {{ box-sizing: border-box; }}
body {{ font-family: system-ui, -apple-system, "Segoe UI", sans-serif; background: #18181b; color: #e4e4e7; display: flex; justify-content: center; align-items: center; min-height: 100vh; margin: 0; padding: 16px; }}
.card {{ background: #27272a; border: 1px solid #3f3f46; border-radius: 12px; padding: 32px; width: 100%; max-width: 360px; }}
h2 {{ margin-top: 0; color: #fb923c; }}
input {{ width: 100%; padding: 10px; margin: 8px 0; background: #18181b; border: 1px solid #3f3f46; border-radius: 6px; color: #e4e4e7; }}
input:focus {{ outline: none; border-color: #ea580c; }}
button {{ width: 100%; padding: 10px; background: #ea580c; border: none; border-radius: 6px; color: white; font-weight: bold; cursor: pointer; margin-top: 12px; }}
button:hover {{ background: #f97316; }}
.pool {{ color: #71717a; font-size: 12px; margin-bottom: 16px; }}
.error {{ background: #450a0a; border: 1px solid #991b1b; border-radius: 6px; padding: 10px; margin-bottom: 12px; color: #fca5a5; font-size: 14px; }}
.idps {{ display: flex; flex-direction: column; gap: 8px; }}
.idp-btn {{ display: block; padding: 10px; text-align: center; background: #18181b; border: 1px solid #3f3f46; border-radius: 6px; color: #e4e4e7; text-decoration: none; font-size: 14px; }}
.idp-btn:hover {{ border-color: #ea580c; }}
.divider {{ display: flex; align-items: center; text-align: center; color: #71717a; font-size: 12px; margin: 16px 0 4px; }}
.divider::before, .divider::after {{ content: ""; flex: 1; border-bottom: 1px solid #3f3f46; }}
.divider span {{ padding: 0 10px; }}
.links {{ margin-top: 16px; font-size: 13px; text-align: center; }}
.links a {{ color: #fb923c; text-decoration: none; }}
.links a:hover {{ text-decoration: underline; }}
</style></head>
<body>
<div class="card">
<h2>Sign In</h2>
<div class="pool">Pool: {pool_id_e}</div>
{error_html}
{idp_section}
<form method="POST" action="/cognito/{pool_id_e}/oauth2/authorize">
<input type="hidden" name="response_type" value="{response_type_e}">
<input type="hidden" name="client_id" value="{client_id_e}">
<input type="hidden" name="redirect_uri" value="{redirect_uri_e}">
<input type="hidden" name="scope" value="{scope_e}">
<input type="hidden" name="state" value="{state_param_e}">
<input type="hidden" name="nonce" value="{nonce_e}">
<input type="hidden" name="code_challenge" value="{code_challenge_e}">
<input type="hidden" name="code_challenge_method" value="{code_challenge_method_e}">
<input type="text" name="username" placeholder="Username" required autofocus value="{username_e}">
<input type="password" name="password" placeholder="Password" required>
<button type="submit">Sign In</button>
</form>
<div class="links"><a href="/cognito/{pool_id_e}/oauth2/forgot-password?{forgot_query_e}">Forgot password?</a></div>
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

#[allow(clippy::too_many_arguments)]
fn change_password_page_html(
    pool_id: &str,
    response_type: &str,
    client_id: &str,
    redirect_uri: &str,
    scope: &str,
    state_param: &str,
    nonce: &str,
    code_challenge: &str,
    code_challenge_method: &str,
    username: &str,
    temp_password: &str,
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
    let username_e = escape_html(username);
    let temp_password_e = escape_html(temp_password);

    let html = format!(
        r#"<!DOCTYPE html>
<html><head><title>AWSim Change Password</title>
<style>
body {{ font-family: sans-serif; background: #18181b; color: #e4e4e7; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; }}
.card {{ background: #27272a; border: 1px solid #3f3f46; border-radius: 12px; padding: 32px; width: 360px; }}
h2 {{ margin-top: 0; color: #fb923c; }}
input {{ width: 100%; padding: 10px; margin: 8px 0; background: #18181b; border: 1px solid #3f3f46; border-radius: 6px; color: #e4e4e7; box-sizing: border-box; }}
button {{ width: 100%; padding: 10px; background: #ea580c; border: none; border-radius: 6px; color: white; font-weight: bold; cursor: pointer; margin-top: 12px; }}
button:hover {{ background: #f97316; }}
.pool {{ color: #71717a; font-size: 12px; margin-bottom: 16px; }}
.notice {{ color: #a1a1aa; font-size: 13px; margin-bottom: 12px; }}
.error {{ background: #450a0a; border: 1px solid #991b1b; border-radius: 6px; padding: 10px; margin-bottom: 12px; color: #fca5a5; font-size: 14px; }}
</style></head>
<body>
<div class="card">
<h2>Change Password</h2>
<div class="pool">Pool: {pool_id_e}</div>
<div class="notice">A new password is required for {username_e} before sign-in.</div>
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
<input type="hidden" name="username" value="{username_e}">
<input type="hidden" name="password" value="{temp_password_e}">
<input type="password" name="new_password" placeholder="New password" required autofocus>
<input type="password" name="confirm_password" placeholder="Confirm new password" required>
<button type="submit">Set Password</button>
</form>
</div></body></html>"#
    );

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(Body::from(html))
        .expect("well-known header values cannot fail")
}

/// Render the "request a reset code" page (forgot-password step 1).
#[allow(clippy::too_many_arguments)]
fn forgot_password_page_html(
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
    info_msg: Option<&str>,
    prefill_username: Option<&str>,
) -> Response {
    let pool_id_e = escape_html(pool_id);
    let username_e = prefill_username.map(escape_html).unwrap_or_default();
    let error_html = error_msg
        .map(|e| format!(r#"<div class="error">{}</div>"#, escape_html(e)))
        .unwrap_or_default();
    let info_html = info_msg
        .map(|m| format!(r#"<div class="notice">{}</div>"#, escape_html(m)))
        .unwrap_or_default();
    let oauth_query = build_oauth_query(&[
        ("response_type", response_type),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("scope", scope),
        ("state", state_param),
        ("nonce", nonce),
        ("code_challenge", code_challenge),
        ("code_challenge_method", code_challenge_method),
    ]);
    let oauth_query_e = escape_html(&oauth_query);
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
<html><head><title>AWSim Forgot Password</title>
<style>
body {{ font-family: sans-serif; background: #18181b; color: #e4e4e7; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; }}
.card {{ background: #27272a; border: 1px solid #3f3f46; border-radius: 12px; padding: 32px; width: 360px; }}
h2 {{ margin-top: 0; color: #fb923c; }}
input {{ width: 100%; padding: 10px; margin: 8px 0; background: #18181b; border: 1px solid #3f3f46; border-radius: 6px; color: #e4e4e7; box-sizing: border-box; }}
button {{ width: 100%; padding: 10px; background: #ea580c; border: none; border-radius: 6px; color: white; font-weight: bold; cursor: pointer; margin-top: 12px; }}
button:hover {{ background: #f97316; }}
.pool {{ color: #71717a; font-size: 12px; margin-bottom: 16px; }}
.notice {{ color: #a1a1aa; font-size: 13px; margin-bottom: 12px; }}
.error {{ background: #450a0a; border: 1px solid #991b1b; border-radius: 6px; padding: 10px; margin-bottom: 12px; color: #fca5a5; font-size: 14px; }}
.links {{ margin-top: 16px; font-size: 13px; text-align: center; }}
.links a {{ color: #fb923c; text-decoration: none; }}
.links a:hover {{ text-decoration: underline; }}
</style></head>
<body>
<div class="card">
<h2>Reset password</h2>
<div class="pool">Pool: {pool_id_e}</div>
{info_html}{error_html}
<form method="POST" action="/cognito/{pool_id_e}/oauth2/forgot-password?{oauth_query_e}">
<input type="hidden" name="response_type" value="{response_type_e}">
<input type="hidden" name="client_id" value="{client_id_e}">
<input type="hidden" name="redirect_uri" value="{redirect_uri_e}">
<input type="hidden" name="scope" value="{scope_e}">
<input type="hidden" name="state" value="{state_param_e}">
<input type="hidden" name="nonce" value="{nonce_e}">
<input type="hidden" name="code_challenge" value="{code_challenge_e}">
<input type="hidden" name="code_challenge_method" value="{code_challenge_method_e}">
<input type="text" name="username" placeholder="Username" required autofocus value="{username_e}">
<button type="submit">Send reset code</button>
</form>
<div class="links"><a href="/cognito/{pool_id_e}/oauth2/authorize?{oauth_query_e}">Back to sign in</a></div>
</div></body></html>"#
    );
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(Body::from(html))
        .expect("well-known header values cannot fail")
}

/// Render the "enter code + new password" page (forgot-password step 2).
#[allow(clippy::too_many_arguments)]
fn forgot_password_confirm_page_html(
    pool_id: &str,
    response_type: &str,
    client_id: &str,
    redirect_uri: &str,
    scope: &str,
    state_param: &str,
    nonce: &str,
    code_challenge: &str,
    code_challenge_method: &str,
    username: &str,
    code_hint: Option<&str>,
    error_msg: Option<&str>,
) -> Response {
    let pool_id_e = escape_html(pool_id);
    let username_e = escape_html(username);
    let error_html = error_msg
        .map(|e| format!(r#"<div class="error">{}</div>"#, escape_html(e)))
        .unwrap_or_default();
    // Show the freshly-issued code right on the page in dev mode so
    // the user doesn't have to scrape it from awsim's logs. This is
    // explicitly an emulator affordance — real Cognito would email it.
    let code_html = code_hint
        .map(|c| {
            format!(
                r#"<div class="hint">DEV: code is <code>{}</code></div>"#,
                escape_html(c)
            )
        })
        .unwrap_or_default();
    let oauth_query = build_oauth_query(&[
        ("response_type", response_type),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("scope", scope),
        ("state", state_param),
        ("nonce", nonce),
        ("code_challenge", code_challenge),
        ("code_challenge_method", code_challenge_method),
    ]);
    let oauth_query_e = escape_html(&oauth_query);
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
<html><head><title>AWSim Confirm Reset</title>
<style>
body {{ font-family: sans-serif; background: #18181b; color: #e4e4e7; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; }}
.card {{ background: #27272a; border: 1px solid #3f3f46; border-radius: 12px; padding: 32px; width: 360px; }}
h2 {{ margin-top: 0; color: #fb923c; }}
input {{ width: 100%; padding: 10px; margin: 8px 0; background: #18181b; border: 1px solid #3f3f46; border-radius: 6px; color: #e4e4e7; box-sizing: border-box; }}
button {{ width: 100%; padding: 10px; background: #ea580c; border: none; border-radius: 6px; color: white; font-weight: bold; cursor: pointer; margin-top: 12px; }}
button:hover {{ background: #f97316; }}
.pool {{ color: #71717a; font-size: 12px; margin-bottom: 16px; }}
.error {{ background: #450a0a; border: 1px solid #991b1b; border-radius: 6px; padding: 10px; margin-bottom: 12px; color: #fca5a5; font-size: 14px; }}
.hint {{ background: #1f2937; border: 1px solid #374151; border-radius: 6px; padding: 8px 10px; margin-bottom: 12px; font-size: 12px; color: #fbbf24; }}
.hint code {{ font-family: monospace; font-size: 14px; background: transparent; }}
.links {{ margin-top: 16px; font-size: 13px; text-align: center; }}
.links a {{ color: #fb923c; text-decoration: none; }}
.links a:hover {{ text-decoration: underline; }}
</style></head>
<body>
<div class="card">
<h2>Set new password</h2>
<div class="pool">Pool: {pool_id_e}</div>
{code_html}{error_html}
<form method="POST" action="/cognito/{pool_id_e}/oauth2/forgot-password/confirm?{oauth_query_e}">
<input type="hidden" name="response_type" value="{response_type_e}">
<input type="hidden" name="client_id" value="{client_id_e}">
<input type="hidden" name="redirect_uri" value="{redirect_uri_e}">
<input type="hidden" name="scope" value="{scope_e}">
<input type="hidden" name="state" value="{state_param_e}">
<input type="hidden" name="nonce" value="{nonce_e}">
<input type="hidden" name="code_challenge" value="{code_challenge_e}">
<input type="hidden" name="code_challenge_method" value="{code_challenge_method_e}">
<input type="hidden" name="username" value="{username_e}">
<input type="text" name="code" placeholder="Verification code" required autofocus inputmode="numeric" pattern="[0-9]*">
<input type="password" name="new_password" placeholder="New password" required>
<input type="password" name="confirm_password" placeholder="Confirm new password" required>
<button type="submit">Set password</button>
</form>
<div class="links"><a href="/cognito/{pool_id_e}/oauth2/authorize?{oauth_query_e}">Back to sign in</a></div>
</div></body></html>"#
    );
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
    headers: HeaderMap,
) -> Json<Value> {
    let base = state.issuer(&headers, &pool_id);
    let public_base = state.base_url(&headers);
    Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/oauth2/authorize"),
        "token_endpoint": format!("{base}/oauth2/token"),
        "userinfo_endpoint": format!("{base}/oauth2/userInfo"),
        "revocation_endpoint": format!("{base}/oauth2/revoke"),
        "jwks_uri": format!("{public_base}/cognito/{pool_id}/.well-known/jwks.json"),
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
    Json(crate::keys::jwks_document())
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
    /// When set, the user is redirected to the matching `IdentityProvider`
    /// for federated sign-in instead of the local username/password
    /// form. The eventual code lands at our `/oauth2/idpresponse`
    /// endpoint, which finishes the Cognito flow and bounces back to
    /// `redirect_uri` with the app's authorization code.
    identity_provider: Option<String>,
}

async fn authorize_get(
    State(oauth_state): State<Arc<CognitoOAuthState>>,
    Path(pool_id): Path<String>,
    Query(params): Query<AuthorizeParams>,
    headers: HeaderMap,
) -> Response {
    let redirect_uri = match &params.redirect_uri {
        Some(u) => u.clone(),
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "redirect_uri is required.",
            );
        }
    };

    // Validate pool exists.
    if !oauth_state.cognito.user_pools.contains_key(&pool_id) {
        return (
            StatusCode::BAD_REQUEST,
            format!("User pool {pool_id} does not exist."),
        )
            .into_response();
    }

    // The client must exist on the pool.
    if let Some(pool_ref) = oauth_state.cognito.user_pools.get(&pool_id)
        && !pool_ref.clients.contains_key(&params.client_id)
    {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_client",
            &format!("Client {} not found.", params.client_id),
        );
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
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "redirect_uri does not match any registered callback URL.",
        );
    }

    // Federation: when ?identity_provider=Foo is present, look up
    // Foo on the pool, redirect the user to its authorize URL, and
    // park the original Cognito-side authorize parameters until the
    // IdP redirects back to our /idpresponse endpoint.
    if let Some(provider_name) = params
        .identity_provider
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        return start_federation(
            &oauth_state,
            &headers,
            &pool_id,
            provider_name,
            &params,
            &redirect_uri,
        )
        .await;
    }

    let idps = client_supported_idps(&oauth_state, &pool_id, &params.client_id);
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
        None,
        &idps,
    )
}

async fn start_federation(
    oauth_state: &Arc<CognitoOAuthState>,
    headers: &HeaderMap,
    pool_id: &str,
    provider_name: &str,
    params: &AuthorizeParams,
    redirect_uri: &str,
) -> Response {
    let pool_ref = match oauth_state.cognito.user_pools.get(pool_id) {
        Some(p) => p,
        None => {
            return (StatusCode::BAD_REQUEST, "user pool not found").into_response();
        }
    };
    let idp = match pool_ref
        .identity_providers
        .iter()
        .find(|i| i.provider_name == provider_name)
        .cloned()
    {
        Some(i) => i,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                format!("identity_provider {provider_name} is not registered on this pool"),
            )
                .into_response();
        }
    };
    drop(pool_ref);

    if idp.provider_type.eq_ignore_ascii_case("SAML") {
        return start_saml_federation(oauth_state, headers, pool_id, &idp, params, redirect_uri)
            .await;
    }

    let cfg = match crate::federation::parse_oidc_config(&idp) {
        Ok(c) => c,
        Err(e) => return (StatusCode::BAD_REQUEST, e.message).into_response(),
    };
    let discovery =
        match crate::federation::resolve_discovery(&oauth_state.federation, &idp, &cfg).await {
            Ok(d) => d,
            Err(e) => return (StatusCode::BAD_GATEWAY, e.message).into_response(),
        };

    // Stash original Cognito-side authorize params so the
    // /idpresponse callback can recover them.
    let pending = crate::federation::PendingFederation {
        pool_id: pool_id.to_string(),
        client_id: params.client_id.clone(),
        redirect_uri: redirect_uri.to_string(),
        scope: params.scope.clone().unwrap_or_else(|| "openid".to_string()),
        app_state: params.state.clone().unwrap_or_default(),
        nonce: params.nonce.clone().filter(|s| !s.is_empty()),
        code_challenge: params.code_challenge.clone().filter(|s| !s.is_empty()),
        code_challenge_method: params
            .code_challenge_method
            .clone()
            .filter(|s| !s.is_empty()),
        provider_name: provider_name.to_string(),
        issued_at: now_epoch(),
    };
    let state_token = crate::federation::stash(&oauth_state.federation, pending);

    // Build our /idpresponse URL using the same base-url resolution
    // we use for the issuer / token endpoints, so the IdP's redirect
    // hits us back on the exact host/scheme the user came in on.
    let cognito_callback = format!(
        "{}/cognito/{pool_id}/oauth2/idpresponse",
        oauth_state.base_url(headers)
    );

    let url = crate::federation::build_idp_authorize_url(
        &discovery,
        &cfg,
        &cognito_callback,
        &state_token,
        params.nonce.as_deref(),
    );
    info!(
        pool_id = %pool_id,
        provider = %provider_name,
        "Cognito federation: redirecting to IdP authorize"
    );
    Redirect::to(&url).into_response()
}

/// Form body the IdP POSTs to the SAML assertion consumer service.
#[derive(Deserialize, Default)]
struct SamlAcsForm {
    #[serde(rename = "SAMLResponse")]
    saml_response: Option<String>,
    #[serde(rename = "RelayState")]
    relay_state: Option<String>,
}

/// SP-initiated SAML: build an AuthnRequest, park the app's authorize params
/// under a relay token, and redirect the browser to the IdP's SSO URL.
async fn start_saml_federation(
    oauth_state: &Arc<CognitoOAuthState>,
    headers: &HeaderMap,
    pool_id: &str,
    idp: &crate::state::IdentityProvider,
    params: &AuthorizeParams,
    redirect_uri: &str,
) -> Response {
    let cfg = match crate::saml::parse_saml_config(idp) {
        Ok(c) => c,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "invalid_request", &e.message),
    };

    let pending = crate::federation::PendingFederation {
        pool_id: pool_id.to_string(),
        client_id: params.client_id.clone(),
        redirect_uri: redirect_uri.to_string(),
        scope: params.scope.clone().unwrap_or_else(|| "openid".to_string()),
        app_state: params.state.clone().unwrap_or_default(),
        nonce: params.nonce.clone().filter(|s| !s.is_empty()),
        code_challenge: params.code_challenge.clone().filter(|s| !s.is_empty()),
        code_challenge_method: params
            .code_challenge_method
            .clone()
            .filter(|s| !s.is_empty()),
        provider_name: idp.provider_name.clone(),
        issued_at: now_epoch(),
    };
    let relay_state = crate::federation::stash(&oauth_state.federation, pending);

    let acs_url = format!(
        "{}/cognito/{pool_id}/saml2/idpresponse",
        oauth_state.base_url(headers)
    );
    let request_id = format!("_{}", new_code());
    let issue_instant = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let url = crate::saml::build_authn_request_url(
        &cfg.sso_redirect_url,
        &crate::saml::sp_entity_id(pool_id),
        &acs_url,
        &relay_state,
        &request_id,
        &issue_instant,
    );
    info!(
        pool_id = %pool_id,
        provider = %idp.provider_name,
        "Cognito federation: redirecting to SAML IdP SSO"
    );
    Redirect::to(&url).into_response()
}

/// SAML assertion consumer service: the IdP POSTs a base64 `SAMLResponse`
/// (HTTP-POST binding) with the `RelayState` we set on the AuthnRequest.
async fn saml_acs(
    State(oauth_state): State<Arc<CognitoOAuthState>>,
    Path(pool_id): Path<String>,
    Form(form): Form<SamlAcsForm>,
) -> Response {
    saml_acs_inner(&oauth_state, pool_id, form)
}

/// Synchronous core of [`saml_acs`]; split out so it is directly testable
/// without an async runtime (the handler does no real I/O).
fn saml_acs_inner(
    oauth_state: &Arc<CognitoOAuthState>,
    pool_id: String,
    form: SamlAcsForm,
) -> Response {
    let Some(response_b64) = form.saml_response.filter(|s| !s.is_empty()) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "SAMLResponse is required",
        );
    };
    let relay_state = form.relay_state.unwrap_or_default();

    let xml = match base64_decode_standard(&response_b64) {
        Some(bytes) => bytes,
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "SAMLResponse is not valid base64",
            );
        }
    };
    let assertion = match crate::saml::parse_saml_response(&xml) {
        Ok(a) => a,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "invalid_request", &e.message),
    };

    // RelayState carries the relay token from the SP-initiated AuthnRequest.
    let pending = match crate::federation::take(&oauth_state.federation, &relay_state) {
        Some(p) => p,
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Unknown or expired RelayState; IdP-initiated SSO is not supported",
            );
        }
    };
    if pending.pool_id != pool_id {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "RelayState does not match this user pool",
        );
    }

    let idp = match oauth_state.cognito.user_pools.get(&pool_id).and_then(|p| {
        p.identity_providers
            .iter()
            .find(|i| i.provider_name == pending.provider_name)
            .cloned()
    }) {
        Some(i) => i,
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "identity_provider no longer registered",
            );
        }
    };

    let mapped = crate::federation::map_attributes(&idp, &assertion.attributes);

    let mut pool = match oauth_state.cognito.user_pools.get_mut(&pool_id) {
        Some(p) => p,
        None => {
            return error_response(StatusCode::BAD_REQUEST, "invalid_request", "user pool gone");
        }
    };
    let (username, user_sub) = crate::federation::upsert_user(
        &mut pool,
        &pending.provider_name,
        &assertion.name_id,
        mapped,
    );
    drop(pool);

    let scopes = parse_scopes(&pending.scope);
    let cognito_code = crate::federation::mint_cognito_code(
        oauth_state,
        &pool_id,
        &pending.client_id,
        &pending.redirect_uri,
        &user_sub,
        &username,
        scopes,
        pending.nonce,
        pending.code_challenge,
        pending.code_challenge_method,
    );

    let mut url = format!("{}?code={cognito_code}", pending.redirect_uri);
    if !pending.app_state.is_empty() {
        url.push_str(&format!("&state={}", urlencoding(&pending.app_state)));
    }
    info!(
        pool_id = %pool_id,
        provider = %pending.provider_name,
        username = %username,
        "Cognito SAML federation: handed app the final code"
    );
    Redirect::to(&url).into_response()
}

/// Decode standard-alphabet base64 (the HTTP-POST binding encoding).
fn base64_decode_standard(s: &str) -> Option<Vec<u8>> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    STANDARD.decode(s.trim()).ok()
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
    new_password: Option<String>,
    confirm_password: Option<String>,
}

async fn authorize_post(
    State(oauth_state): State<Arc<CognitoOAuthState>>,
    Path(pool_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<AuthorizeForm>,
) -> Response {
    let response_type = form.response_type.as_deref().unwrap_or("code").to_string();
    let client_id = form.client_id.as_deref().unwrap_or("").to_string();
    let redirect_uri = match &form.redirect_uri {
        Some(u) if !u.is_empty() => u.clone(),
        _ => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "redirect_uri is required.",
            );
        }
    };
    let scope_str = form.scope.as_deref().unwrap_or("openid").to_string();
    let state_param = form.state.as_deref().unwrap_or("").to_string();
    let nonce = form.nonce.clone();
    let code_challenge = form.code_challenge.clone();
    let code_challenge_method = form.code_challenge_method.clone();
    // Federated providers for this client, for the login page's IdP buttons
    // on any re-render below.
    let idps = client_supported_idps(&oauth_state, &pool_id, &client_id);

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
                None,
                &idps,
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
                format!("User pool {pool_id} does not exist."),
            )
                .into_response();
        }
    };

    // Validate redirect_uri against client callback_urls.
    if let Some(client) = pool_ref.clients.get(&client_id)
        && !client.callback_urls.is_empty()
        && !client.callback_urls.contains(&redirect_uri)
    {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "redirect_uri does not match any registered callback URL.",
        );
    }

    let resolved_username =
        crate::operations::users::resolve_username_for_signin(&pool_ref, &username);
    let user = resolved_username
        .as_ref()
        .and_then(|u| pool_ref.users.get(u).cloned());
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
                Some(&username),
                &idps,
            );
        }
    };
    let username = resolved_username.expect("user lookup matched, so resolution succeeded");

    if !crate::password::verify(password, &user.password_hash) {
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
            Some(&username),
            &idps,
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
            Some(&username),
            &idps,
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
            Some(&username),
            &idps,
        );
    }

    if user.status == "RESET_REQUIRED" {
        // Mirrors the `PasswordResetRequiredException` the SDK paths
        // return — the user must complete a forgot-password flow before
        // direct sign-in is allowed again.
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
                "Password reset required — finish the forgot-password flow or have an admin run AdminSetUserPassword to clear the reset state.",
            ),
            Some(&username),
            &idps,
        );
    }

    let policy_for_change = pool_ref.policies.clone();
    drop(pool_ref);

    let mut user = user;
    if user.status == "FORCE_CHANGE_PASSWORD" {
        let new_password = form.new_password.as_deref().unwrap_or("");
        let confirm_password = form.confirm_password.as_deref().unwrap_or("");

        if new_password.is_empty() {
            return change_password_page_html(
                &pool_id,
                &response_type,
                &client_id,
                &redirect_uri,
                &scope_str,
                &state_param,
                nonce.as_deref().unwrap_or(""),
                code_challenge.as_deref().unwrap_or(""),
                code_challenge_method.as_deref().unwrap_or(""),
                &username,
                password,
                None,
            );
        }

        if new_password != confirm_password {
            return change_password_page_html(
                &pool_id,
                &response_type,
                &client_id,
                &redirect_uri,
                &scope_str,
                &state_param,
                nonce.as_deref().unwrap_or(""),
                code_challenge.as_deref().unwrap_or(""),
                code_challenge_method.as_deref().unwrap_or(""),
                &username,
                password,
                Some("Passwords do not match"),
            );
        }

        if let Err(err) =
            crate::operations::auth_policy::validate_password(&policy_for_change, new_password)
        {
            return change_password_page_html(
                &pool_id,
                &response_type,
                &client_id,
                &redirect_uri,
                &scope_str,
                &state_param,
                nonce.as_deref().unwrap_or(""),
                code_challenge.as_deref().unwrap_or(""),
                code_challenge_method.as_deref().unwrap_or(""),
                &username,
                password,
                Some(&err.message),
            );
        }

        let new_hash = match crate::password::hash(new_password) {
            Ok(h) => h,
            Err(e) => {
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
                    Some(&e.message),
                    Some(&username),
                    &idps,
                );
            }
        };

        let (srp_salt, srp_verifier) =
            crate::password::srp_material(&pool_id, &username, new_password);

        if let Some(mut pool_mut) = cognito.user_pools.get_mut(&pool_id)
            && let Some(user_mut) = pool_mut.users.get_mut(&username)
        {
            user_mut.password_hash = new_hash.clone();
            user_mut.srp_salt = Some(srp_salt.clone());
            user_mut.srp_verifier = Some(srp_verifier.clone());
            user_mut.status = "CONFIRMED".to_string();
            user_mut.failed_login_attempts = 0;
            user_mut.locked_until_secs = None;
        }

        info!(
            pool_id = %pool_id,
            username = %username,
            "OAuth: completed FORCE_CHANGE_PASSWORD via login form"
        );

        user.password_hash = new_hash;
        user.srp_salt = Some(srp_salt);
        user.srp_verifier = Some(srp_verifier);
        user.status = "CONFIRMED".to_string();
    }
    let user = user;
    let pool_ref = match cognito.user_pools.get(&pool_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                format!("User pool {pool_id} does not exist."),
            )
                .into_response();
        }
    };

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
    let issuer_url = oauth_state.issuer(&headers, &pool_id);
    let token_validity = pool_ref
        .clients
        .get(&client_id)
        .map(|c| (c.access_token_validity, c.id_token_validity))
        .unwrap_or((3600, 3600));
    // Resolve the client's ReadAttributes before dropping pool_ref so
    // the ID token below only carries attributes the client may read.
    let read_attrs =
        crate::operations::users::client_read_set(&pool_ref, &client_id).unwrap_or_default();

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
                None,
            );
            let id_tok = jwt::id_token(
                &user.sub,
                &oauth_state.default_region,
                &pool_id,
                &client_id,
                &user.username,
                &attributes,
                &read_attrs,
                &effective_scopes,
                nonce.as_deref(),
                &group_pairs,
                Some(&issuer_url),
                token_validity.1,
                None,
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
// 3c. IdP response endpoint — accepts the federated IdP's redirect
//     after a successful authorize, exchanges the IdP code for an
//     ID token, validates + maps claims, upserts the federated user,
//     mints a Cognito authorization code, and finally redirects back
//     to the original app `redirect_uri`.
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct IdpResponseParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn idpresponse(
    State(oauth_state): State<Arc<CognitoOAuthState>>,
    Path(pool_id): Path<String>,
    Query(params): Query<IdpResponseParams>,
    headers: HeaderMap,
) -> Response {
    if let Some(err) = params.error {
        let msg = params.error_description.unwrap_or_default();
        warn!(error = %err, description = %msg, "Cognito federation: IdP returned error");
        return (StatusCode::BAD_GATEWAY, format!("IdP error: {err} {msg}")).into_response();
    }

    let state_token = match params.state.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return (StatusCode::BAD_REQUEST, "missing state").into_response(),
    };
    let code = match params.code.as_deref().filter(|s| !s.is_empty()) {
        Some(c) => c,
        None => return (StatusCode::BAD_REQUEST, "missing code").into_response(),
    };

    let pending = match crate::federation::take(&oauth_state.federation, state_token) {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "state token unknown or expired - retry the sign-in flow",
            )
                .into_response();
        }
    };

    if pending.pool_id != pool_id {
        return (
            StatusCode::BAD_REQUEST,
            "state token does not match this pool",
        )
            .into_response();
    }

    let pool_ref = match oauth_state.cognito.user_pools.get(&pool_id) {
        Some(p) => p,
        None => return (StatusCode::BAD_REQUEST, "user pool not found").into_response(),
    };
    let idp = match pool_ref
        .identity_providers
        .iter()
        .find(|i| i.provider_name == pending.provider_name)
        .cloned()
    {
        Some(i) => i,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                format!(
                    "identity_provider {} no longer registered",
                    pending.provider_name
                ),
            )
                .into_response();
        }
    };
    drop(pool_ref);

    let cfg = match crate::federation::parse_oidc_config(&idp) {
        Ok(c) => c,
        Err(e) => return (StatusCode::BAD_REQUEST, e.message).into_response(),
    };
    let discovery =
        match crate::federation::resolve_discovery(&oauth_state.federation, &idp, &cfg).await {
            Ok(d) => d,
            Err(e) => return (StatusCode::BAD_GATEWAY, e.message).into_response(),
        };

    let cognito_callback = format!(
        "{}/cognito/{pool_id}/oauth2/idpresponse",
        oauth_state.base_url(&headers)
    );
    let id_token =
        match crate::federation::exchange_code(&discovery, &cfg, code, &cognito_callback).await {
            Ok(t) => t,
            Err(e) => return (StatusCode::BAD_GATEWAY, e.message).into_response(),
        };

    let claims = match crate::federation::verify_id_token(&discovery, &cfg, &id_token).await {
        Ok(c) => c,
        Err(e) => return (StatusCode::BAD_GATEWAY, e.message).into_response(),
    };
    let idp_sub = match crate::federation::extract_idp_sub(&claims) {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_GATEWAY, e.message).into_response(),
    };
    let mapped = crate::federation::map_attributes(&idp, &claims);

    let mut pool = match oauth_state.cognito.user_pools.get_mut(&pool_id) {
        Some(p) => p,
        None => return (StatusCode::BAD_REQUEST, "user pool gone").into_response(),
    };
    let (username, user_sub) =
        crate::federation::upsert_user(&mut pool, &pending.provider_name, &idp_sub, mapped);
    drop(pool);

    let scopes = parse_scopes(&pending.scope);
    let cognito_code = crate::federation::mint_cognito_code(
        &oauth_state,
        &pool_id,
        &pending.client_id,
        &pending.redirect_uri,
        &user_sub,
        &username,
        scopes,
        pending.nonce,
        pending.code_challenge,
        pending.code_challenge_method,
    );

    let mut url = format!("{}?code={cognito_code}", pending.redirect_uri);
    if !pending.app_state.is_empty() {
        url.push_str(&format!("&state={}", urlencoding(&pending.app_state)));
    }
    info!(
        pool_id = %pool_id,
        provider = %pending.provider_name,
        username = %username,
        "Cognito federation: handed app the final code"
    );
    Redirect::to(&url).into_response()
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

            // The client_id presented at the token endpoint must match the one
            // the authorization code was issued to; a different client cannot
            // redeem another client's code.
            if !client_id.is_empty() && client_id != entry.client_id {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "client_id mismatch.",
                );
            }
            let effective_client_id = entry.client_id.clone();

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
            let issuer_url = oauth_state.issuer(&headers, &pool_id);

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
            let read_attrs = oauth_state
                .cognito
                .user_pools
                .get(&pool_id)
                .and_then(|p| crate::operations::users::client_read_set(&p, &effective_client_id))
                .unwrap_or_default();

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
                None,
            );
            let id_tok = jwt::id_token(
                &sub,
                &oauth_state.default_region,
                &pool_id,
                &effective_client_id,
                &username,
                &attributes,
                &read_attrs,
                &scopes,
                nonce.as_deref(),
                &group_pairs,
                Some(&issuer_url),
                validity.1,
                None,
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

            // client_credentials is machine-to-machine: it only issues the
            // custom resource-server scopes the client is allowed. Standard
            // OIDC scopes (openid/email/...) and any scope not in the client's
            // AllowedOAuthScopes are rejected with invalid_scope, matching AWS.
            const STANDARD_SCOPES: &[&str] = &[
                "openid",
                "email",
                "phone",
                "profile",
                "aws.cognito.signin.user.admin",
            ];
            let is_custom_scope = |s: &str| s.contains('/') && !STANDARD_SCOPES.contains(&s);
            let client_allowed = pool
                .clients
                .get(&effective_client_id)
                .map(|c| c.allowed_oauth_scopes.clone())
                .unwrap_or_default();
            let requested_scopes = parse_scopes(form.scope.as_deref().unwrap_or(""));
            for s in &requested_scopes {
                if !is_custom_scope(s) || !client_allowed.contains(s) {
                    return error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_scope",
                        &format!("{s} is not a valid scope for the client_credentials grant"),
                    );
                }
            }
            // No explicit scope: default to all custom scopes the client allows.
            let effective_scopes: Vec<String> = if requested_scopes.is_empty() {
                client_allowed
                    .into_iter()
                    .filter(|s| is_custom_scope(s))
                    .collect()
            } else {
                requested_scopes
            };

            let client_access_validity = pool
                .clients
                .get(&effective_client_id)
                .map(|c| c.access_token_validity)
                .unwrap_or(3600);

            // client_credentials is machine-to-machine — no user groups.
            let issuer_url = oauth_state.issuer(&headers, &pool_id);
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
                None,
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

            // The token must resolve to a real user; a forged or unknown
            // token is rejected rather than minting tokens for "unknown".
            let user = match pool
                .users
                .values()
                .find(|u| u.sub == sub || refresh_tok.contains(&u.sub))
                .cloned()
            {
                Some(u) => u,
                None => {
                    return error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_grant",
                        "Invalid Refresh Token.",
                    );
                }
            };

            // Honour revocation and a global sign-out that predates the token.
            if user
                .revoked_refresh_tokens
                .iter()
                .any(|t| t == &refresh_tok)
                || user.signed_out_at.is_some_and(|t| {
                    jwt::refresh_token_issued_at(&refresh_tok).is_none_or(|issued| issued < t)
                })
            {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "Refresh Token has been revoked",
                );
            }

            let pairs = user_group_role_pairs(&pool, &user.groups);
            let (user_sub, username, attributes, group_pairs) = (
                user.sub.clone(),
                user.username.clone(),
                user.attributes.clone(),
                pairs,
            );

            let effective_client_id = if client_id.is_empty() {
                "unknown-client".to_string()
            } else {
                client_id.clone()
            };

            let scopes = parse_scopes(form.scope.as_deref().unwrap_or("openid"));
            let issuer_url = oauth_state.issuer(&headers, &pool_id);

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
                None,
            );
            let id_tok = jwt::id_token(
                &user_sub,
                &oauth_state.default_region,
                &pool_id,
                &effective_client_id,
                &username,
                &attributes,
                &crate::operations::users::client_read_set(&pool, &effective_client_id)
                    .unwrap_or_default(),
                &scopes,
                None,
                &group_pairs,
                Some(&issuer_url),
                validity.1,
                None,
            );
            // AWS Cognito intentionally does NOT issue a new refresh_token
            // on a refresh-grant exchange — the SPA keeps using the
            // original one. Mirroring that here avoids confusing SDKs that
            // store the response and either retain a stale value or drop
            // the original.
            info!(
                pool_id = %pool_id,
                username = %username,
                "OAuth: refresh_token exchange successful"
            );

            Json(json!({
                "access_token": access_tok,
                "id_token": id_tok,
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
                format!("User pool {pool_id} does not exist."),
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
                    return (StatusCode::UNAUTHORIZED, "User does not exist.").into_response();
                }
            }
        }
    };

    // userInfo returns only the claims the access token's scopes grant. The
    // admin scope (aws.cognito.signin.user.admin) returns every attribute;
    // otherwise email/phone/profile each gate their own claim group.
    let scopes = token_scopes(&token);
    let has = |s: &str| scopes.iter().any(|x| x == s);

    let mut claims = json!({
        "sub": user.sub,
        "username": user.username,
    });
    if let Some(obj) = claims.as_object_mut() {
        if has("aws.cognito.signin.user.admin") {
            for (k, v) in &user.attributes {
                obj.insert(k.clone(), Value::String(v.clone()));
            }
        } else {
            let mut allowed: Vec<&str> = Vec::new();
            if has("email") {
                allowed.extend(["email", "email_verified"]);
            }
            if has("phone") {
                allowed.extend(["phone_number", "phone_number_verified"]);
            }
            if has("profile") {
                allowed.extend([
                    "name",
                    "given_name",
                    "family_name",
                    "middle_name",
                    "nickname",
                    "preferred_username",
                    "picture",
                    "profile",
                    "website",
                    "gender",
                    "birthdate",
                    "zoneinfo",
                    "locale",
                    "updated_at",
                    "address",
                ]);
            }
            for attr in allowed {
                if let Some(v) = user.attributes.get(attr) {
                    obj.insert(attr.to_string(), Value::String(v.clone()));
                }
            }
        }
    }

    Json(claims).into_response()
}

/// Read the space-separated `scope` claim out of an access token's payload
/// (best-effort: an unparsable token yields no scopes).
fn token_scopes(token: &str) -> Vec<String> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let Some(payload_seg) = token.split('.').nth(1) else {
        return Vec::new();
    };
    let Ok(bytes) = URL_SAFE_NO_PAD.decode(payload_seg) else {
        return Vec::new();
    };
    let Ok(payload) = serde_json::from_slice::<Value>(&bytes) else {
        return Vec::new();
    };
    payload["scope"]
        .as_str()
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
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
    headers: HeaderMap,
    Query(params): Query<LogoutParams>,
) -> Response {
    let pool_ref = match oauth_state.cognito.user_pools.get(&pool_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                format!("User pool {pool_id} does not exist."),
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
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "logout_uri does not match any registered LogoutURL.",
            );
        }
        info!(pool_id = %pool_id, client_id = %params.client_id, "OAuth: logout → logout_uri");
        return Redirect::to(logout_uri).into_response();
    }

    if let Some(redirect_uri) = params.redirect_uri.as_deref().filter(|s| !s.is_empty()) {
        if !client.callback_urls.is_empty()
            && !client.callback_urls.contains(&redirect_uri.to_string())
        {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "redirect_uri does not match any registered callback URL.",
            );
        }
        let response_type = params.response_type.as_deref().unwrap_or("code");
        let scope = params.scope.as_deref().unwrap_or("openid");
        let state_param = params.state.as_deref().unwrap_or("");
        let mut url = format!(
            "{}/cognito/{pool_id}/oauth2/authorize?client_id={}&response_type={}&redirect_uri={}&scope={}",
            oauth_state.base_url(&headers),
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
// 7. Forgot-password endpoints (hosted UI)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct ForgotPasswordParams {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    scope: Option<String>,
    state: Option<String>,
    nonce: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
}

#[derive(Deserialize, Default)]
struct ForgotPasswordForm {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    scope: Option<String>,
    state: Option<String>,
    nonce: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    username: Option<String>,
}

#[derive(Deserialize, Default)]
struct ForgotPasswordConfirmForm {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    scope: Option<String>,
    state: Option<String>,
    nonce: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    username: Option<String>,
    code: Option<String>,
    new_password: Option<String>,
    confirm_password: Option<String>,
}

async fn forgot_password_get(
    State(_oauth_state): State<Arc<CognitoOAuthState>>,
    Path(pool_id): Path<String>,
    Query(params): Query<ForgotPasswordParams>,
) -> Response {
    forgot_password_page_html(
        &pool_id,
        params.response_type.as_deref().unwrap_or("code"),
        params.client_id.as_deref().unwrap_or(""),
        params.redirect_uri.as_deref().unwrap_or(""),
        params.scope.as_deref().unwrap_or("openid"),
        params.state.as_deref().unwrap_or(""),
        params.nonce.as_deref().unwrap_or(""),
        params.code_challenge.as_deref().unwrap_or(""),
        params.code_challenge_method.as_deref().unwrap_or(""),
        None,
        None,
        None,
    )
}

async fn forgot_password_post(
    State(oauth_state): State<Arc<CognitoOAuthState>>,
    Path(pool_id): Path<String>,
    Form(form): Form<ForgotPasswordForm>,
) -> Response {
    let response_type = form.response_type.as_deref().unwrap_or("code");
    let client_id = form.client_id.as_deref().unwrap_or("");
    let redirect_uri = form.redirect_uri.as_deref().unwrap_or("");
    let scope = form.scope.as_deref().unwrap_or("openid");
    let state_param = form.state.as_deref().unwrap_or("");
    let nonce = form.nonce.as_deref().unwrap_or("");
    let code_challenge = form.code_challenge.as_deref().unwrap_or("");
    let code_challenge_method = form.code_challenge_method.as_deref().unwrap_or("");

    let username = match form.username.as_deref().filter(|u| !u.is_empty()) {
        Some(u) => u,
        None => {
            return forgot_password_page_html(
                &pool_id,
                response_type,
                client_id,
                redirect_uri,
                scope,
                state_param,
                nonce,
                code_challenge,
                code_challenge_method,
                Some("Username is required"),
                None,
                None,
            );
        }
    };

    // Reuse the SDK-side ForgotPassword handler so the code-issuance
    // path is exactly the same. We synthesize a minimal request value
    // because we already have the pool_id route param.
    let req_input = json!({ "ClientId": client_id, "Username": username });
    let ctx = awsim_core::RequestContext::new_with_account(
        "cognito-idp",
        &oauth_state.default_region,
        &oauth_state.default_account_id,
    );
    if let Err(e) =
        crate::operations::users::forgot_password(&oauth_state.cognito, &req_input, &ctx)
    {
        // For a non-existent user we still surface a generic notice +
        // route to the confirm page — real Cognito is intentionally
        // vague about whether an account exists. But we'll only show
        // the dev-mode hint when the user actually exists, so a wrong
        // username won't get a code.
        warn!(
            username = %username,
            pool_id = %pool_id,
            error = %e,
            "OAuth forgot-password: SDK handler rejected"
        );
        return forgot_password_page_html(
            &pool_id,
            response_type,
            client_id,
            redirect_uri,
            scope,
            state_param,
            nonce,
            code_challenge,
            code_challenge_method,
            Some(&e.message),
            None,
            Some(username),
        );
    }

    // Pull the freshly stored code so we can show it to the dev
    // directly. In real AWS this code would arrive by email; awsim
    // also logs it at info.
    let code_hint = oauth_state
        .cognito
        .user_pools
        .get(&pool_id)
        .and_then(|p| p.users.get(username).cloned())
        .and_then(|u| {
            u.pending_verifications
                .get(crate::operations::users::FORGOT_PASSWORD_KEY)
                .cloned()
        });

    forgot_password_confirm_page_html(
        &pool_id,
        response_type,
        client_id,
        redirect_uri,
        scope,
        state_param,
        nonce,
        code_challenge,
        code_challenge_method,
        username,
        code_hint.as_deref(),
        None,
    )
}

async fn forgot_password_confirm_get(
    State(_oauth_state): State<Arc<CognitoOAuthState>>,
    Path(pool_id): Path<String>,
    Query(params): Query<ForgotPasswordParams>,
) -> Response {
    // Direct GET: we don't have a username yet, so route back to the
    // request page rather than rendering an empty confirm form.
    forgot_password_page_html(
        &pool_id,
        params.response_type.as_deref().unwrap_or("code"),
        params.client_id.as_deref().unwrap_or(""),
        params.redirect_uri.as_deref().unwrap_or(""),
        params.scope.as_deref().unwrap_or("openid"),
        params.state.as_deref().unwrap_or(""),
        params.nonce.as_deref().unwrap_or(""),
        params.code_challenge.as_deref().unwrap_or(""),
        params.code_challenge_method.as_deref().unwrap_or(""),
        None,
        Some("Start a fresh reset by entering your username."),
        None,
    )
}

async fn forgot_password_confirm_post(
    State(oauth_state): State<Arc<CognitoOAuthState>>,
    Path(pool_id): Path<String>,
    Form(form): Form<ForgotPasswordConfirmForm>,
) -> Response {
    let response_type = form.response_type.as_deref().unwrap_or("code");
    let client_id = form.client_id.as_deref().unwrap_or("");
    let redirect_uri = form.redirect_uri.as_deref().unwrap_or("");
    let scope = form.scope.as_deref().unwrap_or("openid");
    let state_param = form.state.as_deref().unwrap_or("");
    let nonce = form.nonce.as_deref().unwrap_or("");
    let code_challenge = form.code_challenge.as_deref().unwrap_or("");
    let code_challenge_method = form.code_challenge_method.as_deref().unwrap_or("");

    let username = form.username.as_deref().unwrap_or("");
    let code = form.code.as_deref().unwrap_or("");
    let new_password = form.new_password.as_deref().unwrap_or("");
    let confirm_password = form.confirm_password.as_deref().unwrap_or("");

    let render_error = |msg: &str| {
        forgot_password_confirm_page_html(
            &pool_id,
            response_type,
            client_id,
            redirect_uri,
            scope,
            state_param,
            nonce,
            code_challenge,
            code_challenge_method,
            username,
            None,
            Some(msg),
        )
    };

    if username.is_empty() || code.is_empty() || new_password.is_empty() {
        return render_error("All fields are required");
    }
    if new_password != confirm_password {
        return render_error("Passwords do not match");
    }

    let req_input = json!({
        "ClientId": client_id,
        "Username": username,
        "ConfirmationCode": code,
        "Password": new_password,
    });
    let ctx = awsim_core::RequestContext::new_with_account(
        "cognito-idp",
        &oauth_state.default_region,
        &oauth_state.default_account_id,
    );
    if let Err(e) =
        crate::operations::users::confirm_forgot_password(&oauth_state.cognito, &req_input, &ctx)
    {
        return render_error(&e.message);
    }

    info!(
        pool_id = %pool_id,
        username = %username,
        "OAuth: forgot-password completed via hosted UI"
    );

    // Send the user back to /authorize with a hint that they should
    // sign in with their new password.
    let oauth_query = build_oauth_query(&[
        ("response_type", response_type),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("scope", scope),
        ("state", state_param),
        ("nonce", nonce),
        ("code_challenge", code_challenge),
        ("code_challenge_method", code_challenge_method),
    ]);
    let url = if oauth_query.is_empty() {
        format!("/cognito/{pool_id}/oauth2/authorize")
    } else {
        format!("/cognito/{pool_id}/oauth2/authorize?{oauth_query}")
    };
    Redirect::to(&url).into_response()
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    auth.strip_prefix("Bearer ").map(|t| t.trim().to_string())
}

#[cfg(test)]
mod saml_tests {
    use super::*;
    use crate::operations::{identity_providers, pools};
    use awsim_core::RequestContext;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use dashmap::DashMap;
    use serde_json::json;

    fn ctx() -> RequestContext {
        RequestContext::new("cognito-idp", "us-east-1")
    }

    fn setup() -> (Arc<CognitoOAuthState>, String, String) {
        let cognito = Arc::new(CognitoState::default());
        let pool = pools::create_user_pool(&cognito, &json!({ "PoolName": "p" }), &ctx()).unwrap();
        let pool_id = pool["UserPool"]["Id"].as_str().unwrap().to_string();
        let client = pools::create_user_pool_client(
            &cognito,
            &json!({ "UserPoolId": pool_id, "ClientName": "c",
                     "CallbackURLs": ["https://app.test/cb"] }),
            &ctx(),
        )
        .unwrap();
        let client_id = client["UserPoolClient"]["ClientId"]
            .as_str()
            .unwrap()
            .to_string();
        identity_providers::create_identity_provider(
            &cognito,
            &json!({
                "UserPoolId": pool_id,
                "ProviderName": "CorpSAML",
                "ProviderType": "SAML",
                "ProviderDetails": { "SSORedirectBindingURI": "https://idp.example/sso" },
                "AttributeMapping": { "email": "http://schemas.xmlsoap.org/claims/EmailAddress" }
            }),
            &ctx(),
        )
        .unwrap();
        let state = Arc::new(CognitoOAuthState {
            cognito,
            default_region: "us-east-1".to_string(),
            default_account_id: "000000000000".to_string(),
            auth_codes: Arc::new(DashMap::new()),
            revoked_refresh_tokens: Arc::new(DashMap::new()),
            federation: crate::federation::FederationState::new(),
            port: 4566,
        });
        (state, pool_id, client_id)
    }

    fn saml_response_b64(name_id: &str, email: &str) -> String {
        let xml = format!(
            r#"<samlp:Response xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol"
                 xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion">
              <saml:Assertion><saml:Subject>
                <saml:NameID>{name_id}</saml:NameID>
              </saml:Subject><saml:AttributeStatement>
                <saml:Attribute Name="http://schemas.xmlsoap.org/claims/EmailAddress">
                  <saml:AttributeValue>{email}</saml:AttributeValue>
                </saml:Attribute>
              </saml:AttributeStatement></saml:Assertion>
            </samlp:Response>"#
        );
        STANDARD.encode(xml)
    }

    #[test]
    fn saml_acs_federates_user_and_issues_code() {
        let (state, pool_id, client_id) = setup();
        let relay = crate::federation::stash(
            &state.federation,
            crate::federation::PendingFederation {
                pool_id: pool_id.clone(),
                client_id: client_id.clone(),
                redirect_uri: "https://app.test/cb".to_string(),
                scope: "openid email".to_string(),
                app_state: "xyz".to_string(),
                nonce: None,
                code_challenge: None,
                code_challenge_method: None,
                provider_name: "CorpSAML".to_string(),
                issued_at: now_epoch(),
            },
        );

        let form = SamlAcsForm {
            saml_response: Some(saml_response_b64("corp-user-1", "alice@corp.example")),
            relay_state: Some(relay),
        };
        let resp = saml_acs_inner(&state, pool_id.clone(), form);

        assert!(resp.status().is_redirection(), "ACS redirects back to app");
        let location = resp
            .headers()
            .get(axum::http::header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(location.starts_with("https://app.test/cb?code="));
        assert!(location.contains("&state=xyz"));

        // The federated user was created with the mapped email.
        let pool = state.cognito.user_pools.get(&pool_id).unwrap();
        let user = pool
            .users
            .get("CorpSAML_corp-user-1")
            .expect("federated user");
        assert_eq!(
            user.attributes.get("email").map(String::as_str),
            Some("alice@corp.example")
        );
        assert_eq!(user.status, "EXTERNAL_PROVIDER");
    }

    #[test]
    fn saml_acs_rejects_unknown_relay_state() {
        let (state, pool_id, _client_id) = setup();
        let form = SamlAcsForm {
            saml_response: Some(saml_response_b64("u", "u@e.com")),
            relay_state: Some("nonexistent".to_string()),
        };
        let resp = saml_acs_inner(&state, pool_id, form);
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
