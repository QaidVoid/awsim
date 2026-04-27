use awsim_core::AppState;
use axum::extract::State;
use axum::response::Json;
use axum::response::sse::{Event, KeepAlive, Sse};
use serde_json::{Value, json};
use std::convert::Infallible;
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
