//! Amazon OpenSearch (Elasticsearch-compatible) emulator for AWSim.
//!
//! Unlike other AWS services, OpenSearch exposes an Elasticsearch-compatible
//! REST API rather than using AWS API protocols. This crate provides an Axum
//! router that handles index management, document CRUD, and search queries.

#![deny(warnings)]

mod operations;
pub mod state;
mod util;

use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::{delete, get, head, post, put};
use serde::Deserialize;
use serde_json::{Value, json};

use state::OpenSearchState;

/// Build an Axum router for the OpenSearch Elasticsearch-compatible API.
///
/// Mount this at `/opensearch` in the main server.
pub fn router(state: Arc<OpenSearchState>) -> Router {
    Router::new()
        // Cluster info
        .route("/", get(cluster_info))
        // Cluster health
        .route("/_cluster/health", get(cluster_health_handler))
        // Tasks API
        .route("/_tasks/{task_id}", get(task_handler))
        // Aliases
        .route("/_aliases", post(aliases_handler))
        // Reindex
        .route("/_reindex", post(reindex_handler))
        // Multi-search (global)
        .route("/_msearch", post(msearch_global_handler))
        // Multi-get (global)
        .route("/_mget", post(mget_global_handler))
        // Cat APIs
        .route("/_cat/indices", get(cat_indices))
        // Bulk API
        .route("/_bulk", post(bulk_handler))
        // Index operations
        .route("/{index}", put(create_index))
        .route("/{index}", get(get_index))
        .route("/{index}", head(head_index))
        .route("/{index}", delete(delete_index))
        .route("/{index}/_mapping", get(get_mapping))
        .route("/{index}/_mapping", put(put_mapping))
        .route("/{index}/_refresh", post(refresh_handler))
        .route("/{index}/_count", post(count))
        .route("/{index}/_count", get(count))
        // Search
        .route("/{index}/_search", post(search))
        .route("/{index}/_search", get(search))
        // Multi-search (per-index)
        .route("/{index}/_msearch", post(msearch_index_handler))
        // Multi-get (per-index)
        .route("/{index}/_mget", post(mget_index_handler))
        // Document operations
        .route("/{index}/_doc/{id}", put(put_doc))
        .route("/{index}/_doc/{id}", post(put_doc))
        .route("/{index}/_doc/{id}", get(get_doc))
        .route("/{index}/_doc/{id}", delete(delete_doc))
        .route("/{index}/_doc", post(post_doc_auto_id))
        // Update document
        .route("/{index}/_update/{id}", post(update_doc_handler))
        // Update by query
        .route("/{index}/_update_by_query", post(update_by_query_handler))
        // Delete by query
        .route("/{index}/_delete_by_query", post(delete_by_query_handler))
        // Source-only get
        .route("/{index}/_source/{id}", get(get_source_handler))
        // Bulk per index
        .route("/{index}/_bulk", post(bulk_index_handler))
        .with_state(state)
}

// --- Handlers ---

async fn cluster_info() -> Json<Value> {
    Json(json!({
        "name": "awsim-opensearch",
        "cluster_name": "awsim",
        "cluster_uuid": "awsim-local",
        "version": {
            "distribution": "opensearch",
            "number": "3.6.0",
            "build_type": "tar",
            "build_hash": "awsim",
            "build_date": "2025-01-01T00:00:00Z",
            "build_snapshot": false,
            "lucene_version": "10.2.0",
            "minimum_wire_compatibility_version": "3.0.0",
            "minimum_index_compatibility_version": "3.0.0",
        },
        "tagline": "The OpenSearch Project: https://opensearch.org/",
    }))
}

async fn cat_indices(State(state): State<Arc<OpenSearchState>>) -> Json<Value> {
    let (_, body) = operations::index::cat_indices(&state);
    Json(body)
}

