//! Built-in OIDC identity provider for offline federation testing.
//!
//! Tier 1 of the federation roadmap (see `docs/guide/cognito-federation.md`):
//! awsim hosts a fully self-contained, RFC 6749 / OIDC Core 1.0
//! authorization server at `/_awsim/idp/{provider_id}/...` so that a
//! Cognito user pool's `IdentityProvider` of type `OIDC` can point at
//! it and the entire federation flow (authorize -> code -> token
//! exchange -> userinfo -> attribute mapping -> linked user) works
//! end-to-end without any external network calls.
//!
//! Endpoints exposed under `/_awsim/idp/{provider_id}`:
//!   - `.well-known/openid-configuration` — discovery
//!   - `.well-known/jwks.json` — public key for ID-token verification
//!   - `authorize` — GET shows a free-form claim entry form, POST mints
//!     an authorization code
//!   - `token` — POST exchanges code for ID + access tokens
//!   - `userinfo` — GET returns claims for an access token
//!
//! And under `/_awsim/idp` (admin / control plane):
//!   - `POST` — register a new mock provider, returns `{provider_id,
//!     client_id, client_secret, discovery_url}`
//!   - `GET` — list registered providers
//!   - `DELETE /{provider_id}` — remove
//!
//! All mock providers sign with a single process-wide RSA keypair
//! distinct from the Cognito pool key, so the IdP looks like a real
//! external trust root from the Cognito side.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Json, Redirect, Response};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use dashmap::DashMap;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header};
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::traits::PublicKeyParts;
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{info, warn};

const KID: &str = "awsim-idp-key-1";

/// Lifetime of an authorization code in seconds. Real OIDC providers
/// typically use 60 s; we match that so tests for "expired code"
/// behaviour against awsim line up with prod.
const CODE_TTL_SECS: u64 = 60;
/// Lifetime of issued ID and access tokens.
const TOKEN_TTL_SECS: u64 = 3600;

/// Per-provider config: client credentials + a free-text "default
/// claims" template the authorize form is pre-populated with so that
/// quick smoke tests don't have to retype them every redirect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockIdpProvider {
    pub provider_id: String,
    pub client_id: String,
    pub client_secret: String,
    /// Pre-baked claims pre-filled in the authorize form (sub, email,
    /// name, groups, etc.). Stored as a JSON object so it round-trips
    /// through the discovery + admin APIs.
    pub default_claims: Value,
}

#[derive(Debug, Clone)]
struct AuthCode {
    /// The provider that minted it.
    provider_id: String,
    /// The full claim set the user submitted at the authorize form.
    claims: Value,
    /// `redirect_uri` we're committed to send the code back to. Real
    /// OAuth servers cross-check this on /token to defeat redirect
    /// substitution.
    redirect_uri: String,
    /// PKCE challenge from the original authorize request.
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    /// Scope set agreed at authorize time (echoed back in the token
    /// response).
    scope: String,
    /// `nonce` parameter for replay protection in the ID token.
    nonce: Option<String>,
    issued_at: u64,
}

#[derive(Debug, Clone)]
struct AccessTokenEntry {
    claims: Value,
    issued_at: u64,
}

#[derive(Default)]
pub struct MockIdpState {
    pub providers: DashMap<String, MockIdpProvider>,
    codes: DashMap<String, AuthCode>,
    access_tokens: DashMap<String, AccessTokenEntry>,
}

impl MockIdpState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

// -----------------------------------------------------------------
// Signing key (process-wide, separate from the Cognito pool key).
// -----------------------------------------------------------------

struct SigningMaterial {
    encoding: EncodingKey,
    #[allow(dead_code)]
    decoding: DecodingKey,
    n_b64url: String,
    e_b64url: String,
}

static MATERIAL: OnceLock<SigningMaterial> = OnceLock::new();

