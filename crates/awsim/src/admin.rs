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

// ---------------------------------------------------------------------------
// Chaos engine admin endpoints
// ---------------------------------------------------------------------------

use awsim_chaos::{ChaosEngine, ChaosRule};
use std::sync::atomic::Ordering as AtomicOrdering;

pub async fn chaos_presets_list() -> Json<Value> {
    let entries: Vec<Value> = awsim_chaos::PRESETS
        .iter()
        .map(|p| {
            json!({
                "name": p.name,
                "description": p.description,
            })
        })
        .collect();
    Json(json!({ "presets": entries }))
}

/// POST /_awsim/chaos/presets/{name} — appends the preset's rules
/// to the engine. Returns the new rule ids.
pub async fn chaos_preset_apply(
    State(engine): State<Arc<ChaosEngine>>,
    Path(name): Path<String>,
) -> Response {
    let Some(rules) = awsim_chaos::presets::build(&name) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "PresetNotFound", "name": name})),
        )
            .into_response();
    };
    let ids: Vec<String> = rules.iter().map(|r| r.id.clone()).collect();
    for rule in rules {
        engine.add_rule(rule);
    }
    (
        StatusCode::CREATED,
        Json(json!({ "preset": name, "rule_ids": ids })),
    )
        .into_response()
}

pub async fn chaos_list(State(engine): State<Arc<ChaosEngine>>) -> Json<Value> {
    let rules = engine.rules();
    Json(json!({
        "rules": rules,
        "total_injections": engine.total_injections.load(AtomicOrdering::Relaxed),
    }))
}

/// POST /_awsim/chaos/rules — accepts a `ChaosRule` body. The
/// caller can omit `id` / `created_at` / `injection_count` and we
/// fill them in.
pub async fn chaos_add(
    State(engine): State<Arc<ChaosEngine>>,
    Json(body): Json<Value>,
) -> Response {
    let mut rule: ChaosRule = match serde_json::from_value(body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "InvalidRule", "message": e.to_string()})),
            )
                .into_response();
        }
    };
    if rule.id.is_empty() {
        rule.id = uuid::Uuid::new_v4().to_string();
    }
    if rule.created_at == 0 {
        rule.created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
    }
    rule.injection_count = 0;
    let id = rule.id.clone();
    engine.add_rule(rule);
    (StatusCode::CREATED, Json(json!({"id": id}))).into_response()
}

pub async fn chaos_remove(
    State(engine): State<Arc<ChaosEngine>>,
    Path(id): Path<String>,
) -> Response {
    if engine.remove_rule(&id) {
        (StatusCode::NO_CONTENT, ()).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "RuleNotFound", "id": id})),
        )
            .into_response()
    }
}

#[derive(serde::Deserialize)]
pub struct ChaosPatchBody {
    pub enabled: Option<bool>,
}

pub async fn chaos_patch(
    State(engine): State<Arc<ChaosEngine>>,
    Path(id): Path<String>,
    Json(body): Json<ChaosPatchBody>,
) -> Response {
    if let Some(enabled) = body.enabled
        && !engine.set_enabled(&id, enabled)
    {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "RuleNotFound", "id": id})),
        )
            .into_response();
    }
    (StatusCode::NO_CONTENT, ()).into_response()
}

pub async fn chaos_clear(State(engine): State<Arc<ChaosEngine>>) -> Response {
    engine.clear();
    (StatusCode::NO_CONTENT, ()).into_response()
}

pub async fn chaos_stats(State(engine): State<Arc<ChaosEngine>>) -> Json<Value> {
    Json(json!({
        "total_injections": engine.total_injections.load(AtomicOrdering::Relaxed),
        "recent": engine.recent_injections(),
    }))
}

// ---------------------------------------------------------------------------
// DynamoDB admin — VACUUM
// ---------------------------------------------------------------------------

use awsim_dynamodb::DynamoDbService;

// ---------------------------------------------------------------------------
// SQLite-backed storage stats — row counts + db file sizes for the
// four high-volume services. Surfaces real numbers so users can see
// where their memory / disk went.
// ---------------------------------------------------------------------------

