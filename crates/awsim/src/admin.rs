use awsim_billing::{BillingMeter, compute_report};
use awsim_core::AppState;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json, Response};
use base64::Engine;
use bytes::Bytes;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};

pub async fn health(State(state): State<AppState>) -> Json<Value> {
    let uptime = state.start_time.elapsed().as_secs();
    Json(json!({
        "status": "ok",
        "service": "awsim",
        "version": env!("CARGO_PKG_VERSION"),
        "services": state.services.len(),
        "requests": state.request_count.load(Ordering::Relaxed),
        "uptime": uptime,
    }))
}

pub async fn list_services(State(state): State<AppState>) -> Json<Value> {
    let services: Vec<Value> = state
        .services
        .iter()
        .map(|(name, handler)| {
            json!({
                "name": name,
                "signingName": handler.signing_name(),
                "protocol": format!("{:?}", handler.protocol()),
            })
        })
        .collect();
    Json(json!({ "services": services }))
}

pub async fn config(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "region": state.default_region,
        "accountId": state.default_account_id,
        "services": state.services.len(),
    }))
}

pub async fn storage(State(state): State<AppState>) -> Json<Value> {
    let Some(data_dir) = state.data_dir.as_ref() else {
        return Json(json!({
            "data_dir": Value::Null,
            "services": [],
        }));
    };

    let mut services_json: Vec<Value> = Vec::with_capacity(state.body_stores.len());
    let mut total: u64 = 0;
    for handle in state.body_stores.iter() {
        let mut size_bytes: u64 = 0;
        let mut blob_count: usize = 0;
        for group in &handle.groups {
            size_bytes =
                size_bytes.saturating_add(handle.body_store.group_size(group).unwrap_or(0));
            blob_count =
                blob_count.saturating_add(handle.body_store.group_blob_count(group).unwrap_or(0));
        }
        total = total.saturating_add(size_bytes);
        services_json.push(json!({
            "name": handle.service_name,
            "groups": handle.groups,
            "size_bytes": size_bytes,
            "blob_count": blob_count,
        }));
    }

    let snapshots_path = data_dir.join("snapshots");
    let snapshots_size = dir_size(&snapshots_path).unwrap_or(0);

    Json(json!({
        "data_dir": data_dir.display().to_string(),
        "snapshots": {
            "path": snapshots_path.display().to_string(),
            "size_bytes": snapshots_size,
        },
        "services": services_json,
        "total_size_bytes": total,
    }))
}

fn dir_size(path: &std::path::Path) -> std::io::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let mut total: u64 = 0;
    let mut stack: Vec<std::path::PathBuf> = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        for entry in entries.flatten() {
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file()
                && let Ok(meta) = entry.metadata()
            {
                total = total.saturating_add(meta.len());
            }
        }
    }
    Ok(total)
}

pub async fn stats(State(state): State<AppState>) -> Json<Value> {
    let uptime = state.start_time.elapsed().as_secs();
    let requests = state.request_count.load(Ordering::Relaxed);
    Json(json!({
        "uptime": uptime,
        "uptimeFormatted": format_duration(uptime),
        "totalRequests": requests,
        "requestsPerSecond": requests.checked_div(uptime).unwrap_or(0),
        "services": state.services.len(),
    }))
}

pub async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.events.subscribe();
    let stream = BroadcastStream::new(receiver)
        .filter_map(|res| {
            res.ok()
                .and_then(|evt| Event::default().json_data(&evt).ok())
        })
        .map(Ok::<_, Infallible>);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Returns the captured headers + bodies for a single recent request,
/// or 404 if it has fallen out of the ring buffer.
pub async fn request_detail(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.request_details.get(&id) {
        Some(detail) => Json(detail).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "RequestNotFound", "id": id})),
        )
            .into_response(),
    }
}

/// Returns the most recent N captured request ids (newest first). Used by
/// the UI to power the "inspect last request" hotkey.
pub async fn recent_request_ids(State(state): State<AppState>) -> Json<Value> {
    let ids = state.request_details.recent_ids(50);
    Json(json!({ "ids": ids }))
}

/// Re-issues a captured request through the gateway pipeline and returns
/// the new id + status. The UI typically follows up with a GET on
/// `/_awsim/requests/{new_id}` to render the fresh response in the
/// inspect drawer.
///
/// Bails out when the original request body was truncated during capture,
/// since replaying a partial body would silently lie about the result.
pub async fn replay_request(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let detail = match state.request_details.get(&id) {
        Some(d) => d,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "RequestNotFound", "id": id})),
            )
                .into_response();
        }
    };

    if detail.request_body.truncated {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "RequestBodyTruncated",
                "message": "Original request body was truncated during capture; replay would not be faithful.",
                "captured_size": detail.request_body.size,
            })),
        )
            .into_response();
    }

    let method = match Method::from_bytes(detail.method.as_bytes()) {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "InvalidMethod", "message": e.to_string()})),
            )
                .into_response();
        }
    };

    let uri_str = match &detail.query {
        Some(q) => format!("{}?{}", detail.path, q),
        None => detail.path.clone(),
    };
    let uri: Uri = match uri_str.parse() {
        Ok(u) => u,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "InvalidUri", "message": e.to_string()})),
            )
                .into_response();
        }
    };

    // Reconstruct the header map from the captured pairs. Skip any header
    // that fails to parse — practically the only realistic failure is a
    // weird control character, and skipping is friendlier than 500ing.
    let mut headers = HeaderMap::new();
    for h in &detail.request_headers {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(h.name.as_bytes()),
            HeaderValue::from_str(&h.value),
        ) {
            headers.insert(name, value);
        }
    }

    let body_bytes = match &detail.request_body.data_b64 {
        Some(b64) => match base64::engine::general_purpose::STANDARD.decode(b64) {
            Ok(b) => Bytes::from(b),
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "InvalidBody", "message": e.to_string()})),
                )
                    .into_response();
            }
        },
        None => Bytes::new(),
    };

    let (response, new_id) =
        awsim_core::gateway::dispatch_request(&state, method, uri, headers, body_bytes).await;

    Json(json!({
        "new_id": new_id,
        "status_code": response.status().as_u16(),
        "original_id": id,
    }))
    .into_response()
}

fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    if hours > 0 {
        format!("{hours}h {mins}m {s}s")
    } else if mins > 0 {
        format!("{mins}m {s}s")
    } else {
        format!("{s}s")
    }
}

/// Returns the rolling estimated bill: per-service breakdown + projected
/// monthly cost based on the rate observed since the meter started.
///
/// Pricing covers the vertical-slice services (S3, Lambda, DynamoDB) for
/// us-east-1; everything else is omitted from the report rather than
/// faked at zero so users don't think they're seeing a complete bill.
pub async fn billing(State(meter): State<Arc<BillingMeter>>) -> Json<Value> {
    let report = compute_report(&meter.store, &meter.pricing);
    Json(serde_json::to_value(report).unwrap_or_else(|_| json!({"error": "serialise_failed"})))
}
