//! Named state snapshots — point-in-time bundles of every
//! ServiceHandler's JSON snapshot plus billing + chaos. Lives at
//! `{data_dir}/named-snapshots/{name}/` and is independent of the
//! shutdown-time `{data_dir}/snapshots/` directory.
//!
//! Limitations (v1): only captures JSON-serialisable handler state.
//! DynamoDB rows (SQLite) and body-store payloads (S3 objects, Lambda
//! code, SQS message bodies) are NOT captured — buckets/queues/tables
//! survive but their contents do not. This is good enough for sharing
//! topology + IAM + chaos scenarios; deeper bundling can come later.

use awsim_billing::BillingMeter;
use awsim_core::AppState;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const NAMED_DIR: &str = "named-snapshots";
const MANIFEST_FILE: &str = "manifest.json";
const BILLING_FILE: &str = "_billing.json";
const CHAOS_FILE: &str = "_chaos.json";

/// Composite state for the snapshot router — needs the live AppState
/// (services + chaos + data_dir) plus the billing meter.
pub struct SnapshotState {
    pub app: AppState,
    pub billing: Arc<BillingMeter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    name: String,
    created_ts: u64,
    awsim_version: String,
    services: Vec<String>,
    has_billing: bool,
    has_chaos: bool,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Reject names that would let a caller escape the snapshot root.
/// Only ASCII alphanumeric, `-`, `_`, max 64 chars.
fn validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("name must not be empty");
    }
    if name.len() > 64 {
        return Err("name must be at most 64 characters");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("name may only contain ASCII letters, digits, '-' and '_'");
    }
    Ok(())
}

fn snapshot_dir(data_dir: &Path, name: &str) -> PathBuf {
    data_dir.join(NAMED_DIR).join(name)
}

fn err_response(status: StatusCode, code: &str, message: impl ToString) -> Response {
    (
        status,
        Json(json!({ "error": code, "message": message.to_string() })),
    )
        .into_response()
}

fn no_data_dir() -> Response {
    err_response(
        StatusCode::PRECONDITION_FAILED,
        "DataDirRequired",
        "Named snapshots require awsim to be running with --data-dir.",
    )
}

fn invalid_name(reason: &str) -> Response {
    err_response(StatusCode::BAD_REQUEST, "InvalidName", reason)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

pub async fn list(State(s): State<Arc<SnapshotState>>) -> Response {
    let Some(data_dir) = s.app.data_dir.as_ref() else {
        return no_data_dir();
    };
    let root = data_dir.join(NAMED_DIR);
    let mut entries: Vec<Manifest> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let manifest_path = e.path().join(MANIFEST_FILE);
            if let Ok(bytes) = std::fs::read(&manifest_path)
                && let Ok(m) = serde_json::from_slice::<Manifest>(&bytes)
            {
                entries.push(m);
            }
        }
    }
    entries.sort_by_key(|m| std::cmp::Reverse(m.created_ts));
    Json(json!({ "snapshots": entries })).into_response()
}

pub async fn save(
    State(s): State<Arc<SnapshotState>>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    if let Err(e) = validate_name(&name) {
        return invalid_name(e);
    }
    let Some(data_dir) = s.app.data_dir.as_ref() else {
        return no_data_dir();
    };
    let dir = snapshot_dir(data_dir, &name);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, "IoError", e);
    }

    let mut services = Vec::new();
    for (svc_name, handler) in s.app.services.iter() {
        if let Some(bytes) = handler.snapshot() {
            let path = dir.join(format!("{svc_name}.json"));
            if let Err(e) = write_atomic(&path, &bytes) {
                return err_response(StatusCode::INTERNAL_SERVER_ERROR, "IoError", e);
            }
            services.push(svc_name.clone());
        }
    }
    services.sort();

    let mut has_billing = false;
    if let Some(bytes) = s.billing.store.snapshot_to_bytes() {
        if let Err(e) = write_atomic(&dir.join(BILLING_FILE), &bytes) {
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, "IoError", e);
        }
        has_billing = true;
    }

    let mut has_chaos = false;
    if let Some(bytes) = s.app.chaos.snapshot_to_bytes() {
        if let Err(e) = write_atomic(&dir.join(CHAOS_FILE), &bytes) {
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, "IoError", e);
        }
        has_chaos = true;
    }

    let manifest = Manifest {
        name: name.clone(),
        created_ts: now_secs(),
        awsim_version: env!("CARGO_PKG_VERSION").to_string(),
        services,
        has_billing,
        has_chaos,
    };
    let manifest_bytes = match serde_json::to_vec_pretty(&manifest) {
        Ok(b) => b,
        Err(e) => {
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, "SerializeFailed", e);
        }
    };
    if let Err(e) = write_atomic(&dir.join(MANIFEST_FILE), &manifest_bytes) {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, "IoError", e);
    }

    let body = serde_json::to_value(&manifest).unwrap_or(json!({}));
    (StatusCode::CREATED, Json(body)).into_response()
}

pub async fn load(
    State(s): State<Arc<SnapshotState>>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    if let Err(e) = validate_name(&name) {
        return invalid_name(e);
    }
    let Some(data_dir) = s.app.data_dir.as_ref() else {
        return no_data_dir();
    };
    let dir = snapshot_dir(data_dir, &name);
    let manifest_path = dir.join(MANIFEST_FILE);
    let manifest_bytes = match std::fs::read(&manifest_path) {
        Ok(b) => b,
        Err(_) => {
            return err_response(StatusCode::NOT_FOUND, "SnapshotNotFound", &name);
        }
    };
    let manifest: Manifest = match serde_json::from_slice(&manifest_bytes) {
        Ok(m) => m,
        Err(e) => {
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, "ManifestCorrupt", e);
        }
    };

    let mut restored = Vec::new();
    let mut failed: Vec<Value> = Vec::new();
    for svc_name in &manifest.services {
        let path = dir.join(format!("{svc_name}.json"));
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => {
                failed.push(json!({ "service": svc_name, "error": "missing file" }));
                continue;
            }
        };
        let Some(handler) = s.app.services.get(svc_name) else {
            failed.push(json!({ "service": svc_name, "error": "service not registered" }));
            continue;
        };
        match handler.restore(&bytes) {
            Ok(()) => restored.push(svc_name.clone()),
            Err(e) => failed.push(json!({ "service": svc_name, "error": e })),
        }
    }

    if manifest.has_billing
        && let Ok(bytes) = std::fs::read(dir.join(BILLING_FILE))
        && let Err(e) = s.billing.store.restore_from_bytes(&bytes)
    {
        failed.push(json!({ "service": "_billing", "error": e.to_string() }));
    }
    if manifest.has_chaos
        && let Ok(bytes) = std::fs::read(dir.join(CHAOS_FILE))
        && let Err(e) = s.app.chaos.restore_from_bytes(&bytes)
    {
        failed.push(json!({ "service": "_chaos", "error": e.to_string() }));
    }

    Json(json!({
        "name": name,
        "restored": restored,
        "failed": failed,
    }))
    .into_response()
}

pub async fn delete(
    State(s): State<Arc<SnapshotState>>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    if let Err(e) = validate_name(&name) {
        return invalid_name(e);
    }
    let Some(data_dir) = s.app.data_dir.as_ref() else {
        return no_data_dir();
    };
    let dir = snapshot_dir(data_dir, &name);
    if !dir.exists() {
        return err_response(StatusCode::NOT_FOUND, "SnapshotNotFound", &name);
    }
    if let Err(e) = std::fs::remove_dir_all(&dir) {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, "IoError", e);
    }
    (StatusCode::NO_CONTENT, ()).into_response()
}
