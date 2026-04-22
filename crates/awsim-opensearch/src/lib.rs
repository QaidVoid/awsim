//! Amazon OpenSearch (Elasticsearch-compatible) emulator for AWSim.
//!
//! Unlike other AWS services, OpenSearch exposes an Elasticsearch-compatible
//! REST API rather than using AWS API protocols. This crate provides an Axum
//! router that handles index management, document CRUD, and search queries.

mod operations;
pub mod state;
mod util;

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::{delete, get, head, post, put};
use axum::Router;
use serde_json::{json, Value};

use state::OpenSearchState;

/// Build an Axum router for the OpenSearch Elasticsearch-compatible API.
///
/// Mount this at `/opensearch` in the main server.
pub fn router(state: Arc<OpenSearchState>) -> Router {
    Router::new()
        // Cluster info
        .route("/", get(cluster_info))
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
        .route("/{index}/_count", post(count))
        // Search
        .route("/{index}/_search", post(search))
        .route("/{index}/_search", get(search))
        // Document operations
        .route("/{index}/_doc/{id}", put(put_doc))
        .route("/{index}/_doc/{id}", post(put_doc))
        .route("/{index}/_doc/{id}", get(get_doc))
        .route("/{index}/_doc/{id}", delete(delete_doc))
        .route("/{index}/_doc", post(post_doc_auto_id))
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
            "number": "2.11.0",
            "build_type": "tar",
            "lucene_version": "9.7.0",
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
    (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(result))
}

async fn get_index(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
) -> impl IntoResponse {
    let (status, result) = operations::index::get_index(&state, &index);
    (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(result))
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
    (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(result))
}

async fn get_mapping(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
) -> impl IntoResponse {
    let (status, result) = operations::index::get_mapping(&state, &index);
    (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(result))
}

async fn search(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
    body: Option<Json<Value>>,
) -> impl IntoResponse {
    let body = body.map(|b| b.0).unwrap_or(json!({"query": {"match_all": {}}}));
    let (status, result) = operations::search::search(&state, &index, &body);
    (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(result))
}

async fn count(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
    body: Option<Json<Value>>,
) -> impl IntoResponse {
    let body = body.map(|b| b.0).unwrap_or(json!({"query": {"match_all": {}}}));
    let (status, result) = operations::search::count(&state, &index, &body);
    (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(result))
}

async fn put_doc(
    State(state): State<Arc<OpenSearchState>>,
    Path((index, id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let (status, result) = operations::document::index_document(&state, &index, Some(&id), &body);
    (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(result))
}

async fn post_doc_auto_id(
    State(state): State<Arc<OpenSearchState>>,
    Path(index): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let (status, result) = operations::document::index_document(&state, &index, None, &body);
    (StatusCode::from_u16(status).unwrap_or(StatusCode::CREATED), Json(result))
}

async fn get_doc(
    State(state): State<Arc<OpenSearchState>>,
    Path((index, id)): Path<(String, String)>,
) -> impl IntoResponse {
    let (status, result) = operations::document::get_document(&state, &index, &id);
    (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(result))
}

async fn delete_doc(
    State(state): State<Arc<OpenSearchState>>,
    Path((index, id)): Path<(String, String)>,
) -> impl IntoResponse {
    let (status, result) = operations::document::delete_document(&state, &index, &id);
    (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(result))
}

async fn bulk_handler(
    State(state): State<Arc<OpenSearchState>>,
    body: String,
) -> impl IntoResponse {
    let (status, result) = operations::bulk::bulk(&state, &body);
    (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(result))
}

async fn bulk_index_handler(
    State(state): State<Arc<OpenSearchState>>,
    Path(_index): Path<String>,
    body: String,
) -> impl IntoResponse {
    let (status, result) = operations::bulk::bulk(&state, &body);
    (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(result))
}