pub struct SqliteStatsState {
    pub dynamodb: Arc<DynamoDbService>,
    pub cw_logs: Arc<awsim_cloudwatch_logs::CloudWatchLogsService>,
    pub cw_metrics: Arc<awsim_cloudwatch_metrics::CloudWatchMetricsService>,
    pub kinesis: Arc<awsim_kinesis::KinesisService>,
    pub ses: Arc<awsim_ses::SesService>,
}

fn file_size(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

pub async fn sqlite_stats(State(s): State<Arc<SqliteStatsState>>) -> Json<Value> {
    use awsim_cloudwatch_logs as cwl;
    use awsim_cloudwatch_metrics as cwm;
    use awsim_kinesis as kin;
    use awsim_ses as ses;

    // DynamoDB doesn't expose a public sqlite handle, so we just
    // report the file size for now. Row counts on the other three
    // come straight from each store.
    // DynamoDB only exposes its tempdir path; for the persistent
    // case we'd need a public `db_path()` accessor on the service.
    // Report the tempdir-based file size when available, 0 otherwise.
    let dynamodb_db = s.dynamodb.tempdir_path().map(|p| p.join("dynamodb.db"));
    let dynamodb_size = dynamodb_db.as_deref().map(file_size).unwrap_or(0);

    let cwl_store: Option<Arc<cwl::SqliteStore>> = sqlite_store_for_logs(&s.cw_logs);
    let cwm_store: Option<Arc<cwm::SqliteStore>> = sqlite_store_for_cwm(&s.cw_metrics);
    let kinesis_store: Option<Arc<kin::SqliteStore>> = sqlite_store_for_kinesis(&s.kinesis);

    let cwl_rows = cwl_store
        .as_ref()
        .and_then(|s| s.total_rows().ok())
        .unwrap_or(0);
    let cwl_size = cwl_store
        .as_ref()
        .map(|s| file_size(s.db_path()))
        .unwrap_or(0);

    let cwm_rows = cwm_store
        .as_ref()
        .and_then(|s| s.total_rows().ok())
        .unwrap_or(0);
    let cwm_size = cwm_store
        .as_ref()
        .map(|s| file_size(s.db_path()))
        .unwrap_or(0);

    let kinesis_rows = kinesis_store
        .as_ref()
        .and_then(|s| s.total_rows().ok())
        .unwrap_or(0);
    let kinesis_size = kinesis_store
        .as_ref()
        .map(|s| file_size(s.db_path()))
        .unwrap_or(0);

    let ses_store: Option<Arc<ses::SqliteStore>> = sqlite_store_for_ses(&s.ses);
    let ses_rows = ses_store
        .as_ref()
        .and_then(|s| s.total_rows().ok())
        .unwrap_or(0);
    let ses_size = ses_store
        .as_ref()
        .map(|s| file_size(s.db_path()))
        .unwrap_or(0);

    Json(json!({
        "stores": [
            {
                "service": "dynamodb",
                "rows": Value::Null,
                "size_bytes": dynamodb_size,
            },
            {
                "service": "cloudwatch-logs",
                "rows": cwl_rows,
                "size_bytes": cwl_size,
            },
            {
                "service": "cloudwatch-metrics",
                "rows": cwm_rows,
                "size_bytes": cwm_size,
            },
            {
                "service": "kinesis",
                "rows": kinesis_rows,
                "size_bytes": kinesis_size,
            },
            {
                "service": "ses",
                "rows": ses_rows,
                "size_bytes": ses_size,
            },
        ]
    }))
}

// Internal accessors. The services don't currently expose a public
// `sqlite_store()` method (it would be tempting to leak the type),
// so we walk through public test helpers / known paths instead.
fn sqlite_store_for_logs(
    svc: &Arc<awsim_cloudwatch_logs::CloudWatchLogsService>,
) -> Option<Arc<awsim_cloudwatch_logs::SqliteStore>> {
    svc.sqlite_store_handle()
}
fn sqlite_store_for_cwm(
    svc: &Arc<awsim_cloudwatch_metrics::CloudWatchMetricsService>,
) -> Option<Arc<awsim_cloudwatch_metrics::SqliteStore>> {
    svc.sqlite_store_handle()
}
fn sqlite_store_for_kinesis(
    svc: &Arc<awsim_kinesis::KinesisService>,
) -> Option<Arc<awsim_kinesis::SqliteStore>> {
    svc.sqlite_store_handle()
}
fn sqlite_store_for_ses(svc: &Arc<awsim_ses::SesService>) -> Option<Arc<awsim_ses::SqliteStore>> {
    svc.sqlite_store_handle()
}

// ---------------------------------------------------------------------------
// Memory diagnostic — counts entries in every major in-memory store
// so users can diff snapshots and pinpoint what's growing without a
// heap profiler. Bring up the page, hammer a workload, refresh — the
// section that grew is your leak.
// ---------------------------------------------------------------------------

pub struct DebugObjectsState {
    pub app: awsim_core::AppState,
    pub billing: Arc<awsim_billing::BillingMeter>,
    pub cognito: Arc<awsim_cognito::CognitoState>,
    pub sqlite: Arc<SqliteStatsState>,
}

fn read_proc_status(label: &str) -> Option<u64> {
    let raw = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix(label) {
            let kib = rest
                .trim()
                .trim_end_matches("kB")
                .trim()
                .parse::<u64>()
                .ok()?;
            return Some(kib * 1024);
        }
    }
    None
}

