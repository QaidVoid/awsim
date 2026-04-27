use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{RequestContext, ServiceHandler};
use awsim_sqs::SqsService;
use serde_json::{Value, json};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn tmp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("awsim-sqs-persist-{label}-{nanos}-{n}"));
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
async fn send_then_restart_then_receive_round_trips_body() {
    let dir = tmp_dir("send-restart-receive");
    let body = "hello world";
    let queue_name = "qpersist1";

    let snapshot = {
        let svc = SqsService::with_data_dir(&dir);
        svc.handle("CreateQueue", json!({"QueueName": queue_name}), &ctx())
            .await
            .unwrap();
        svc.handle(
            "SendMessage",
            json!({"QueueUrl": queue_url(queue_name), "MessageBody": body}),
            &ctx(),
        )
        .await
        .unwrap();
        svc.snapshot().expect("snapshot bytes")
    };

    let entries: Vec<_> = std::fs::read_dir(dir.join("sqs").join(queue_name))
        .unwrap()
        .collect();
    assert_eq!(entries.len(), 1);

    let svc2 = SqsService::with_data_dir(&dir);
    svc2.restore(&snapshot).expect("restore");

    let received = svc2
        .handle(
            "ReceiveMessage",
            json!({"QueueUrl": queue_url(queue_name), "MaxNumberOfMessages": 1}),
            &ctx(),
        )
        .await
        .unwrap();

    let messages = received
        .get("Messages")
        .and_then(Value::as_array)
        .expect("Messages array");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].get("Body").and_then(Value::as_str), Some(body));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn delete_then_restart_drops_blob_and_message() {
    let dir = tmp_dir("delete-restart");
    let queue_name = "qpersist2";

    let svc = SqsService::with_data_dir(&dir);
    svc.handle("CreateQueue", json!({"QueueName": queue_name}), &ctx())
        .await
        .unwrap();
    svc.handle(
        "SendMessage",
        json!({"QueueUrl": queue_url(queue_name), "MessageBody": "to-delete"}),
        &ctx(),
    )
    .await
    .unwrap();

    let received = svc
        .handle(
            "ReceiveMessage",
            json!({"QueueUrl": queue_url(queue_name), "MaxNumberOfMessages": 1}),
            &ctx(),
        )
        .await
        .unwrap();
    let messages = received.get("Messages").and_then(Value::as_array).unwrap();
    assert_eq!(messages.len(), 1);
    let receipt = messages[0]
        .get("ReceiptHandle")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();
    let message_id = messages[0]
        .get("MessageId")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();
    let blob_path = dir.join("sqs").join(queue_name).join(&message_id);
    assert!(blob_path.exists());

    svc.handle(
        "DeleteMessage",
        json!({"QueueUrl": queue_url(queue_name), "ReceiptHandle": receipt}),
        &ctx(),
    )
    .await
    .unwrap();
    assert!(!blob_path.exists());

    let snapshot = svc.snapshot().expect("snapshot");

    let svc2 = SqsService::with_data_dir(&dir);
    svc2.restore(&snapshot).expect("restore");

    let received = svc2
        .handle(
            "ReceiveMessage",
            json!({"QueueUrl": queue_url(queue_name), "MaxNumberOfMessages": 10}),
            &ctx(),
        )
        .await
        .unwrap();
    let messages = received.get("Messages").and_then(Value::as_array).unwrap();
    assert!(messages.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}
