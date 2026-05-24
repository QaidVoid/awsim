//! Operator authentication for the admin endpoints and UI.
//!
//! AWSim ships loginless by default so local-dev test suites keep
//! working with no setup. When `AWSIM_REQUIRE_OPERATOR_AUTH=true`,
//! requests to the admin endpoints (`/_awsim/{health, services,
//! config, stats, storage, events, requests, ...}`) and the admin
//! UI must carry a session token from a successful
//! `POST /_awsim/auth/login`.
//!
//! Login takes IAM user credentials: username + password (verified
//! against the bcrypt hash stored on the user's LoginProfile) plus
//! an optional 6-digit TOTP code (required when the user has an
//! enabled virtual MFA device). The response is an
//! `awsim_core::bearer_token` session that the client sends back as
//! `Authorization: Bearer <token>` or via the
//! `awsim_session` HTTP-only cookie.
//!
//! Sessions are stateless: the token is an HMAC envelope of
//! `{principal, expiry}`, so logout is just "stop sending it" and
//! sessions don't survive a process restart.

use awsim_core::bearer_token;
use awsim_iam::IamService;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long an operator login session lives (12 hours, matches
/// AWS IAM console default).
const SESSION_TTL: Duration = Duration::from_secs(12 * 60 * 60);

/// HTTP-only cookie name carrying the operator session.
pub const SESSION_COOKIE: &str = "awsim_session";

/// Cookie + header prefix on the principal string the bearer
/// envelope wraps, so other auth flows (future SCIM, CodeArtifact)
/// can share `bearer_token` without colliding on the principal
/// namespace.
const PRINCIPAL_PREFIX: &str = "operator:";

/// Failed-login attempts tracked per principal so a flood of
/// guesses against a single name can be throttled without locking
/// out a different operator from logging in concurrently.
type AttemptMap = HashMap<String, AttemptRecord>;

#[derive(Debug, Clone, Copy)]
struct AttemptRecord {
    count: u32,
    first_attempt: Instant,
}

/// How many bad logins per username before the throttle trips.
const MAX_FAILED_ATTEMPTS: u32 = 5;
/// Sliding window for counting failed attempts. The throttle lifts
/// when the window elapses since the first attempt in the burst.
const THROTTLE_WINDOW: Duration = Duration::from_secs(60);

/// Username reserved for the bootstrap operator account that
/// `setup` provisions. Re-exported from awsim-iam so the IAM
/// service's root-protection guard and the operator-auth setup
/// flow agree on a single source of truth.
pub use awsim_iam::ROOT_USERNAME;

/// Bootstrap-flow state. The `Pending` variant stores the SHA-256
/// hash of the one-time setup token; the raw token is only ever
/// printed to stdout on startup and never persisted.
#[derive(Debug, Clone)]
enum BootstrapState {
    NotRequired,
    Pending { token_hash: [u8; 32] },
    Complete,
}

/// Shared state injected into the auth routes and the middleware.
#[derive(Clone)]
pub struct OperatorAuthState {
    pub iam: Arc<IamService>,
    pub default_account_id: String,
    pub default_region: String,
    failed_attempts: Arc<Mutex<AttemptMap>>,
    bootstrap: Arc<Mutex<BootstrapState>>,
}

impl OperatorAuthState {
    pub fn new(iam: Arc<IamService>, default_account_id: String, default_region: String) -> Self {
        Self {
            iam,
            default_account_id,
            default_region,
            failed_attempts: Arc::new(Mutex::new(HashMap::new())),
            bootstrap: Arc::new(Mutex::new(BootstrapState::NotRequired)),
        }
    }

    /// Arm the bootstrap flow. Generates a fresh setup token,
    /// stores its hash, and returns the raw token so the caller
    /// (usually `main`) can print it to stdout. Subsequent setup
    /// calls compare against the stored hash.
    pub fn arm_bootstrap(&self) -> String {
        let mut buf = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut buf);
        let token = hex_encode(&buf);
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        let hash: [u8; 32] = hasher.finalize().into();
        *self.bootstrap.lock().unwrap() = BootstrapState::Pending { token_hash: hash };
        token
    }

    /// Mark bootstrap complete. Called once `setup` succeeds and
    /// also by `main` at startup when an existing root user is
    /// found in the snapshot.
    pub fn mark_bootstrap_complete(&self) {
        *self.bootstrap.lock().unwrap() = BootstrapState::Complete;
    }

    fn bootstrap_state(&self) -> BootstrapState {
        self.bootstrap.lock().unwrap().clone()
    }

    /// Returns the seconds the caller must wait before retrying, or
    /// `None` if they're still within the allowed budget.
    fn throttle_retry_after(&self, username: &str) -> Option<u64> {
        let mut g = self.failed_attempts.lock().unwrap();
        if let Some(rec) = g.get(username).copied() {
            let elapsed = rec.first_attempt.elapsed();
            if rec.count >= MAX_FAILED_ATTEMPTS && elapsed < THROTTLE_WINDOW {
                return Some((THROTTLE_WINDOW - elapsed).as_secs().max(1));
            }
            if elapsed >= THROTTLE_WINDOW {
                g.remove(username);
            }
        }
        None
    }

    fn record_failure(&self, username: &str) {
        let mut g = self.failed_attempts.lock().unwrap();
        let now = Instant::now();
        match g.get_mut(username) {
            Some(rec) if rec.first_attempt.elapsed() < THROTTLE_WINDOW => {
                rec.count = rec.count.saturating_add(1);
            }
            _ => {
                g.insert(
                    username.to_string(),
                    AttemptRecord {
                        count: 1,
                        first_attempt: now,
                    },
                );
            }
        }
    }

    fn clear_failures(&self, username: &str) {
        self.failed_attempts.lock().unwrap().remove(username);
    }
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub mfa_code: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub session_token: String,
    pub expires_in: u64,
    pub principal: String,
}

