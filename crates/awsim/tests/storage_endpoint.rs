use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use aws_credential_types::Credentials;
use aws_sdk_s3::primitives::ByteStream;
use awsim_core::{AppState, BodyStoreHandle, ServiceHandler};
use axum::extract::State;
use axum::response::Json;
use serde_json::{Value, json};

async fn make_config(endpoint: &str) -> aws_config::SdkConfig {
    aws_config::defaults(aws_config::BehaviorVersion::latest())
        .endpoint_url(endpoint)
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(Credentials::new("test", "test", None, None, "test"))
        .load()
        .await
}

async fn make_s3_client(endpoint: &str) -> aws_sdk_s3::Client {
    let config = make_config(endpoint).await;
    let s3_config = aws_sdk_s3::config::Builder::from(&config)
        .force_path_style(true)
        .build();
    aws_sdk_s3::Client::from_conf(s3_config)
}

fn unique_temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("awsim-storage-test-{nanos}-{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn storage_handler(State(state): State<AppState>) -> Json<Value> {
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
    Json(json!({
        "data_dir": data_dir.display().to_string(),
        "services": services_json,
        "total_size_bytes": total,
    }))
}

async fn start_server_with_data_dir(data_dir: &std::path::Path) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let endpoint = format!("http://{addr}");

    let mut state = AppState::new("us-east-1".into(), "000000000000".into());

    let iam = Arc::new(awsim_iam::IamService::new());
    state.register(iam, vec![]);

    let sts = Arc::new(awsim_sts::StsService::new());
    state.register(sts, vec![]);

    let s3 = awsim_s3::S3Service::with_data_dir(data_dir);
    let s3_routes = s3.routes();
    let s3_arc = Arc::new(s3);
    let s3_clone = Arc::clone(&s3_arc);
    state.register(s3_arc, s3_routes);

    let mut handles: Vec<BodyStoreHandle> = Vec::new();
    if let Some(bs) = s3_clone.body_store() {
        handles.push(BodyStoreHandle {
            service_name: "s3".to_string(),
            groups: awsim_s3::S3Service::GROUPS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            body_store: Arc::clone(bs),
        });
    }
    state.body_stores = Arc::new(handles);
    state.data_dir = Some(Arc::new(data_dir.to_path_buf()));

    let app = axum::Router::new()
        .route("/_awsim/storage", axum::routing::get(storage_handler))
        .fallback(awsim_core::gateway::handle_request)
        .with_state(state)
        .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(tower_http::cors::CorsLayer::permissive());

    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    endpoint
}

#[tokio::test]
async fn storage_endpoint_reports_s3_disk_usage() {
    let data_dir = unique_temp_dir();
    let endpoint = start_server_with_data_dir(&data_dir).await;
    let client = make_s3_client(&endpoint).await;

    client
        .create_bucket()
        .bucket("storage-test")
        .send()
        .await
        .expect("CreateBucket failed");

    let payload = vec![0u8; 1024];
    client
        .put_object()
        .bucket("storage-test")
        .key("hello.bin")
        .body(ByteStream::from(payload))
        .send()
        .await
        .expect("PutObject failed");

    let response = reqwest::get(format!("{endpoint}/_awsim/storage"))
        .await
        .expect("GET /_awsim/storage failed");
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.expect("parse json");

    assert!(
        body.get("data_dir").and_then(|v| v.as_str()).is_some(),
        "data_dir missing: {body}"
    );

    let services = body
        .get("services")
        .and_then(|v| v.as_array())
        .expect("services array");
    let s3 = services
        .iter()
        .find(|s| s.get("name").and_then(|n| n.as_str()) == Some("s3"))
        .expect("s3 entry");
    assert!(
        s3.get("size_bytes").and_then(|n| n.as_u64()).unwrap_or(0) >= 1024,
        "s3 size_bytes too small: {s3}"
    );
    assert_eq!(
        s3.get("blob_count").and_then(|n| n.as_u64()).unwrap_or(0),
        1,
        "s3 blob_count mismatch: {s3}"
    );

    let _ = std::fs::remove_dir_all(&data_dir);
}

#[tokio::test]
async fn storage_endpoint_when_persistence_disabled() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let endpoint = format!("http://{addr}");

    let state = AppState::new("us-east-1".into(), "000000000000".into());
    let app = axum::Router::new()
        .route("/_awsim/storage", axum::routing::get(storage_handler))
        .with_state(state);

    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let body: Value = reqwest::get(format!("{endpoint}/_awsim/storage"))
        .await
        .expect("GET")
        .json()
        .await
        .expect("json");

    assert!(body.get("data_dir").is_some_and(|v| v.is_null()));
    assert_eq!(
        body.get("services")
            .and_then(|v| v.as_array())
            .map(|a| a.len()),
        Some(0)
    );
}
