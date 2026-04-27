use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use awsim_core::{BlobInventory, RequestContext, ServiceHandler};
use awsim_sqs::SqsService;
use serde_json::json;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn tmp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("awsim-sqs-pergc-{label}-{nanos}-{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ctx() -> RequestContext {
    RequestContext::new("sqs", "us-east-1")
}

#[tokio::test]
async fn periodic_gc_removes_orphan_blob() {
    let dir = tmp_dir("orphan");
    let queue_name = "qgc1";

    let svc = Arc::new(SqsService::with_data_dir(&dir));
    svc.handle("CreateQueue", json!({"QueueName": queue_name}), &ctx())
        .await
        .unwrap();

    let bs = svc.body_store().expect("body store").clone();
    let orphan_path = bs
        .write_blob("sqs", queue_name, "orphan-msg-id", b"orphan body")
        .unwrap();
    assert!(orphan_path.exists());

    let svc_gc = Arc::clone(&svc);
    let interval = Duration::from_millis(200);
    let handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            let known: HashSet<(String, String, String)> =
                svc_gc.known_blobs().into_iter().collect();
            let _ = svc_gc
                .body_store()
                .unwrap()
                .gc_orphaned(SqsService::GROUPS, &known);
        }
    });

    tokio::time::sleep(Duration::from_millis(700)).await;
    handle.abort();

    assert!(
        !orphan_path.exists(),
        "orphan blob should have been GC'd: {orphan_path:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
