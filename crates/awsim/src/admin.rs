use awsim_core::AppState;
use axum::extract::State;
use axum::response::Json;
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

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

pub async fn stats(State(state): State<AppState>) -> Json<Value> {
    let uptime = state.start_time.elapsed().as_secs();
    let requests = state.request_count.load(Ordering::Relaxed);
    Json(json!({
        "uptime": uptime,
        "uptimeFormatted": format_duration(uptime),
        "totalRequests": requests,
        "requestsPerSecond": if uptime > 0 { requests / uptime } else { 0 },
        "services": state.services.len(),
    }))
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
