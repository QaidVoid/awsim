//! Bulk-seed S3 buckets + small objects via `S3Service::seed`.

use std::sync::Arc;

use awsim_s3::{S3Service, SeedDatasetInput};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use tracing::info;

#[derive(Deserialize)]
pub struct SeedS3Body {
    pub buckets: u64,
    #[serde(default)]
    pub objects_per_bucket: u64,
    /// Body bytes per object. Capped at 64 KiB. Default 256.
    #[serde(default)]
    pub body_bytes: Option<u64>,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
}

#[derive(Clone)]
pub struct SeedS3State {
    pub service: Arc<S3Service>,
    pub default_account: String,
    pub default_region: String,
}

const MAX_BUCKETS: u64 = 500;
const MAX_OBJECTS_PER_BUCKET: u64 = 10_000;

pub async fn seed(State(state): State<Arc<SeedS3State>>, Json(body): Json<SeedS3Body>) -> Response {
    if body.buckets == 0 {
        return Json(json!({ "buckets_created": 0, "objects_created": 0 })).into_response();
    }
    if body.buckets > MAX_BUCKETS {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "ValidationException",
                "message": format!("buckets must be ≤ {MAX_BUCKETS}"),
            })),
        )
            .into_response();
    }
    if body.objects_per_bucket > MAX_OBJECTS_PER_BUCKET {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "ValidationException",
                "message": format!("objects_per_bucket must be ≤ {MAX_OBJECTS_PER_BUCKET}"),
            })),
        )
            .into_response();
    }

    let input = SeedDatasetInput {
        account: body
            .account
            .unwrap_or_else(|| state.default_account.clone()),
        region: body.region.unwrap_or_else(|| state.default_region.clone()),
        buckets: body.buckets,
        objects_per_bucket: body.objects_per_bucket,
        body_bytes: body.body_bytes.unwrap_or(256),
        prefix: body.prefix.unwrap_or_else(|| "seed".to_string()),
    };

    let svc = Arc::clone(&state.service);
    let result = tokio::task::spawn_blocking(move || svc.seed(input)).await;
    match result {
        Ok(out) => {
            info!(
                target = "seed",
                buckets = out.buckets_created,
                objects = out.objects_created,
                "Seeded S3"
            );
            Json(json!({
                "buckets_created":    out.buckets_created,
                "objects_created":    out.objects_created,
                "bytes_written":      out.bytes_written,
                "elapsed_ms":         out.elapsed_ms,
                "objects_per_bucket": body.objects_per_bucket,
                "sample_buckets":     out.sample_buckets,
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "JoinError", "message": e.to_string() })),
        )
            .into_response(),
    }
}