/// `POST /_awsim/auth/login`
///
/// Body: `{ "username": "...", "password": "...", "mfa_code"?: "123456" }`
///
/// Returns the session token in JSON plus a `Set-Cookie:
/// awsim_session=...` header so browsers don't need to handle the
/// body. 401 on bad credentials or missing/wrong MFA.
pub async fn login(
    State(state): State<OperatorAuthState>,
    Json(req): Json<LoginRequest>,
) -> Response {
    if matches!(state.bootstrap_state(), BootstrapState::Pending { .. }) {
        return setup_required_response();
    }

    if let Some(retry_after) = state.throttle_retry_after(&req.username) {
        let mut resp = (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "code": "ThrottlingException",
                "message": format!(
                    "Too many failed login attempts. Retry after {retry_after} seconds."
                ),
            })),
        )
            .into_response();
        if let Ok(v) = retry_after.to_string().parse() {
            resp.headers_mut().insert(header::RETRY_AFTER, v);
        }
        return resp;
    }

    // IAM is a global service: the store always shards by
    // `(account_id, IAM_REGION)` regardless of which AWS region the
    // gateway was configured with. Reading with `default_region`
    // instead lands on an empty IamState and every login fails with
    // "Invalid credentials".
    let iam_state = state
        .iam
        .store()
        .get(&state.default_account_id, awsim_iam::IAM_REGION);

    if let Err(e) = awsim_iam::verify_password(&iam_state, &req.username, &req.password) {
        state.record_failure(&req.username);
        return (StatusCode::UNAUTHORIZED, Json(error_body(&e))).into_response();
    }

    if let Err(e) = require_mfa_if_enabled(&iam_state, &req.username, req.mfa_code.as_deref()) {
        state.record_failure(&req.username);
        return (StatusCode::UNAUTHORIZED, Json(error_body(&e))).into_response();
    }

    state.clear_failures(&req.username);

    let principal = format!(
        "{PRINCIPAL_PREFIX}iam-user:{}/{}",
        state.default_account_id, req.username
    );
    let token = bearer_token::mint(&principal, SESSION_TTL);

    let cookie = format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
        SESSION_TTL.as_secs()
    );
    let mut response = Json(LoginResponse {
        session_token: token,
        expires_in: SESSION_TTL.as_secs(),
        principal,
    })
    .into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, cookie.parse().unwrap());
    response
}

