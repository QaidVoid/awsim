//! Bulk-seed users into a Cognito user pool. Writes directly into
//! `CognitoState.user_pools` — bypasses the SigV4 / gateway path so a
//! 10k-user seed completes in well under a second instead of taking
//! the full request-cycle hit per user.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_cognito::CognitoState;
use awsim_cognito::state::CognitoUser;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use super::{fake_email, fake_name, pick, probability};

#[derive(Deserialize)]
pub struct SeedCognitoUsersBody {
    /// Pool ID to seed into. Must already exist.
    pub pool_id: String,
    /// Number of users to create. Capped at 100k per call to keep
    /// the writer responsive.
    pub count: u64,
    /// Optional username prefix; default `seed-`.
    #[serde(default)]
    pub prefix: Option<String>,
    /// Default password assigned to every seeded user. Defaults to
    /// `Seed-Pass-1234!` which clears the standard pool policy.
    #[serde(default)]
    pub password: Option<String>,
}

const STATUSES: &[&str] = &[
    // 80% CONFIRMED, 15% FORCE_CHANGE_PASSWORD, 5% UNCONFIRMED — biased
    // via repetition since the picker is uniform.
    "CONFIRMED",
    "CONFIRMED",
    "CONFIRMED",
    "CONFIRMED",
    "CONFIRMED",
    "CONFIRMED",
    "CONFIRMED",
    "CONFIRMED",
    "FORCE_CHANGE_PASSWORD",
    "FORCE_CHANGE_PASSWORD",
    "UNCONFIRMED",
];

const MAX_COUNT: u64 = 100_000;
const SAMPLE_LIMIT: usize = 5;

pub async fn seed(
    State(state): State<Arc<CognitoState>>,
    Json(body): Json<SeedCognitoUsersBody>,
) -> Response {
    if body.count == 0 {
        return Json(json!({ "created": 0, "skipped": 0 })).into_response();
    }
    if body.count > MAX_COUNT {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "ValidationException",
                "message": format!("count must be ≤ {MAX_COUNT}"),
            })),
        )
            .into_response();
    }

    let prefix = body.prefix.unwrap_or_else(|| "seed-".to_string());
    let password = body
        .password
        .unwrap_or_else(|| "Seed-Pass-1234!".to_string());

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let result = tokio::task::spawn_blocking(move || -> Result<Value, String> {
        let started = std::time::Instant::now();
        let mut pool = state
            .user_pools
            .get_mut(&body.pool_id)
            .ok_or_else(|| format!("Pool not found: {}", body.pool_id))?;

        let pool_name = pool.name.clone();
        let mut created = 0u64;
        let mut skipped = 0u64;
        let mut status_confirmed = 0u64;
        let mut status_force_change = 0u64;
        let mut status_unconfirmed = 0u64;
        let mut samples: Vec<Value> = Vec::with_capacity(SAMPLE_LIMIT);
        for i in 0..body.count {
            let username = format!("{prefix}{i}-{}", Uuid::new_v4().simple());
            if pool.users.contains_key(&username) {
                skipped += 1;
                continue;
            }
            let name = fake_name();
            let (given, family) = split_name(&name);
            let email = fake_email();
            let status = (*pick(STATUSES)).to_string();
            let enabled = probability(0.95);
            let email_verified = status == "CONFIRMED" && probability(0.9);

            match status.as_str() {
                "CONFIRMED" => status_confirmed += 1,
                "FORCE_CHANGE_PASSWORD" => status_force_change += 1,
                "UNCONFIRMED" => status_unconfirmed += 1,
                _ => {}
            }

            let mut attributes = HashMap::new();
            let sub = Uuid::new_v4().to_string();
            attributes.insert("sub".to_string(), sub.clone());
            attributes.insert("email".to_string(), email.clone());
            attributes.insert("given_name".to_string(), given);
            attributes.insert("family_name".to_string(), family);
            attributes.insert("name".to_string(), name);
            if email_verified {
                attributes.insert("email_verified".to_string(), "true".to_string());
            }

            if samples.len() < SAMPLE_LIMIT {
                samples.push(json!({
                    "username": username,
                    "email": email,
                    "status": status,
                }));
            }

            let user = CognitoUser {
                username: username.clone(),
                sub,
                password: password.clone(),
                attributes,
                status,
                enabled,
                groups: Vec::new(),
                created_date: now,
                pending_verifications: HashMap::new(),
                revoked_refresh_tokens: Vec::new(),
                mfa_enabled: false,
                mfa_preferred: None,
                totp_secret: None,
                totp_verified: false,
                devices: Vec::new(),
                linked_providers: Vec::new(),
                mfa_options: Vec::new(),
                webauthn_credentials: Vec::new(),
                webauthn_pending_challenge: None,
                failed_login_attempts: 0,
                locked_until_secs: None,
                auth_events: Vec::new(),
            };
            pool.users.insert(username, user);
            created += 1;
        }
        let elapsed_ms = started.elapsed().as_millis() as u64;
        Ok(json!({
            "created": created,
            "skipped": skipped,
            "pool_id": body.pool_id,
            "pool_name": pool_name,
            "password": password,
            "username_prefix": prefix,
            "elapsed_ms": elapsed_ms,
            "status_breakdown": {
                "CONFIRMED": status_confirmed,
                "FORCE_CHANGE_PASSWORD": status_force_change,
                "UNCONFIRMED": status_unconfirmed,
            },
            "sample_users": samples,
        }))
    })
    .await;

    match result {
        Ok(Ok(v)) => {
            info!(target = "seed", value = %v, "Seeded Cognito users");
            Json(v).into_response()
        }
        Ok(Err(msg)) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "ResourceNotFoundException", "message": msg })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "JoinError", "message": e.to_string() })),
        )
            .into_response(),
    }
}

fn split_name(full: &str) -> (String, String) {
    let mut parts = full.splitn(2, ' ');
    let given = parts.next().unwrap_or("First").to_string();
    let family = parts.next().unwrap_or("Last").to_string();
    (given, family)
}