fn material() -> &'static SigningMaterial {
    MATERIAL.get_or_init(|| {
        let mut rng = rand::thread_rng();
        let private = RsaPrivateKey::new(&mut rng, 2048)
            .expect("RSA key generation should succeed with a working RNG");
        let public = RsaPublicKey::from(&private);
        let der = private
            .to_pkcs1_der()
            .expect("freshly generated RSA key encodes to PKCS#1");
        let encoding = EncodingKey::from_rsa_der(der.as_bytes());
        let n_b64url = URL_SAFE_NO_PAD.encode(public.n().to_bytes_be());
        let e_b64url = URL_SAFE_NO_PAD.encode(public.e().to_bytes_be());
        let decoding = DecodingKey::from_rsa_components(&n_b64url, &e_b64url)
            .expect("base64url-encoded RSA components are well-formed");
        SigningMaterial {
            encoding,
            decoding,
            n_b64url,
            e_b64url,
        }
    })
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn random_id() -> String {
    uuid::Uuid::new_v4().to_string().replace('-', "")
}

fn base_url(headers: &HeaderMap) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost:4566");
    format!("{scheme}://{host}")
}

fn issuer(headers: &HeaderMap, provider_id: &str) -> String {
    format!("{}/_awsim/idp/{}", base_url(headers), provider_id)
}

// -----------------------------------------------------------------
// Router
// -----------------------------------------------------------------

pub fn router(state: Arc<MockIdpState>) -> axum::Router {
    axum::Router::new()
        .route(
            "/_awsim/idp",
            axum::routing::get(list_providers).post(register_provider),
        )
        .route(
            "/_awsim/idp/{provider_id}",
            axum::routing::delete(delete_provider),
        )
        .route(
            "/_awsim/idp/{provider_id}/.well-known/openid-configuration",
            axum::routing::get(openid_config),
        )
        .route(
            "/_awsim/idp/{provider_id}/.well-known/jwks.json",
            axum::routing::get(jwks),
        )
        .route(
            "/_awsim/idp/{provider_id}/authorize",
            axum::routing::get(authorize_get).post(authorize_post),
        )
        .route(
            "/_awsim/idp/{provider_id}/token",
            axum::routing::post(token),
        )
        .route(
            "/_awsim/idp/{provider_id}/userinfo",
            axum::routing::get(userinfo),
        )
        .with_state(state)
}

// -----------------------------------------------------------------
// Admin: register / list / delete providers
// -----------------------------------------------------------------

#[derive(Deserialize)]
struct RegisterInput {
    provider_id: Option<String>,
    default_claims: Option<Value>,
}

async fn register_provider(
    State(state): State<Arc<MockIdpState>>,
    headers: HeaderMap,
    Json(input): Json<RegisterInput>,
) -> Response {
    let provider_id = input.provider_id.unwrap_or_else(random_id);
    if state.providers.contains_key(&provider_id) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error": "provider_id already exists"})),
        )
            .into_response();
    }

    // Sensible defaults so a freshly-registered provider can be used
    // immediately without further config.
    let default_claims = input.default_claims.unwrap_or_else(|| {
        json!({
            "sub": "mock-user-001",
            "email": "user@example.com",
            "email_verified": true,
            "name": "Mock User",
            "given_name": "Mock",
            "family_name": "User"
        })
    });

    let provider = MockIdpProvider {
        provider_id: provider_id.clone(),
        client_id: format!("awsim-idp-{provider_id}"),
        client_secret: random_id(),
        default_claims,
    };
    state
        .providers
        .insert(provider_id.clone(), provider.clone());

    info!(provider_id = %provider_id, "Mock IdP: registered provider");

    Json(json!({
        "provider_id": provider.provider_id,
        "client_id": provider.client_id,
        "client_secret": provider.client_secret,
        "discovery_url": format!("{}/.well-known/openid-configuration", issuer(&headers, &provider_id)),
        "authorize_url": format!("{}/authorize", issuer(&headers, &provider_id)),
        "token_url": format!("{}/token", issuer(&headers, &provider_id)),
        "userinfo_url": format!("{}/userinfo", issuer(&headers, &provider_id)),
        "jwks_url": format!("{}/.well-known/jwks.json", issuer(&headers, &provider_id)),
        "default_claims": provider.default_claims,
    }))
    .into_response()
}

