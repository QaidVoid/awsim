use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{RequestContext, ServiceHandler};
use awsim_s3::S3Service;
use base64::Engine;
use serde_json::{Value, json};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn tmp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("awsim-s3-persist-{label}-{nanos}-{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ctx() -> RequestContext {
    RequestContext::new("s3", "us-east-1")
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn decode(value: &Value) -> Vec<u8> {
    let raw = value
        .get("__raw_body")
        .and_then(Value::as_str)
        .or_else(|| value.get("Body").and_then(Value::as_str))
        .expect("expected body");
    base64::engine::general_purpose::STANDARD
        .decode(raw)
        .unwrap()
}

#[tokio::test]
async fn put_then_restart_then_get() {
    let dir = tmp_dir("putget");
    let bucket = "mybucket";
    let key = "folder/foo.txt";
    let payload: &[u8] = b"hello world";

    let snapshot = {
        let svc = S3Service::with_data_dir(&dir);
        svc.handle("CreateBucket", json!({"Bucket": bucket}), &ctx())
            .await
            .unwrap();
        svc.handle(
            "PutObject",
            json!({
                "Bucket": bucket,
                "Key": key,
                "__raw_body": b64(payload),
            }),
            &ctx(),
        )
        .await
        .unwrap();
        svc.snapshot().expect("snapshot bytes")
    };

    let svc2 = S3Service::with_data_dir(&dir);
    svc2.restore(&snapshot).expect("restore");

    let got = svc2
        .handle("GetObject", json!({"Bucket": bucket, "Key": key}), &ctx())
        .await
        .unwrap();
    assert_eq!(decode(&got), payload);

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn versioned_history_survives_snapshot_and_restore() {
    let dir = tmp_dir("versions");
    let bucket = "vbucket";
    let key = "log.txt";

    let (v1, v2, snapshot) = {
        let svc = S3Service::with_data_dir(&dir);
        svc.handle("CreateBucket", json!({"Bucket": bucket}), &ctx())
            .await
            .unwrap();
        // Turn versioning on so successive PutObjects retain history.
        svc.handle(
            "PutBucketVersioning",
            json!({
                "Bucket": bucket,
                "VersioningConfiguration": { "Status": "Enabled" },
            }),
            &ctx(),
        )
        .await
        .unwrap();
        let r1 = svc
            .handle(
                "PutObject",
                json!({ "Bucket": bucket, "Key": key, "__raw_body": b64(b"v1") }),
                &ctx(),
            )
            .await
            .unwrap();
        let r2 = svc
            .handle(
                "PutObject",
                json!({ "Bucket": bucket, "Key": key, "__raw_body": b64(b"v2") }),
                &ctx(),
            )
            .await
            .unwrap();
        let v1 = r1["VersionId"].as_str().unwrap().to_string();
        let v2 = r2["VersionId"].as_str().unwrap().to_string();
        (v1, v2, svc.snapshot().expect("snapshot bytes"))
    };

    let svc2 = S3Service::with_data_dir(&dir);
    svc2.restore(&snapshot).expect("restore");

    // The latest version is readable as before.
    let latest = svc2
        .handle("GetObject", json!({"Bucket": bucket, "Key": key}), &ctx())
        .await
        .unwrap();
    assert_eq!(decode(&latest), b"v2");

    // The historical version body is still readable by ID.
    let historical = svc2
        .handle(
            "GetObject",
            json!({"Bucket": bucket, "Key": key, "VersionId": v1}),
            &ctx(),
        )
        .await
        .unwrap();
    assert_eq!(decode(&historical), b"v1");

    // And the second-version ID still resolves to the same v2 body.
    let by_id = svc2
        .handle(
            "GetObject",
            json!({"Bucket": bucket, "Key": key, "VersionId": v2}),
            &ctx(),
        )
        .await
        .unwrap();
    assert_eq!(decode(&by_id), b"v2");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn multipart_then_restart_then_get() {
    let dir = tmp_dir("multipart");
    let bucket = "buck";
    let key = "big/object.bin";
    // A single part is treated as the final part and has no minimum size,
    // matching AWS' EntityTooSmall rule which only applies to non-final parts.
    let part1: &[u8] = b"AAAAAAAAAA";

    let snapshot = {
        let svc = S3Service::with_data_dir(&dir);
        svc.handle("CreateBucket", json!({"Bucket": bucket}), &ctx())
            .await
            .unwrap();
        let init = svc
            .handle(
                "CreateMultipartUpload",
                json!({"Bucket": bucket, "Key": key}),
                &ctx(),
            )
            .await
            .unwrap();
        let upload_id = init
            .get("UploadId")
            .and_then(Value::as_str)
            .unwrap()
            .to_string();

        let part1_resp = svc
            .handle(
                "UploadPart",
                json!({
                    "Bucket": bucket,
                    "Key": key,
                    "uploadId": upload_id,
                    "partNumber": "1",
                    "__raw_body": b64(part1),
                }),
                &ctx(),
            )
            .await
            .unwrap();
        let etag1 = part1_resp["ETag"].as_str().unwrap().to_string();
        svc.handle(
            "CompleteMultipartUpload",
            json!({
                "Bucket": bucket,
                "Key": key,
                "uploadId": upload_id,
                "CompleteMultipartUpload": {
                    "Part": [
                        {"PartNumber": "1", "ETag": etag1}
                    ]
                }
            }),
            &ctx(),
        )
        .await
        .unwrap();

        svc.snapshot().expect("snapshot bytes")
    };

    let svc2 = S3Service::with_data_dir(&dir);
    svc2.restore(&snapshot).expect("restore");

    let got = svc2
        .handle("GetObject", json!({"Bucket": bucket, "Key": key}), &ctx())
        .await
        .unwrap();
    assert_eq!(decode(&got), part1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn overwrite_on_disk_keeps_the_new_body() {
    let dir = tmp_dir("overwrite");
    let svc = S3Service::with_data_dir(&dir);
    svc.handle("CreateBucket", json!({"Bucket": "bucket"}), &ctx())
        .await
        .unwrap();
    for body in [b"one".as_slice(), b"one two".as_slice()] {
        svc.handle(
            "PutObject",
            json!({ "Bucket": "bucket", "Key": "k.txt", "__raw_body": b64(body) }),
            &ctx(),
        )
        .await
        .unwrap();
    }
    let got = svc
        .handle(
            "GetObject",
            json!({ "Bucket": "bucket", "Key": "k.txt" }),
            &ctx(),
        )
        .await
        .unwrap();
    assert_eq!(decode(&got), b"one two");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn put_if_match_writes_only_over_the_etag_named() {
    let dir = tmp_dir("ifmatch");
    let svc = S3Service::with_data_dir(&dir);
    svc.handle("CreateBucket", json!({"Bucket": "bucket"}), &ctx())
        .await
        .unwrap();
    let request = ctx();
    let put = |body: &[u8], if_match: Option<&str>| {
        let mut input = json!({ "Bucket": "bucket", "Key": "k.txt", "__raw_body": b64(body) });
        if let Some(etag) = if_match {
            input["IfMatch"] = json!(etag);
        }
        svc.handle("PutObject", input, &request)
    };
    let missing = put(b"x", Some("\"abc\"")).await.unwrap_err();
    assert_eq!(missing.code, "NoSuchKey");
    let first = put(b"one", None).await.unwrap();
    let etag = first["ETag"].as_str().unwrap().to_string();
    put(b"one two", Some(&etag)).await.unwrap();
    let stale = put(b"lost", Some(&etag)).await.unwrap_err();
    assert_eq!(stale.code, "PreconditionFailed");
    let got = svc
        .handle(
            "GetObject",
            json!({ "Bucket": "bucket", "Key": "k.txt" }),
            &ctx(),
        )
        .await
        .unwrap();
    assert_eq!(decode(&got), b"one two");
    let _ = std::fs::remove_dir_all(&dir);
}