#[derive(Debug, Deserialize)]
pub struct SetupRequest {
    pub bootstrap_token: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct SetupResponse {
    pub principal: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

/// `POST /_awsim/auth/setup`
///
/// First-run handshake. Accepts the bootstrap token AWSim printed
/// to stdout on first boot when `AWSIM_REQUIRE_OPERATOR_AUTH=true`
/// and no `root` user exists, plus a password the operator picks
/// for the root account. On success, creates the root IAM user, a
/// login profile with the password, an initial access-key pair
/// for programmatic use, and flips the gate so normal login
/// begins working.
///
/// Returns 410 Gone once setup has completed; the token is single
/// use.
pub async fn setup(
    State(state): State<OperatorAuthState>,
    Json(req): Json<SetupRequest>,
) -> Response {
    let bootstrap_state = state.bootstrap_state();
    let expected_hash = match bootstrap_state {
        BootstrapState::Pending { token_hash } => token_hash,
        BootstrapState::Complete | BootstrapState::NotRequired => {
            return (
                StatusCode::GONE,
                Json(json!({
                    "code": "SetupAlreadyComplete",
                    "message": "Operator setup has already run; use /_awsim/auth/login.",
                })),
            )
                .into_response();
        }
    };

    let mut hasher = Sha256::new();
    hasher.update(req.bootstrap_token.as_bytes());
    let supplied: [u8; 32] = hasher.finalize().into();
    if !constant_time_eq(&supplied, &expected_hash) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "code": "InvalidBootstrapToken",
                "message": "The supplied bootstrap token does not match the printed value.",
            })),
        )
            .into_response();
    }

    use awsim_core::ServiceHandler;
    // Bootstrap flow needs to provision the root IAM record, which
    // the IAM service blocks on every external call. `internal`
    // builds a context that bypasses the root-protection precondition
    // so CreateUser("root") + CreateLoginProfile + CreateAccessKey
    // succeed exactly once here at first-run setup.
    let ctx = awsim_core::RequestContext::internal(
        "iam",
        &state.default_region,
        &state.default_account_id,
    );

    let create_user = serde_json::json!({ "UserName": ROOT_USERNAME });
    if let Err(e) = state.iam.handle("CreateUser", create_user, &ctx).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_body(&e))).into_response();
    }

    let create_login = serde_json::json!({
        "UserName": ROOT_USERNAME,
        "Password": req.password,
        "PasswordResetRequired": false,
    });
    if let Err(e) = state
        .iam
        .handle("CreateLoginProfile", create_login, &ctx)
        .await
    {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_body(&e))).into_response();
    }

    let key_input = serde_json::json!({ "UserName": ROOT_USERNAME });
    let key_resp = match state.iam.handle("CreateAccessKey", key_input, &ctx).await {
        Ok(v) => v,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_body(&e))).into_response(),
    };
    let access_key_id = key_resp["AccessKey"]["AccessKeyId"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let secret_access_key = key_resp["AccessKey"]["SecretAccessKey"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    state.mark_bootstrap_complete();

    Json(SetupResponse {
        principal: format!("iam-user:{}/{}", state.default_account_id, ROOT_USERNAME),
        access_key_id,
        secret_access_key,
    })
    .into_response()
}

fn setup_required_response() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "code": "OperatorSetupRequired",
            "message": "Run POST /_awsim/auth/setup with the bootstrap token printed to stdout before signing in.",
        })),
    )
        .into_response()
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(*b >> 4) as usize] as char);
        out.push(HEX[(*b & 0x0f) as usize] as char);
    }
    out
}

fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

#[derive(Debug, Serialize)]
pub struct OperatorCredentialsResponse {
    pub access_key_id: String,
    pub secret_access_key: String,
    /// IAM access keys never expire on their own. We surface
    /// `expires_at` so the UI can refresh in step with the operator
    /// session, but the underlying credentials remain valid until the
    /// operator explicitly rotates them. The value here mirrors the
    /// session TTL so client-side caching tracks the wall-clock
    /// lifetime of the sign-in.
    pub expires_at: String,
    pub principal: String,
}

/// `GET /_awsim/auth/credentials`
///
/// Returns the IAM access-key + secret pair the UI should use to
/// sign AWS requests on behalf of the currently signed-in operator.
/// Behind the operator-auth middleware so only the holder of a
/// valid session cookie can fetch the secret.
///
/// AWS-parity note: real AWS console federates through STS and
/// hands the browser short-lived ASIA credentials; AWSim returns
/// the operator's own long-lived IAM keys directly. This is a
/// documented simplification for the local-dev case where
/// short-lived credentials add deployment complexity (token
/// rotation, refresh loop, expiry handling) without buying real
/// security since the snapshot file already contains every secret.
/// A future phase can swap this for `sts:GetSessionToken` once the
/// session store gains support for user-issued sessions.
pub async fn credentials(State(state): State<OperatorAuthState>, headers: HeaderMap) -> Response {
    let principal = match resolve_session(&headers) {
        Some(p) => p,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "code": "UnauthorizedException",
                    "message": "No operator session.",
                })),
            )
                .into_response();
        }
    };
    let user_name = match principal.rsplit_once('/') {
        Some((_, name)) => name.to_string(),
        None => principal.clone(),
    };
    let iam_state = state
        .iam
        .store()
        .get(&state.default_account_id, awsim_iam::IAM_REGION);
    let user = match iam_state.users.get(&user_name) {
        Some(u) => u,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "code": "NoSuchEntity",
                    "message": format!("IAM user {user_name} no longer exists."),
                })),
            )
                .into_response();
        }
    };
    let active_key = user
        .access_keys
        .iter()
        .find(|k| k.status == "Active")
        .cloned();
    drop(user);
    let key = match active_key {
        Some(k) => k,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "code": "NoAccessKey",
                    "message": format!(
                        "IAM user {user_name} has no active access key. \
                         Create one before signing requests as this principal.",
                    ),
                })),
            )
                .into_response();
        }
    };
    let expires_at = (chrono::Utc::now()
        + chrono::Duration::from_std(SESSION_TTL).unwrap_or(chrono::Duration::zero()))
    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    Json(OperatorCredentialsResponse {
        access_key_id: key.access_key_id,
        secret_access_key: key.secret_access_key,
        expires_at,
        principal,
    })
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct RevealSecretRequest {
    pub user_name: String,
    pub access_key_id: String,
}