async fn list_providers(State(state): State<Arc<MockIdpState>>) -> Json<Value> {
    let providers: Vec<Value> = state
        .providers
        .iter()
        .map(|kv| {
            let p = kv.value();
            json!({
                "provider_id": p.provider_id,
                "client_id": p.client_id,
                "default_claims": p.default_claims,
            })
        })
        .collect();
    Json(json!({"providers": providers}))
}

async fn delete_provider(
    State(state): State<Arc<MockIdpState>>,
    Path(provider_id): Path<String>,
) -> Response {
    match state.providers.remove(&provider_id) {
        Some(_) => StatusCode::NO_CONTENT.into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "provider not found"})),
        )
            .into_response(),
    }
}

// -----------------------------------------------------------------
// Discovery / JWKS
// -----------------------------------------------------------------

async fn openid_config(
    State(state): State<Arc<MockIdpState>>,
    Path(provider_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !state.providers.contains_key(&provider_id) {
        return provider_not_found();
    }
    let iss = issuer(&headers, &provider_id);
    Json(json!({
        "issuer": iss,
        "authorization_endpoint": format!("{iss}/authorize"),
        "token_endpoint": format!("{iss}/token"),
        "userinfo_endpoint": format!("{iss}/userinfo"),
        "jwks_uri": format!("{iss}/.well-known/jwks.json"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
        "scopes_supported": ["openid", "email", "profile", "groups"],
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"],
        "claims_supported": [
            "sub", "email", "email_verified", "name", "given_name",
            "family_name", "preferred_username", "groups"
        ],
        "code_challenge_methods_supported": ["S256", "plain"]
    }))
    .into_response()
}

async fn jwks(State(state): State<Arc<MockIdpState>>, Path(provider_id): Path<String>) -> Response {
    if !state.providers.contains_key(&provider_id) {
        return provider_not_found();
    }
    let m = material();
    Json(json!({
        "keys": [{
            "kty": "RSA",
            "alg": "RS256",
            "use": "sig",
            "kid": KID,
            "n": m.n_b64url,
            "e": m.e_b64url
        }]
    }))
    .into_response()
}

// -----------------------------------------------------------------
// Authorize: GET (form) / POST (mint code)
// -----------------------------------------------------------------

#[derive(Deserialize)]
struct AuthorizeParams {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    scope: Option<String>,
    state: Option<String>,
    nonce: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
}

async fn authorize_get(
    State(state): State<Arc<MockIdpState>>,
    Path(provider_id): Path<String>,
    Query(params): Query<AuthorizeParams>,
) -> Response {
    let provider = match state.providers.get(&provider_id) {
        Some(p) => p.clone(),
        None => return provider_not_found(),
    };

    if params.response_type != "code" {
        return (
            StatusCode::BAD_REQUEST,
            "Only response_type=code is supported by the mock IdP",
        )
            .into_response();
    }
    if params.client_id != provider.client_id {
        return (StatusCode::BAD_REQUEST, "client_id mismatch").into_response();
    }

    let claims_pretty =
        serde_json::to_string_pretty(&provider.default_claims).unwrap_or_else(|_| "{}".into());
    let html = render_login_form(
        &provider_id,
        &params.client_id,
        &params.redirect_uri,
        params.scope.as_deref().unwrap_or(""),
        params.state.as_deref().unwrap_or(""),
        params.nonce.as_deref().unwrap_or(""),
        params.code_challenge.as_deref().unwrap_or(""),
        params.code_challenge_method.as_deref().unwrap_or(""),
        &claims_pretty,
        None,
    );
    Html(html).into_response()
}

#[derive(Deserialize)]
struct AuthorizeForm {
    client_id: String,
    redirect_uri: String,
    scope: Option<String>,
    state: Option<String>,
    nonce: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    /// Free-text JSON the user edited in the login form. Parsed
    /// server-side so we can render a usable error message instead
    /// of bouncing back a 400.
    claims_json: String,
}

async fn authorize_post(
    State(state): State<Arc<MockIdpState>>,
    Path(provider_id): Path<String>,
    Form(form): Form<AuthorizeForm>,
) -> Response {
    let provider = match state.providers.get(&provider_id) {
        Some(p) => p.clone(),
        None => return provider_not_found(),
    };

    if form.client_id != provider.client_id {
        return (StatusCode::BAD_REQUEST, "client_id mismatch").into_response();
    }

    let claims: Value = match serde_json::from_str(&form.claims_json) {
        Ok(v @ Value::Object(_)) => v,
        Ok(_) => {
            return reshow_form(&provider, &form, "claims must be a JSON object");
        }
        Err(e) => {
            return reshow_form(&provider, &form, &format!("invalid JSON: {e}"));
        }
    };
    if claims.get("sub").and_then(|s| s.as_str()).is_none() {
        return reshow_form(
            &provider,
            &form,
            "claims object must include a `sub` string",
        );
    }

    let code = random_id();
    state.codes.insert(
        code.clone(),
        AuthCode {
            provider_id: provider_id.clone(),
            claims,
            redirect_uri: form.redirect_uri.clone(),
            code_challenge: form.code_challenge.clone().filter(|s| !s.is_empty()),
            code_challenge_method: form.code_challenge_method.clone().filter(|s| !s.is_empty()),
            scope: form.scope.clone().unwrap_or_default(),
            nonce: form.nonce.clone().filter(|s| !s.is_empty()),
            issued_at: now_epoch(),
        },
    );

    info!(provider_id = %provider_id, "Mock IdP: minted authorization code");

    let mut url = form.redirect_uri.clone();
    url.push(if url.contains('?') { '&' } else { '?' });
    url.push_str(&format!("code={}", urlencoding(&code)));
    if let Some(s) = form.state.as_ref().filter(|s| !s.is_empty()) {
        url.push_str(&format!("&state={}", urlencoding(s)));
    }
    Redirect::to(&url).into_response()
}

fn reshow_form(provider: &MockIdpProvider, form: &AuthorizeForm, err: &str) -> Response {
    Html(render_login_form(
        &provider.provider_id,
        &form.client_id,
        &form.redirect_uri,
        form.scope.as_deref().unwrap_or(""),
        form.state.as_deref().unwrap_or(""),
        form.nonce.as_deref().unwrap_or(""),
        form.code_challenge.as_deref().unwrap_or(""),
        form.code_challenge_method.as_deref().unwrap_or(""),
        &form.claims_json,
        Some(err),
    ))
    .into_response()
}

// -----------------------------------------------------------------
// Token: exchange code for ID + access tokens
// -----------------------------------------------------------------

#[derive(Deserialize)]
struct TokenForm {
    grant_type: String,
    code: Option<String>,
    redirect_uri: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    code_verifier: Option<String>,
}

async fn token(
    State(state): State<Arc<MockIdpState>>,
    Path(provider_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<TokenForm>,
) -> Response {
    let provider = match state.providers.get(&provider_id) {
        Some(p) => p.clone(),
        None => return provider_not_found(),
    };

    if form.grant_type != "authorization_code" {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "unsupported_grant_type",
            "Only authorization_code is supported",
        );
    }

    // Client auth: Basic header beats body params (RFC 6749 §2.3.1).
    let (client_id, client_secret) = basic_auth_credentials(&headers).unwrap_or((
        form.client_id.clone().unwrap_or_default(),
        form.client_secret.clone().unwrap_or_default(),
    ));
    if client_id != provider.client_id || client_secret != provider.client_secret {
        return oauth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "client_id / client_secret mismatch",
        );
    }

    let code = match form.code {
        Some(c) if !c.is_empty() => c,
        _ => {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "code is required",
            );
        }
    };

    let entry = match state.codes.remove(&code) {
        Some((_, e)) => e,
        None => {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "invalid_grant",
                "code is invalid or already used",
            );
        }
    };

    if entry.provider_id != provider_id {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "code is for a different provider",
        );
    }
    if now_epoch() > entry.issued_at + CODE_TTL_SECS {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", "code expired");
    }
    if let Some(uri) = form.redirect_uri.as_ref()
        && uri != &entry.redirect_uri
    {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "redirect_uri does not match the original authorize call",
        );
    }
    if let Some(challenge) = entry.code_challenge.as_ref() {
        let verifier = form.code_verifier.unwrap_or_default();
        let method = entry.code_challenge_method.as_deref().unwrap_or("plain");
        if !verify_pkce(&verifier, challenge, method) {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "invalid_grant",
                "PKCE code_verifier does not match",
            );
        }
    }

    let now = now_epoch();
    let id_token = mint_id_token(
        &issuer(&headers, &provider_id),
        &provider.client_id,
        &entry.claims,
        entry.nonce.as_deref(),
        now,
    );
    let access_token = random_id();
    state.access_tokens.insert(
        access_token.clone(),
        AccessTokenEntry {
            claims: entry.claims.clone(),
            issued_at: now,
        },
    );

    Json(json!({
        "access_token": access_token,
        "id_token": id_token,
        "token_type": "Bearer",
        "expires_in": TOKEN_TTL_SECS,
        "scope": entry.scope,
    }))
    .into_response()
}

