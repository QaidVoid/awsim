use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use tracing::warn;

use crate::handler::EcrService;

pub fn router(service: Arc<EcrService>) -> axum::Router<()> {
    axum::Router::new()
        .route("/v2/{repo}/blobs/{digest}", axum::routing::get(get_blob))
        .with_state(service)
}

async fn get_blob(
    State(service): State<Arc<EcrService>>,
    Path((repo, digest)): Path<(String, String)>,
) -> Response {
    let store = service.store();
    let mut maybe_layer = None;
    for ((_, _), state) in store.iter_all() {
        if let Some(repository) = state.repositories.get(&repo)
            && let Some(layer) = repository.layers.get(&digest)
        {
            maybe_layer = Some(layer.value().clone());
            break;
        }
    }

    let layer = match maybe_layer {
        Some(l) => l,
        None => return (StatusCode::NOT_FOUND, "layer not found").into_response(),
    };

    let bytes = match layer.body.read_all() {
        Ok(b) => b,
        Err(e) => {
            warn!(repo = %repo, digest = %digest, error = %e, "Failed to read ECR layer body");
            return (StatusCode::INTERNAL_SERVER_ERROR, "failed to read layer").into_response();
        }
    };

    let mut response = Response::new(Body::from(bytes));
    let headers = response.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&layer.media_type) {
        headers.insert(header::CONTENT_TYPE, v);
    }
    if let Ok(v) = HeaderValue::from_str(&digest) {
        headers.insert("Docker-Content-Digest", v);
    }
    response
}
