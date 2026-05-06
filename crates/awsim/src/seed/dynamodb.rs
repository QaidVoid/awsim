//! Bulk-seed DynamoDB tables + items via `DynamoDbService::seed`.
//! Skips the SigV4 / gateway path so a 1k-table × 100-item seed
//! completes in well under a second.

use std::sync::Arc;

use awsim_dynamodb::{DynamoDbService, SeedDatasetInput};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use tracing::info;

#[derive(Deserialize)]
pub struct SeedDdbBody {
    /// Number of tables to create. Capped at 1 000 per call.
    pub tables: u64,
    /// Items per table. Capped at 100 000.
    #[serde(default)]
    pub items_per_table: u64,
    /// Optional table-name prefix; default `seed`.
    #[serde(default)]
    pub prefix: Option<String>,
    /// Account ID — defaults to the server's default account on
    /// the awsim-side once the request reaches the seeder.
    #[serde(default)]
    pub account: Option<String>,
    /// Region — same default rules as account.
    #[serde(default)]
    pub region: Option<String>,
}

#[derive(Clone)]
pub struct SeedDdbState {
    pub service: Arc<DynamoDbService>,
    pub default_account: String,
    pub default_region: String,
}

const MAX_TABLES: u64 = 1_000;
const MAX_ITEMS_PER_TABLE: u64 = 100_000;

pub async fn seed(
    State(state): State<Arc<SeedDdbState>>,
    Json(body): Json<SeedDdbBody>,
) -> Response {
    if body.tables == 0 {
        return Json(json!({ "tables_created": 0, "items_created": 0, "errors": [] }))
            .into_response();
    }
    if body.tables > MAX_TABLES {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "ValidationException",
                "message": format!("tables must be ≤ {MAX_TABLES}"),
            })),
        )
            .into_response();
    }
    if body.items_per_table > MAX_ITEMS_PER_TABLE {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "ValidationException",
                "message": format!("items_per_table must be ≤ {MAX_ITEMS_PER_TABLE}"),
            })),
        )
            .into_response();
    }

    let input = SeedDatasetInput {
        account: body
            .account
            .unwrap_or_else(|| state.default_account.clone()),
        region: body.region.unwrap_or_else(|| state.default_region.clone()),
        tables: body.tables,
        items_per_table: body.items_per_table,
        id_prefix: body.prefix.unwrap_or_else(|| "seed".to_string()),
    };

    let svc = Arc::clone(&state.service);
    let result = tokio::task::spawn_blocking(move || svc.seed(input)).await;
    match result {
        Ok(out) => {
            info!(
                target = "seed",
                tables = out.tables_created,
                items = out.items_created,
                errors = out.errors.len(),
                "Seeded DynamoDB"
            );
            Json(json!({
                "tables_created":  out.tables_created,
                "items_created":   out.items_created,
                "errors":          out.errors,
                "elapsed_ms":      out.elapsed_ms,
                "items_per_table": body.items_per_table,
                "sample_tables":   out.sample_tables,
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