// -----------------------------------------------------------------
// Userinfo
// -----------------------------------------------------------------

async fn userinfo(
    State(state): State<Arc<MockIdpState>>,
    Path(_provider_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned);
    let token = match bearer {
        Some(t) if !t.is_empty() => t,
        _ => {
            return (StatusCode::UNAUTHORIZED, "Bearer token required").into_response();
        }
    };
    let entry = match state.access_tokens.get(&token) {
        Some(e) => e.clone(),
        None => {
            return (StatusCode::UNAUTHORIZED, "Invalid access token").into_response();
        }
    };
    if now_epoch() > entry.issued_at + TOKEN_TTL_SECS {
        return (StatusCode::UNAUTHORIZED, "Access token expired").into_response();
    }
    Json(entry.claims).into_response()
}

// -----------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------

fn provider_not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": "mock IdP provider not registered"})),
    )
        .into_response()
}

fn oauth_error(status: StatusCode, error: &str, description: &str) -> Response {
    (
        status,
        Json(json!({
            "error": error,
            "error_description": description,
        })),
    )
        .into_response()
}

fn basic_auth_credentials(headers: &HeaderMap) -> Option<(String, String)> {
    use base64::engine::general_purpose::STANDARD;
    let auth = headers.get("authorization")?.to_str().ok()?;
    let encoded = auth.strip_prefix("Basic ")?;
    let decoded = STANDARD.decode(encoded).ok()?;
    let s = String::from_utf8(decoded).ok()?;
    let mut parts = s.splitn(2, ':');
    let id = parts.next()?.to_string();
    let secret = parts.next().unwrap_or("").to_string();
    Some((id, secret))
}

