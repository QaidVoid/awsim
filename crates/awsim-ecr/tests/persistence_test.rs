use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{RequestContext, ServiceHandler};
use awsim_ecr::EcrService;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn ctx() -> RequestContext {
    RequestContext::new("ecr", "us-east-1")
}

fn tmp_dir(label: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("awsim-ecr-{label}-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn layer_round_trip_with_persistence() {
    let root = tmp_dir("rt");
    let svc = EcrService::with_data_dir(&root);
    let ctx = ctx();

    svc.handle(
        "CreateRepository",
        json!({ "repositoryName": "round-trip" }),
        &ctx,
    )
    .await
    .unwrap();

    let init = svc
        .handle(
            "InitiateLayerUpload",
            json!({ "repositoryName": "round-trip" }),
            &ctx,
        )
        .await
        .unwrap();
    let upload_id = init["uploadId"].as_str().unwrap().to_string();

    let payload: Vec<u8> = (0..1024).map(|i| (i % 251) as u8).collect();
    let payload_str = String::from_utf8_lossy(&payload).to_string();

    svc.handle(
        "UploadLayerPart",
        json!({
            "repositoryName": "round-trip",
            "uploadId": upload_id,
            "layerPartBlob": payload_str,
        }),
        &ctx,
    )
    .await
    .unwrap();

    let complete = svc
        .handle(
            "CompleteLayerUpload",
            json!({
                "repositoryName": "round-trip",
                "uploadId": upload_id,
            }),
            &ctx,
        )
        .await
        .unwrap();
    let digest = complete["layerDigest"].as_str().unwrap().to_string();

    let avail = svc
        .handle(
            "BatchCheckLayerAvailability",
            json!({
                "repositoryName": "round-trip",
                "layerDigests": [digest.clone()],
            }),
            &ctx,
        )
        .await
        .unwrap();
    let layers = avail["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0]["layerAvailability"], "AVAILABLE");

    let snap = svc.snapshot().expect("snapshot");
    drop(svc);

    let svc2 = EcrService::with_data_dir(&root);
    svc2.restore(&snap).unwrap();

    let avail2 = svc2
        .handle(
            "BatchCheckLayerAvailability",
            json!({
                "repositoryName": "round-trip",
                "layerDigests": [digest.clone()],
            }),
            &ctx,
        )
        .await
        .unwrap();
    let layers2 = avail2["layers"].as_array().unwrap();
    assert_eq!(layers2.len(), 1);
    assert_eq!(layers2[0]["layerAvailability"], "AVAILABLE");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let svc_arc = Arc::new(svc2.with_port(port));
    let app = awsim_ecr::router(Arc::clone(&svc_arc));

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await;
    });

    let path = format!("/v2/round-trip/blobs/{digest}");
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n");
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();

    let split = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("headers/body delimiter");
    let header_bytes = &buf[..split];
    let body = &buf[split + 4..];

    let header_str = std::str::from_utf8(header_bytes).unwrap();
    let header_lower = header_str.to_ascii_lowercase();
    assert!(
        header_str.starts_with("HTTP/1.1 200"),
        "status line: {header_str}"
    );
    assert!(
        header_lower.contains("docker-content-digest:"),
        "missing digest header: {header_str}"
    );
    assert!(
        header_lower.contains(&digest.to_ascii_lowercase()),
        "digest mismatch: {header_str}"
    );
    assert!(
        header_lower.contains("application/vnd.docker.image.rootfs.diff.tar.gzip"),
        "missing content-type: {header_str}"
    );

    assert_eq!(body, payload_str.as_bytes());

    let _ = tx.send(());
    let _ = server.await;

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn two_part_upload_digest_matches_concat_and_temp_removed() {
    use sha2::{Digest, Sha256};

    let root = tmp_dir("two-part");
    let svc = EcrService::with_data_dir(&root);
    let ctx = ctx();

    svc.handle(
        "CreateRepository",
        json!({ "repositoryName": "multi-part" }),
        &ctx,
    )
    .await
    .unwrap();

    let init = svc
        .handle(
            "InitiateLayerUpload",
            json!({ "repositoryName": "multi-part" }),
            &ctx,
        )
        .await
        .unwrap();
    let upload_id = init["uploadId"].as_str().unwrap().to_string();

    // Two contiguous parts. Use bytes that stay valid UTF-8 round-trips.
    let part1: Vec<u8> = (0..512).map(|i| (i % 91 + 32) as u8).collect();
    let part2: Vec<u8> = (0..300).map(|i| ((i * 7) % 91 + 32) as u8).collect();
    let part1_str = String::from_utf8(part1.clone()).unwrap();
    let part2_str = String::from_utf8(part2.clone()).unwrap();

    let r1 = svc
        .handle(
            "UploadLayerPart",
            json!({
                "repositoryName": "multi-part",
                "uploadId": upload_id,
                "partFirstByte": 0,
                "partLastByte": part1.len() as u64 - 1,
                "layerPartBlob": part1_str,
            }),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(r1["lastByteReceived"].as_u64().unwrap(), part1.len() as u64);

    let r2 = svc
        .handle(
            "UploadLayerPart",
            json!({
                "repositoryName": "multi-part",
                "uploadId": upload_id,
                "partFirstByte": part1.len() as u64,
                "partLastByte": part1.len() as u64 + part2.len() as u64 - 1,
                "layerPartBlob": part2_str,
            }),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(
        r2["lastByteReceived"].as_u64().unwrap(),
        (part1.len() + part2.len()) as u64
    );

    // The temp upload blob should exist while the upload is in progress.
    let temp_path = root.join("ecr").join("_uploads").join(&upload_id);
    assert!(temp_path.exists(), "temp upload blob missing mid-upload");

    let complete = svc
        .handle(
            "CompleteLayerUpload",
            json!({
                "repositoryName": "multi-part",
                "uploadId": upload_id,
            }),
            &ctx,
        )
        .await
        .unwrap();
    let digest = complete["layerDigest"].as_str().unwrap().to_string();

    // Digest equals sha256 of the concatenated parts.
    let mut hasher = Sha256::new();
    hasher.update(&part1);
    hasher.update(&part2);
    let expected = format!("sha256:{:x}", hasher.finalize());
    assert_eq!(digest, expected, "digest must match concat sha256");

    // The temp `_uploads/<id>` blob is gone after completion.
    assert!(
        !temp_path.exists(),
        "temp upload blob still present after CompleteLayerUpload"
    );

    // The finalized layer is available under its digest.
    let avail = svc
        .handle(
            "BatchCheckLayerAvailability",
            json!({
                "repositoryName": "multi-part",
                "layerDigests": [digest.clone()],
            }),
            &ctx,
        )
        .await
        .unwrap();
    let layers = avail["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0]["layerAvailability"], "AVAILABLE");
    assert_eq!(
        layers[0]["layerSize"].as_u64().unwrap(),
        (part1.len() + part2.len()) as u64
    );

    let _ = std::fs::remove_dir_all(&root);
}
