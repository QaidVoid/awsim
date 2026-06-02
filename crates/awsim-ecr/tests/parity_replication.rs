use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use awsim_core::tick::TestDriver;
use awsim_core::{RequestContext, ServiceHandler};
use awsim_ecr::EcrService;
use serde_json::json;

const SRC_ACCOUNT: &str = "000000000000";
const SRC_REGION: &str = "us-east-1";
const DEST_ACCOUNT: &str = "111111111111";
const DEST_REGION: &str = "us-west-2";

fn tmp_dir(label: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("awsim-ecr-{label}-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ctx_for(account: &str, region: &str) -> RequestContext {
    let mut c = RequestContext::new("ecr", region);
    c.account_id = account.to_string();
    c
}

/// A replication rule + PutImage enqueues a task that the tick walks
/// PENDING -> IN_PROGRESS -> COMPLETE, then materializes the image and
/// its layers in the destination account/region state.
#[tokio::test]
async fn replication_task_walks_states_and_materializes_destination() {
    let root = tmp_dir("repl");
    let svc = Arc::new(EcrService::with_data_dir(&root));
    let src = ctx_for(SRC_ACCOUNT, SRC_REGION);

    svc.handle(
        "CreateRepository",
        json!({ "repositoryName": "repl-repo" }),
        &src,
    )
    .await
    .unwrap();

    // Configure a cross-account/region replication rule.
    svc.handle(
        "PutReplicationConfiguration",
        json!({
            "replicationConfiguration": {
                "rules": [{
                    "destinations": [{
                        "region": DEST_REGION,
                        "registryId": DEST_ACCOUNT,
                    }]
                }]
            }
        }),
        &src,
    )
    .await
    .unwrap();

    // Upload a layer so the source repo carries a blob to replicate.
    let init = svc
        .handle(
            "InitiateLayerUpload",
            json!({ "repositoryName": "repl-repo" }),
            &src,
        )
        .await
        .unwrap();
    let upload_id = init["uploadId"].as_str().unwrap().to_string();
    let payload: Vec<u8> = (0..256).map(|i| (i % 91 + 32) as u8).collect();
    let payload_str = String::from_utf8(payload).unwrap();
    svc.handle(
        "UploadLayerPart",
        json!({
            "repositoryName": "repl-repo",
            "uploadId": upload_id,
            "layerPartBlob": payload_str,
        }),
        &src,
    )
    .await
    .unwrap();
    let complete = svc
        .handle(
            "CompleteLayerUpload",
            json!({ "repositoryName": "repl-repo", "uploadId": upload_id }),
            &src,
        )
        .await
        .unwrap();
    let layer_digest = complete["layerDigest"].as_str().unwrap().to_string();

    // PutImage enqueues a PENDING replication task.
    let manifest = format!(r#"{{"schemaVersion":2,"layers":[{{"digest":"{layer_digest}"}}]}}"#);
    svc.handle(
        "PutImage",
        json!({
            "repositoryName": "repl-repo",
            "imageManifest": manifest,
            "imageTag": "v1",
        }),
        &src,
    )
    .await
    .unwrap();

    // Right after PutImage, before any tick: PENDING.
    assert_eq!(replication_status(&svc, &src).await, Some("PENDING".into()));

    let mut driver = TestDriver::new();
    driver.register(Arc::clone(&svc) as Arc<dyn ServiceHandler>);

    // First tick lands inside the IN_PROGRESS window [100ms, 400ms).
    tokio::time::sleep(Duration::from_millis(150)).await;
    driver.advance(Duration::from_millis(150)).await;
    assert_eq!(
        replication_status(&svc, &src).await,
        Some("IN_PROGRESS".into())
    );

    // Further delay crosses the COMPLETE threshold (>= 400ms total).
    tokio::time::sleep(Duration::from_millis(300)).await;
    driver.advance(Duration::from_millis(300)).await;
    assert_eq!(
        replication_status(&svc, &src).await,
        Some("COMPLETE".into())
    );

    // The image now materializes in the destination account/region.
    let dest = ctx_for(DEST_ACCOUNT, DEST_REGION);
    let describe = svc
        .handle(
            "DescribeImages",
            json!({ "repositoryName": "repl-repo" }),
            &dest,
        )
        .await
        .unwrap();
    let details = describe["imageDetails"].as_array().unwrap();
    assert_eq!(details.len(), 1, "image not replicated to destination");

    // And the replicated layer resolves at the destination.
    let dl = svc
        .handle(
            "GetDownloadUrlForLayer",
            json!({
                "repositoryName": "repl-repo",
                "layerDigest": layer_digest,
            }),
            &dest,
        )
        .await
        .unwrap();
    assert_eq!(dl["layerDigest"].as_str().unwrap(), layer_digest);

    let _ = std::fs::remove_dir_all(&root);
}

/// Read the single replication task's status via
/// DescribeImageReplicationStatus against the source registry.
async fn replication_status(svc: &EcrService, ctx: &RequestContext) -> Option<String> {
    let out = svc
        .handle(
            "DescribeImageReplicationStatus",
            json!({
                "repositoryName": "repl-repo",
                "imageId": { "imageTag": "v1" },
            }),
            ctx,
        )
        .await
        .ok()?;
    out["replicationStatuses"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|s| s["status"].as_str())
        .map(|s| s.to_string())
}