fn verify_pkce(code_verifier: &str, challenge: &str, method: &str) -> bool {
    match method {
        "S256" => {
            use sha2::{Digest, Sha256};
            let hash = Sha256::digest(code_verifier.as_bytes());
            URL_SAFE_NO_PAD.encode(hash) == challenge
        }
        "plain" => code_verifier == challenge,
        _ => false,
    }
}

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
                        let hi = char::from_digit((b >> 4) as u32, 16)
                            .expect("0..=15 is valid base-16")
                            .to_ascii_uppercase();
                        let lo = char::from_digit((b & 0xf) as u32, 16)
                            .expect("0..=15 is valid base-16")
                            .to_ascii_uppercase();
                        vec!['%', hi, lo]
                    })
                    .collect()
            }
        })
        .collect()
}

#[derive(Serialize)]
struct IdTokenClaims<'a> {
    iss: &'a str,
    aud: &'a str,
    sub: &'a str,
    iat: u64,
    exp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    nonce: Option<&'a str>,
    #[serde(flatten)]
    extra: HashMap<String, Value>,
}

fn mint_id_token(
    issuer_url: &str,
    audience: &str,
    user_claims: &Value,
    nonce: Option<&str>,
    now: u64,
) -> String {
    let sub = user_claims
        .get("sub")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let mut extra: HashMap<String, Value> = HashMap::new();
    if let Some(obj) = user_claims.as_object() {
        for (k, v) in obj {
            if k == "sub" || k == "iss" || k == "aud" || k == "iat" || k == "exp" || k == "nonce" {
                continue;
            }
            extra.insert(k.clone(), v.clone());
        }
    }
    let claims = IdTokenClaims {
        iss: issuer_url,
        aud: audience,
        sub,
        iat: now,
        exp: now + TOKEN_TTL_SECS,
        nonce,
        extra,
    };
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KID.to_string());
    jsonwebtoken::encode(&header, &claims, &material().encoding).unwrap_or_else(|e| {
        warn!(error = %e, "mock-idp: failed to mint ID token");
        String::new()
    })
}

