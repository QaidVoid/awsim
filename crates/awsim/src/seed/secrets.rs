//! Bulk-seed Secrets Manager secrets. SecretsState is fully public so
//! we can write directly into the AccountRegionStore — no service
//! method indirection needed.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::AccountRegionStore;
use awsim_secretsmanager::state::{Secret, SecretVersion, SecretsState};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use tracing::info;
use uuid::Uuid;

use super::{fake_sentence, fake_slug};

#[derive(Deserialize)]
pub struct SeedSecretsBody {
    pub count: u64,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
}

#[derive(Clone)]
pub struct SeedSecretsState {
    pub store: AccountRegionStore<SecretsState>,
    pub default_account: String,
    pub default_region: String,
}

const MAX_COUNT: u64 = 50_000;

pub async fn seed(
    State(state): State<Arc<SeedSecretsState>>,
    Json(body): Json<SeedSecretsBody>,
) -> Response {
    if body.count == 0 {
        return Json(json!({ "created": 0 })).into_response();
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

    let account = body
        .account
        .unwrap_or_else(|| state.default_account.clone());
    let region = body.region.unwrap_or_else(|| state.default_region.clone());
    let prefix = body.prefix.unwrap_or_else(|| "seed".to_string());

    let result = tokio::task::spawn_blocking(move || {
        let secrets_state = state.store.get(&account, &region);
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        let mut created = 0u64;
        for _ in 0..body.count {
            let name = format!("{prefix}-{}", fake_slug(2));
            if secrets_state.secrets.contains_key(&name) {
                continue;
            }
            let arn = format!(
                "arn:aws:secretsmanager:{region}:{account}:secret:{name}-{}",
                Uuid::new_v4().simple()
            );
            // Realistic-shaped credential blob — small JSON like what
            // most apps store: { username, password, host, port }.
            let secret_string = json!({
                "username": format!("user-{}", fake_slug(1)),
                "password": Uuid::new_v4().simple().to_string(),
                "host":     format!("db-{}.example.test", fake_slug(1)),
                "port":     5432,
            })
            .to_string();
            let version_id = Uuid::new_v4().to_string();
            let mut versions = HashMap::new();
            versions.insert(
                version_id.clone(),
                SecretVersion {
                    version_id: version_id.clone(),
                    secret_string: Some(secret_string),
                    secret_binary: None,
                    stages: vec!["AWSCURRENT".to_string()],
                    created_date: now_secs,
                },
            );
            let secret = Secret {
                arn,
                name: name.clone(),
                description: fake_sentence(),
                versions,
                current_version_id: version_id,
                tags: HashMap::new(),
                created_date: now_secs,
                last_changed_date: now_secs,
                deleted_date: None,
                rotation_enabled: false,
                rotation_lambda_arn: None,
                rotation_automatically_after_days: None,
            };
            secrets_state.secrets.insert(name, secret);
            created += 1;
        }
        created
    })
    .await;

    match result {
        Ok(created) => {
            info!(target = "seed", created, "Seeded Secrets Manager");
            Json(json!({ "created": created })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "JoinError", "message": e.to_string() })),
        )
            .into_response(),
    }
}