fn proc_section() -> Value {
    json!({
        "rss_bytes": read_proc_status("VmRSS:"),
        "vm_size_bytes": read_proc_status("VmSize:"),
        "vm_data_bytes": read_proc_status("VmData:"),
        "vm_peak_bytes": read_proc_status("VmPeak:"),
        "vm_hwm_bytes":  read_proc_status("VmHWM:"),
    })
}

fn cognito_section(cognito: &awsim_cognito::CognitoState) -> Value {
    let mut total_users: u64 = 0;
    let mut total_groups: u64 = 0;
    let mut total_clients: u64 = 0;
    let mut total_auth_events: u64 = 0;
    let mut total_devices: u64 = 0;
    let mut total_revoked_tokens: u64 = 0;
    let mut per_pool: Vec<Value> = Vec::new();
    for entry in cognito.user_pools.iter() {
        let pool = entry.value();
        let users = pool.users.len() as u64;
        let groups = pool.groups.len() as u64;
        let clients = pool.clients.len() as u64;
        let auth_events: u64 = pool
            .users
            .values()
            .map(|u| u.auth_events.len() as u64)
            .sum();
        let devices: u64 = pool.users.values().map(|u| u.devices.len() as u64).sum();
        let revoked: u64 = pool
            .users
            .values()
            .map(|u| u.revoked_refresh_tokens.len() as u64)
            .sum();
        total_users += users;
        total_groups += groups;
        total_clients += clients;
        total_auth_events += auth_events;
        total_devices += devices;
        total_revoked_tokens += revoked;
        per_pool.push(json!({
            "id": entry.key(),
            "users": users,
            "groups": groups,
            "clients": clients,
            "auth_events_total": auth_events,
            "devices_total": devices,
            "revoked_refresh_tokens_total": revoked,
        }));
    }
    json!({
        "user_pools": cognito.user_pools.len(),
        "mfa_sessions": cognito.mfa_sessions.len(),
        "totals": {
            "users": total_users,
            "groups": total_groups,
            "clients": total_clients,
            "auth_events": total_auth_events,
            "devices": total_devices,
            "revoked_refresh_tokens": total_revoked_tokens,
        },
        "per_pool": per_pool,
    })
}

fn billing_section(meter: &awsim_billing::BillingMeter) -> Value {
    use awsim_core::Snapshottable;
    let entries = meter.store.iter_all();
    let mut total_op_counters: u64 = 0;
    let mut total_storage_rows: u64 = 0;
    let mut total_compute_rows: u64 = 0;
    let mut total_resource_rows: u64 = 0;
    for ((account, region), state) in &entries {
        let snap = state.to_snapshot(account, region);
        for ops in snap.services.values() {
            total_op_counters += ops.len() as u64;
        }
        total_storage_rows += snap.storage.len() as u64;
        total_compute_rows += snap.compute.len() as u64;
        total_resource_rows += snap.resources.len() as u64;
    }
    json!({
        "account_region_buckets": entries.len(),
        "op_counters_total": total_op_counters,
        "storage_rows_total": total_storage_rows,
        "compute_rows_total": total_compute_rows,
        "resource_rows_total": total_resource_rows,
    })
}

