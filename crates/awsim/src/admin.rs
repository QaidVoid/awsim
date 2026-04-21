use axum::extract::State;
use axum::response::Json;
use awsim_core::AppState;
use serde_json::{json, Value};

pub async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "awsim",
        "version": env!("CARGO_PKG_VERSION"),
        "services": state.services.len(),
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
