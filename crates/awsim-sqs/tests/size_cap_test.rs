use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{RequestContext, ServiceHandler};
use awsim_sqs::SqsService;
use serde_json::json;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn tmp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("awsim-sqs-cap-{label}-{nanos}-{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ctx() -> RequestContext {
    RequestContext::new("sqs", "us-east-1")
}

fn queue_url(name: &str) -> String {
    format!("http://sqs.us-east-1.localhost:4566/000000000000/{name}")
}

#[tokio::test]
async fn send_three_evicts_oldest_under_size_cap() {
    let dir = tmp_dir("evict");
    let queue_name = "qcap1";

    let svc = SqsService::with_data_dir(&dir).with_max_blob_bytes(1500);
    svc.handle("CreateQueue", json!({"QueueName": queue_name}), &ctx())
        .await
        .unwrap();

    let payload = "x".repeat(600);

    svc.handle(
        "SendMessage",
        json!({"QueueUrl": queue_url(queue_name), "MessageBody": payload}),
        &ctx(),
    )
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(60)).await;

    svc.handle(
        "SendMessage",
        json!({"QueueUrl": queue_url(queue_name), "MessageBody": payload}),
        &ctx(),
    )
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(60)).await;

    let queue_dir = dir.join("sqs").join(queue_name);
    let files_before: Vec<_> = std::fs::read_dir(&queue_dir).unwrap().flatten().collect();
    assert_eq!(files_before.len(), 2);

    svc.handle(
        "SendMessage",
        json!({"QueueUrl": queue_url(queue_name), "MessageBody": payload}),
        &ctx(),
    )
    .await
    .unwrap();

    let files_after: Vec<_> = std::fs::read_dir(&queue_dir).unwrap().flatten().collect();
    assert!(
        files_after.len() < 3,
        "expected eviction; got {} files",
        files_after.len()
    );

    let bs = svc.body_store().expect("body store");
    assert!(bs.total_size().unwrap() <= 1500);

    let _ = std::fs::remove_dir_all(&dir);
}
