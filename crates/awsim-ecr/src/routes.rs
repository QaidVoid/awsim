use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use tracing::warn;

use crate::handler::EcrService;
use crate::operations::auth::validate_authorization_token;

pub fn router(service: Arc<EcrService>) -> axum::Router<()> {
    axum::Router::new()
        .route("/v2/{repo}/blobs/{digest}", axum::routing::get(get_blob))
        .with_state(service)
}

/// Validate the `Authorization` header for a registry HTTP request.
///
/// AWS clients send `Authorization: Basic base64(AWS:<token>)` where
/// `<token>` is the body we minted in
/// [`mint_authorization_token`](crate::operations::auth::mint_authorization_token).
/// When the header is absent we leave the request alone — local
/// testing should keep working without round-tripping
/// GetAuthorizationToken first. When the header IS present, a
/// malformed scheme or a forged / expired token is rejected with
/// `401 Unauthorized`.
fn enforce_authorization(headers: &HeaderMap) -> Result<(), (StatusCode, String)> {
    let Some(auth) = headers.get(header::AUTHORIZATION) else {
        return Ok(());
    };
    let auth = auth.to_str().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            "non-ASCII Authorization header".into(),
        )
    })?;
    let rest = auth.strip_prefix("Basic ").ok_or((
        StatusCode::UNAUTHORIZED,
        "Authorization must be Basic".into(),
    ))?;
    let decoded = BASE64.decode(rest.as_bytes()).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            "Authorization is not valid base64".into(),
        )
    })?;
    let creds = std::str::from_utf8(&decoded).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            "Authorization is not UTF-8".into(),
        )
    })?;
    // Basic creds are `username:password`; AWS uses the literal
    // "AWS" as the username and the minted token as the password.
    let token = match creds.split_once(':') {
        Some(("AWS", t)) if !t.is_empty() => t,
        _ => {
            return Err((
                StatusCode::UNAUTHORIZED,
                "Authorization must be AWS:<token>".into(),
            ));
        }
    };
    if let Err(e) = validate_authorization_token(token) {
        return Err((StatusCode::UNAUTHORIZED, e.message));
    }
    Ok(())
}

async fn get_blob(
    State(service): State<Arc<EcrService>>,
    Path((repo, digest)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if let Err(err) = enforce_authorization(&headers) {
        return err.into_response();
    }
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
