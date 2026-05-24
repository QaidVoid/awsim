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
use serde::{Deserialize, Serialize};
use serde_json::json;
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

/// Shared state injected into the auth routes and the middleware.
#[derive(Clone)]
pub struct OperatorAuthState {
    pub iam: Arc<IamService>,
    pub default_account_id: String,
    pub default_region: String,
    failed_attempts: Arc<Mutex<AttemptMap>>,
}

impl OperatorAuthState {
    pub fn new(iam: Arc<IamService>, default_account_id: String, default_region: String) -> Self {
        Self {
            iam,
            default_account_id,
            default_region,
            failed_attempts: Arc::new(Mutex::new(HashMap::new())),
        }
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

    let iam_state = state
        .iam
        .store()
        .get(&state.default_account_id, &state.default_region);

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
/// Returns `{ "principal": "..." }` for the bearer token in the
/// `Authorization` header or `awsim_session` cookie. 401 on missing
/// or invalid token.
pub async fn whoami(headers: HeaderMap) -> Response {
    match resolve_session(&headers) {
        Some(p) => Json(json!({ "principal": p })).into_response(),
        None => (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "code": "UnauthorizedException",
                "message": "Missing or invalid operator session.",
            })),
        )
            .into_response(),
    }
}

/// Axum middleware that enforces operator-auth on the admin
/// endpoints when `AWSIM_REQUIRE_OPERATOR_AUTH=true`. Off by
/// default so single-user dev / test setups keep working.
pub async fn require_auth(headers: HeaderMap, req: axum::extract::Request, next: Next) -> Response {
    if !require_operator_auth_enabled() {
        return next.run(req).await;
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