#[derive(Debug, Serialize)]
pub struct RevealSecretResponse {
    pub user_name: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub status: String,
    pub create_date: String,
}

/// `POST /_awsim/auth/reveal-access-key`
///
/// Returns the plaintext secret access key for an existing
/// access-key ID. AWS hides the secret after creation, but AWSim is
/// a local-dev emulator and persists it plaintext on the user
/// record. Exposing it here means developers can recover credentials
/// after closing the create-key dialog instead of rotating the key.
///
/// Gated by the operator-auth middleware so only signed-in operators
/// can call it; off entirely when operator auth is disabled because
/// that mode is single-user dev where any client already has access
/// to the snapshot file.
pub async fn reveal_access_key(
    State(state): State<OperatorAuthState>,
    Json(req): Json<RevealSecretRequest>,
) -> Response {
    let iam_state = state
        .iam
        .store()
        .get(&state.default_account_id, awsim_iam::IAM_REGION);
    let user = match iam_state.users.get(&req.user_name) {
        Some(u) => u,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "code": "NoSuchEntity",
                    "message": format!("User {} not found.", req.user_name),
                })),
            )
                .into_response();
        }
    };
    let key = user
        .access_keys
        .iter()
        .find(|k| k.access_key_id == req.access_key_id)
        .cloned();
    drop(user);
    match key {
        Some(k) => Json(RevealSecretResponse {
            user_name: req.user_name,
            access_key_id: k.access_key_id,
            secret_access_key: k.secret_access_key,
            status: k.status,
            create_date: k.create_date,
        })
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "code": "NoSuchEntity",
                "message": format!(
                    "Access key {} not found on user {}.",
                    req.access_key_id, req.user_name
                ),
            })),
        )
            .into_response(),
    }
}

/// `POST /_awsim/auth/logout`
///
/// Clears the cookie. Sessions are stateless so there's nothing
/// server-side to revoke; the client must drop the token.
pub async fn logout() -> Response {
    let clear = format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0");
    let mut resp = Json(json!({"status": "ok"})).into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, clear.parse().unwrap());
    resp
}

/// `GET /_awsim/auth/whoami`
///
/// Always returns 200 with a status envelope so the UI can decide
/// what to render without parsing distinct status codes:
///
/// ```json
/// { "auth_required": false, "setup_required": false, "principal": null }
/// { "auth_required": true,  "setup_required": true,  "principal": null }
/// { "auth_required": true,  "setup_required": false, "principal": null }
/// { "auth_required": true,  "setup_required": false, "principal": "iam-user:..." }
/// ```
///
/// The previous shape returned 401 unconditionally when no session
/// was attached, which made the loginless dev flow indistinguishable
/// from an enabled-but-not-signed-in state.
pub async fn whoami(State(state): State<OperatorAuthState>, headers: HeaderMap) -> Response {
    let auth_required = require_operator_auth_enabled();
    let setup_required =
        auth_required && matches!(state.bootstrap_state(), BootstrapState::Pending { .. });
    let principal = resolve_session(&headers);
    Json(json!({
        "auth_required": auth_required,
        "setup_required": setup_required,
        "principal": principal,
    }))
    .into_response()
}

/// Axum middleware that enforces operator-auth on the admin
/// endpoints when `AWSIM_REQUIRE_OPERATOR_AUTH=true`. Off by
/// default so single-user dev / test setups keep working.
///
/// Returns 503 when the bootstrap flow has not yet run so the
/// operator gets a clear pointer at `/_awsim/auth/setup` instead
/// of a confusing 401.
pub async fn require_auth(
    State(state): State<OperatorAuthState>,
    headers: HeaderMap,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    if !require_operator_auth_enabled() {
        return next.run(req).await;
    }
    if matches!(state.bootstrap_state(), BootstrapState::Pending { .. }) {
        return setup_required_response();
    }
    if resolve_session(&headers).is_some() {
        return next.run(req).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "code": "UnauthorizedException",
            "message": "AWSIM_REQUIRE_OPERATOR_AUTH is on; sign in via POST /_awsim/auth/login.",
        })),
    )
        .into_response()
}