async fn create_index(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
    body: Option<Json<Value>>,
) -> impl IntoResponse {
    let body = body.map(|b| b.0).unwrap_or(json!({}));
    let (status, result) = operations::index::create_index(&state, &index, &body);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn get_index(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
) -> impl IntoResponse {
    let (status, result) = operations::index::get_index(&state, &index);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn head_index(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
) -> impl IntoResponse {
    let status = operations::index::index_exists(&state, &index);
    StatusCode::from_u16(status).unwrap_or(StatusCode::NOT_FOUND)
}

async fn delete_index(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
) -> impl IntoResponse {
    let (status, result) = operations::index::delete_index(&state, &index);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn get_mapping(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
) -> impl IntoResponse {
    let (status, result) = operations::index::get_mapping(&state, &index);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn put_mapping(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
    body: Option<Json<Value>>,
) -> impl IntoResponse {
    let body = body.map(|b| b.0).unwrap_or(json!({}));
    let (status, result) = operations::index::put_mapping(&state, &index, &body);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn refresh_handler(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
) -> impl IntoResponse {
    let (status, result) = operations::index::refresh(&state, &index);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn search(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
    body: Option<Json<Value>>,
) -> impl IntoResponse {
    let body = body
        .map(|b| b.0)
        .unwrap_or(json!({"query": {"match_all": {}}}));
    let (status, result) = operations::search::search(&state, &index, &body);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn count(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
    body: Option<Json<Value>>,
) -> impl IntoResponse {
    let body = body
        .map(|b| b.0)
        .unwrap_or(json!({"query": {"match_all": {}}}));
    let (status, result) = operations::search::count(&state, &index, &body);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn put_doc(
    State(state): State<Arc<OpenSearchState>>,
    Path((index, id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let (status, result) = operations::document::index_document(&state, &index, Some(&id), &body);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn post_doc_auto_id(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let (status, result) = operations::document::index_document(&state, &index, None, &body);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::CREATED),
        Json(result),
    )
}

async fn get_doc(
    State(state): State<Arc<OpenSearchState>>,
    Path((index, id)): Path<(String, String)>,
) -> impl IntoResponse {
    let (status, result) = operations::document::get_document(&state, &index, &id);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn delete_doc(
    State(state): State<Arc<OpenSearchState>>,
    Path((index, id)): Path<(String, String)>,
) -> impl IntoResponse {
    let (status, result) = operations::document::delete_document(&state, &index, &id);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn bulk_handler(
    State(state): State<Arc<OpenSearchState>>,
    body: String,
) -> impl IntoResponse {
    let (status, result) = operations::bulk::bulk(&state, None, &body);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn bulk_index_handler(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
    body: String,
) -> impl IntoResponse {
    let (status, result) = operations::bulk::bulk(&state, Some(&index), &body);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

// --- Cluster-level handlers ---

async fn cluster_health_handler(State(state): State<Arc<OpenSearchState>>) -> impl IntoResponse {
    let (status, result) = operations::cluster::cluster_health(&state);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn task_handler(Path(task_id): Path<String>) -> impl IntoResponse {
    let (status, result) = operations::cluster::get_task(&task_id);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

#[derive(Deserialize)]
struct ReindexParams {
    wait_for_completion: Option<bool>,
}

async fn reindex_handler(
    State(state): State<Arc<OpenSearchState>>,
    Query(params): Query<ReindexParams>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let wait = params.wait_for_completion.unwrap_or(true);
    let (status, result) = operations::cluster::reindex(&state, &body, wait);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn aliases_handler(
    State(state): State<Arc<OpenSearchState>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let (status, result) = operations::cluster::update_aliases(&state, &body);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn msearch_global_handler(
    State(state): State<Arc<OpenSearchState>>,
    body: String,
) -> impl IntoResponse {
    let (status, result) = operations::cluster::msearch(&state, None, &body);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn msearch_index_handler(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
    body: String,
) -> impl IntoResponse {
    let (status, result) = operations::cluster::msearch(&state, Some(&index), &body);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

// --- Document-level handlers ---

async fn update_doc_handler(
    State(state): State<Arc<OpenSearchState>>,
    Path((index, id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let (status, result) = operations::document::update_document(&state, &index, &id, &body);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn update_by_query_handler(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
    body: Option<Json<Value>>,
) -> impl IntoResponse {
    let body = body
        .map(|b| b.0)
        .unwrap_or(json!({"query": {"match_all": {}}}));
    let (status, result) = operations::document::update_by_query(&state, &index, &body);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn delete_by_query_handler(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
    body: Option<Json<Value>>,
) -> impl IntoResponse {
    let body = body
        .map(|b| b.0)
        .unwrap_or(json!({"query": {"match_all": {}}}));
    let (status, result) = operations::document::delete_by_query(&state, &index, &body);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn mget_global_handler(
    State(state): State<Arc<OpenSearchState>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let index = body.get("index").and_then(|v| v.as_str()).unwrap_or("_all");
    let (status, result) = operations::document::mget(&state, index, &body);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn mget_index_handler(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let (status, result) = operations::document::mget(&state, &index, &body);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(result),
    )
}

async fn get_source_handler(
    State(state): State<Arc<OpenSearchState>>,
    Path((index, id)): Path<(String, String)>,
) -> impl IntoResponse {
    let (status, result) = operations::document::get_document(&state, &index, &id);
    if status == 200 {
        // Return just the _source without the metadata wrapper
        let source = result["_source"].clone();
        (StatusCode::OK, Json(source))
    } else {
        (
            StatusCode::from_u16(status).unwrap_or(StatusCode::NOT_FOUND),
            Json(result),
        )
    }
}