fn app_section(app: &awsim_core::AppState) -> Value {
    json!({
        "request_count": app.request_count.load(std::sync::atomic::Ordering::Relaxed),
        "request_details": app.request_details.recent_ids(usize::MAX).len(),
        "registered_services": app.services.len(),
        "request_event_subscribers": app.events.subscriber_count(),
        "internal_event_subscribers": app.event_bus.subscriber_count(),
        "chaos_rules": app.chaos.rules().len(),
        "chaos_recent_injections": app.chaos.recent_injections().len(),
        "uptime_secs": app.start_time.elapsed().as_secs(),
    })
}

fn sqlite_section(s: &SqliteStatsState) -> Value {
    let cwl_store = sqlite_store_for_logs(&s.cw_logs);
    let cwm_store = sqlite_store_for_cwm(&s.cw_metrics);
    let kin_store = sqlite_store_for_kinesis(&s.kinesis);
    let ses_store = sqlite_store_for_ses(&s.ses);
    json!({
        "cloudwatch_logs_rows": cwl_store.as_ref().and_then(|s| s.total_rows().ok()),
        "cloudwatch_metrics_rows": cwm_store.as_ref().and_then(|s| s.total_rows().ok()),
        "kinesis_rows": kin_store.as_ref().and_then(|s| s.total_rows().ok()),
        "ses_rows": ses_store.as_ref().and_then(|s| s.total_rows().ok()),
        "dynamodb_db_size_bytes": s.dynamodb.tempdir_path()
            .map(|p| file_size(&p.join("dynamodb.db"))),
    })
}

/// GET /_awsim/debug/objects — counts everything that grows in
/// memory. Snapshot before + after a workload, diff client-side.
pub async fn debug_objects(State(s): State<Arc<DebugObjectsState>>) -> Json<Value> {
    Json(json!({
        "captured_at": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        "process": proc_section(),
        "app": app_section(&s.app),
        "cognito": cognito_section(&s.cognito),
        "billing": billing_section(&s.billing),
        "sqlite": sqlite_section(&s.sqlite),
    }))
}

// ---------------------------------------------------------------------------
// SES sent-email inspector — reads `SesService::list_sent_emails()` and
// surfaces every captured outbound message so users can verify what was
// sent without parsing the SDK call.
// ---------------------------------------------------------------------------

/// GET /_awsim/ses/sent — list every captured outbound email,
/// newest-first, scoped optionally by `?account=` and `?region=`.
pub async fn ses_sent(
    State(svc): State<Arc<awsim_ses::SesService>>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    let account_filter = q.get("account").cloned();
    let region_filter = q.get("region").cloned();

    let entries = svc.list_sent_emails();
    let emails: Vec<Value> = entries
        .into_iter()
        .filter(|(account, region, _)| {
            account_filter
                .as_ref()
                .map(|a| a == account)
                .unwrap_or(true)
                && region_filter.as_ref().map(|r| r == region).unwrap_or(true)
        })
        .map(|(account, region, e)| {
            json!({
                "account": account,
                "region": region,
                "messageId": e.message_id,
                "from": e.from,
                "to": e.to,
                "cc": e.cc,
                "bcc": e.bcc,
                "subject": e.subject,
                "bodyText": e.body_text,
                "bodyHtml": e.body_html,
                "raw": e.raw,
                "sentAt": e.sent_at,
            })
        })
        .collect();

    Json(json!({ "count": emails.len(), "emails": emails }))
}

/// POST /_awsim/admin/dynamodb/vacuum — reclaim disk space after
/// heavy DELETE / UPDATE churn. Runs SQLite VACUUM, which can take
/// time on large databases, so it's exposed as an explicit admin
/// op rather than running on every shutdown.
pub async fn ddb_vacuum(State(svc): State<Arc<DynamoDbService>>) -> Response {
    let svc = Arc::clone(&svc);
    let result = tokio::task::spawn_blocking(move || svc.vacuum()).await;
    match result {
        Ok(Ok(())) => Json(json!({"status": "ok"})).into_response(),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "VacuumFailed", "message": e.message})),
        )
            .into_response(),
        Err(join) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "JoinError", "message": join.to_string()})),
        )
            .into_response(),
    }
}