/// Cached once-per-process read of `AWSIM_REQUIRE_OPERATOR_AUTH`.
fn require_operator_auth_enabled() -> bool {
    use std::sync::OnceLock;
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("AWSIM_REQUIRE_OPERATOR_AUTH")
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false)
    })
}

/// Pull the bearer token from an `Authorization: Bearer ...` header
/// or the `awsim_session` cookie and verify it. Returns the
/// principal stripped of the `operator:` prefix.
fn resolve_session(headers: &HeaderMap) -> Option<String> {
    let raw = header_token(headers).or_else(|| cookie_token(headers))?;
    let principal = bearer_token::verify(&raw).ok()?;
    principal.strip_prefix(PRINCIPAL_PREFIX).map(str::to_string)
}

fn header_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(str::to_string)
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    for entry in raw.split(';') {
        let entry = entry.trim();
        if let Some(value) = entry.strip_prefix(&format!("{SESSION_COOKIE}=")) {
            return Some(value.to_string());
        }
    }
    None
}

/// Enforce MFA when the user has an enabled virtual MFA device.
fn require_mfa_if_enabled(
    iam_state: &Arc<awsim_iam::state::IamState>,
    user_name: &str,
    code: Option<&str>,
) -> Result<(), awsim_core::AwsError> {
    let user = match iam_state.users.get(user_name) {
        Some(u) => u,
        None => return Ok(()),
    };
    let serial = match user.mfa_devices.first() {
        Some(s) => s.clone(),
        None => return Ok(()),
    };
    drop(user);

    let device = iam_state
        .virtual_mfa_devices
        .get(&serial)
        .ok_or_else(|| awsim_core::AwsError::access_denied("MFA device missing on user."))?;
    let seed = device
        .base32_string_seed
        .as_deref()
        .ok_or_else(|| awsim_core::AwsError::access_denied("MFA device has no seed."))?;
    let code = code
        .ok_or_else(|| awsim_core::AwsError::access_denied("MFA code required for this user."))?;
    if !awsim_core::totp::verify_str(seed, code, 1) {
        return Err(awsim_core::AwsError::access_denied("Invalid MFA code."));
    }
    Ok(())
}

fn error_body(e: &awsim_core::AwsError) -> serde_json::Value {
    json!({
        "code": e.code,
        "message": e.message,
    })
}

#[cfg(test)]
mod throttle_tests {
    use super::*;

    fn empty_state() -> OperatorAuthState {
        OperatorAuthState::new(
            Arc::new(IamService::new()),
            "000000000000".to_string(),
            "us-east-1".to_string(),
        )
    }

    #[test]
    fn under_threshold_does_not_trip_throttle() {
        let s = empty_state();
        for _ in 0..(MAX_FAILED_ATTEMPTS - 1) {
            s.record_failure("alice");
            assert!(s.throttle_retry_after("alice").is_none());
        }
    }

    #[test]
    fn at_threshold_trips_throttle() {
        let s = empty_state();
        for _ in 0..MAX_FAILED_ATTEMPTS {
            s.record_failure("alice");
        }
        assert!(s.throttle_retry_after("alice").is_some());
    }

    #[test]
    fn throttle_is_per_username() {
        let s = empty_state();
        for _ in 0..MAX_FAILED_ATTEMPTS {
            s.record_failure("alice");
        }
        assert!(s.throttle_retry_after("alice").is_some());
        assert!(s.throttle_retry_after("bob").is_none());
    }

    #[test]
    fn successful_login_clears_failures() {
        let s = empty_state();
        for _ in 0..3 {
            s.record_failure("alice");
        }
        s.clear_failures("alice");
        for _ in 0..(MAX_FAILED_ATTEMPTS - 1) {
            s.record_failure("alice");
            assert!(s.throttle_retry_after("alice").is_none());
        }
    }

    #[test]
    fn retry_after_decreases_as_window_elapses() {
        let s = empty_state();
        for _ in 0..MAX_FAILED_ATTEMPTS {
            s.record_failure("alice");
        }
        let first = s.throttle_retry_after("alice").unwrap();
        assert!(first >= 1 && first <= THROTTLE_WINDOW.as_secs());
    }
}
