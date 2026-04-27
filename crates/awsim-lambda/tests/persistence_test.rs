use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{RequestContext, ServiceHandler};
use awsim_lambda::LambdaService;
use base64::Engine;
use serde_json::{Value, json};
use zip::write::SimpleFileOptions;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn tmp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("awsim-lambda-persist-{label}-{nanos}-{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ctx() -> RequestContext {
    RequestContext::new("lambda", "us-east-1")
}

fn build_zip(payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        writer
            .start_file("handler.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(payload).unwrap();
        writer.finish().unwrap();
    }
    buf
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[tokio::test]
async fn create_then_restart_then_get_function() {
    let dir = tmp_dir("create-get");
    let zip_bytes = build_zip(b"first-version-payload");
    let zip_b64 = b64(&zip_bytes);

    let snapshot = {
        let svc = LambdaService::with_data_dir(&dir);
        svc.handle(
            "CreateFunction",
            json!({
                "FunctionName": "mychat",
                "Role": "arn:aws:iam::000000000000:role/lambda",
                "Runtime": "nodejs18.x",
                "Handler": "index.handler",
                "Code": { "ZipFile": zip_b64 },
            }),
            &ctx(),
        )
        .await
        .unwrap();
        svc.snapshot().expect("snapshot bytes")
    };

    let svc2 = LambdaService::with_data_dir(&dir);
    svc2.restore(&snapshot).expect("restore");

    let got = svc2
        .handle("GetFunction", json!({"FunctionName": "mychat"}), &ctx())
        .await
        .unwrap();
    let cfg = got.get("Configuration").expect("Configuration");
    assert_eq!(
        cfg.get("FunctionName").and_then(Value::as_str),
        Some("mychat")
    );

    let on_disk = std::fs::read(dir.join("lambda").join("mychat").join("$LATEST")).unwrap();
    assert_eq!(on_disk, zip_bytes);

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn publish_version_then_restart_keeps_versioned_code() {
    let dir = tmp_dir("publish-version");
    let zip_v1 = build_zip(b"v1");
    let zip_v2 = build_zip(b"v2");

    let snapshot = {
        let svc = LambdaService::with_data_dir(&dir);
        svc.handle(
            "CreateFunction",
            json!({
                "FunctionName": "myfn",
                "Role": "arn:aws:iam::000000000000:role/lambda",
                "Runtime": "nodejs18.x",
                "Handler": "index.handler",
                "Code": { "ZipFile": b64(&zip_v1) },
            }),
            &ctx(),
        )
        .await
        .unwrap();
        svc.handle("PublishVersion", json!({"FunctionName": "myfn"}), &ctx())
            .await
            .unwrap();
        svc.handle(
            "UpdateFunctionCode",
            json!({"FunctionName": "myfn", "ZipFile": b64(&zip_v2)}),
            &ctx(),
        )
        .await
        .unwrap();
        svc.snapshot().expect("snapshot")
    };

    let svc2 = LambdaService::with_data_dir(&dir);
    svc2.restore(&snapshot).expect("restore");

    let latest = std::fs::read(dir.join("lambda").join("myfn").join("$LATEST")).unwrap();
    assert_eq!(latest, zip_v2);
    let v1 = std::fs::read(dir.join("lambda").join("myfn").join("1")).unwrap();
    assert_eq!(v1, zip_v1);

    let versions = svc2
        .handle(
            "ListVersionsByFunction",
            json!({"FunctionName": "myfn"}),
            &ctx(),
        )
        .await
        .unwrap();
    let arr = versions.get("Versions").and_then(Value::as_array).unwrap();
    assert_eq!(arr.len(), 2);

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn delete_function_removes_persisted_code() {
    let dir = tmp_dir("delete");
    let zip_bytes = build_zip(b"to-delete");

    let svc = LambdaService::with_data_dir(&dir);
    svc.handle(
        "CreateFunction",
        json!({
            "FunctionName": "tempfn",
            "Role": "arn:aws:iam::000000000000:role/lambda",
            "Runtime": "nodejs18.x",
            "Handler": "index.handler",
            "Code": { "ZipFile": b64(&zip_bytes) },
        }),
        &ctx(),
    )
    .await
    .unwrap();
    assert!(dir.join("lambda").join("tempfn").join("$LATEST").exists());

    svc.handle("DeleteFunction", json!({"FunctionName": "tempfn"}), &ctx())
        .await
        .unwrap();

    assert!(!dir.join("lambda").join("tempfn").exists());

    let _ = std::fs::remove_dir_all(&dir);
}