// -----------------------------------------------------------------
// Login form (free-form claim entry)
// -----------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn render_login_form(
    provider_id: &str,
    client_id: &str,
    redirect_uri: &str,
    scope: &str,
    state: &str,
    nonce: &str,
    code_challenge: &str,
    code_challenge_method: &str,
    claims_json: &str,
    error: Option<&str>,
) -> String {
    let err_html = error
        .map(|e| format!(r#"<p class="err">{}</p>"#, escape_html(e)))
        .unwrap_or_default();
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Mock IdP — sign in</title>
<style>
body {{ font-family: ui-sans-serif, system-ui; max-width: 600px; margin: 2em auto; padding: 1em; }}
h1 {{ font-size: 1.1em; }}
.banner {{ background: #fef3c7; border: 1px solid #fcd34d; padding: 0.6em 0.8em; border-radius: 6px; margin-bottom: 1em; font-size: 0.85em; }}
textarea {{ width: 100%; min-height: 220px; font-family: ui-monospace, monospace; font-size: 0.85em; padding: 0.6em; border: 1px solid #d1d5db; border-radius: 4px; }}
.err {{ color: #b91c1c; background: #fee2e2; padding: 0.5em 0.7em; border-radius: 4px; }}
button {{ margin-top: 0.8em; padding: 0.5em 1em; background: #2563eb; color: white; border: 0; border-radius: 4px; cursor: pointer; }}
.muted {{ color: #6b7280; font-size: 0.8em; }}
</style></head><body>
<div class="banner">awsim built-in mock OIDC IdP — provider <code>{provider}</code>. Edit the
JSON below to choose what claims this sign-in should produce, then submit.</div>
<h1>Sign in</h1>
{err}
<form method="post" action="">
  <input type="hidden" name="client_id" value="{client_id}">
  <input type="hidden" name="redirect_uri" value="{redirect_uri}">
  <input type="hidden" name="scope" value="{scope}">
  <input type="hidden" name="state" value="{state}">
  <input type="hidden" name="nonce" value="{nonce}">
  <input type="hidden" name="code_challenge" value="{code_challenge}">
  <input type="hidden" name="code_challenge_method" value="{code_challenge_method}">
  <label for="claims_json">Claims to issue (JSON object, must include <code>sub</code>):</label>
  <textarea name="claims_json" id="claims_json">{claims}</textarea>
  <p class="muted">Edit freely — anything you put here lands as claims on the
  ID token + userinfo response. The Cognito side maps these to
  user attributes via the <code>AttributeMapping</code> on the IdentityProvider.</p>
  <button type="submit">Sign in</button>
</form>
</body></html>"#,
        provider = escape_html(provider_id),
        err = err_html,
        client_id = escape_html(client_id),
        redirect_uri = escape_html(redirect_uri),
        scope = escape_html(scope),
        state = escape_html(state),
        nonce = escape_html(nonce),
        code_challenge = escape_html(code_challenge),
        code_challenge_method = escape_html(code_challenge_method),
        claims = escape_html(claims_json),
    )
}

fn escape_html(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect(),
            '>' => "&gt;".chars().collect(),
            '"' => "&quot;".chars().collect(),
            '\'' => "&#x27;".chars().collect(),
            other => vec![other],
        })
        .collect()
}
