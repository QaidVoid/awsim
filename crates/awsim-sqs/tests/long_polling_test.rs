//! SQS long polling.
//!
//! `WaitTimeSeconds` and the queue's `ReceiveMessageWaitTimeSeconds` were
//! accepted and ignored, so `ReceiveMessage` always returned instantly.
//! That turns a consumer written against AWS long polling into a hot spin
//! loop, and makes a test that waits on a producer either flake or pass
//! for the wrong reason.

use std::sync::Arc;
use std::time::{Duration, Instant};

use awsim_core::{RequestContext, ServiceHandler};
use awsim_sqs::SqsService;
use serde_json::json;

fn ctx() -> RequestContext {
    RequestContext::new("sqs", "us-east-1")
}

async fn make_queue(svc: &SqsService, name: &str) -> String {
    let out = svc
        .handle("CreateQueue", json!({ "QueueName": name }), &ctx())
        .await
        .expect("CreateQueue");
    out["QueueUrl"].as_str().unwrap().to_string()
}

fn message_count(v: &serde_json::Value) -> usize {
    v["Messages"].as_array().map(|a| a.len()).unwrap_or(0)
}

/// An empty queue must actually block for the requested wait.
#[tokio::test]
async fn waits_on_an_empty_queue() {
    let svc = SqsService::new();
    let url = make_queue(&svc, "waits").await;

    let start = Instant::now();
    let out = svc
        .handle(
            "ReceiveMessage",
            json!({ "QueueUrl": url, "WaitTimeSeconds": 1 }),
            &ctx(),
        )
        .await
        .expect("ReceiveMessage");
    let elapsed = start.elapsed();

    assert_eq!(message_count(&out), 0);
    assert!(
        elapsed >= Duration::from_millis(900),
        "should have waited about 1s, returned after {elapsed:?}"
    );
}

/// The point of long polling: return as soon as something arrives, not
/// after the full wait.
#[tokio::test]
async fn returns_as_soon_as_a_message_arrives() {
    let svc = Arc::new(SqsService::new());
    let url = make_queue(&svc, "early").await;

    let producer = {
        let svc = Arc::clone(&svc);
        let url = url.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            svc.handle(
                "SendMessage",
                json!({ "QueueUrl": url, "MessageBody": "late" }),
                &ctx(),
            )
            .await
            .expect("SendMessage");
        })
    };

    let start = Instant::now();
    let out = svc
        .handle(
            "ReceiveMessage",
            json!({ "QueueUrl": url, "WaitTimeSeconds": 10 }),
            &ctx(),
        )
        .await
        .expect("ReceiveMessage");
    let elapsed = start.elapsed();
    producer.await.expect("producer");

    assert_eq!(message_count(&out), 1, "should have received the message");
    assert!(
        elapsed < Duration::from_secs(3),
        "should return when the message arrives, not after the full 10s; took {elapsed:?}"
    );
    assert!(
        elapsed >= Duration::from_millis(250),
        "should not have returned before the message was sent; took {elapsed:?}"
    );
}

/// A message already waiting must come back immediately.
#[tokio::test]
async fn returns_immediately_when_a_message_is_already_queued() {
    let svc = SqsService::new();
    let url = make_queue(&svc, "ready").await;
    svc.handle(
        "SendMessage",
        json!({ "QueueUrl": url, "MessageBody": "here" }),
        &ctx(),
    )
    .await
    .expect("SendMessage");

    let start = Instant::now();
    let out = svc
        .handle(
            "ReceiveMessage",
            json!({ "QueueUrl": url, "WaitTimeSeconds": 20 }),
            &ctx(),
        )
        .await
        .expect("ReceiveMessage");

    assert_eq!(message_count(&out), 1);
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "an already-queued message should not wait"
    );
}

/// Short polling stays the default, so existing callers are unaffected.
#[tokio::test]
async fn short_polls_by_default() {
    let svc = SqsService::new();
    let url = make_queue(&svc, "short").await;

    let start = Instant::now();
    let out = svc
        .handle("ReceiveMessage", json!({ "QueueUrl": url }), &ctx())
        .await
        .expect("ReceiveMessage");

    assert_eq!(message_count(&out), 0);
    assert!(
        start.elapsed() < Duration::from_millis(500),
        "no wait configured should return at once"
    );
}

/// A queue configured for long polling applies it without the caller
/// passing WaitTimeSeconds on every request.
#[tokio::test]
async fn queue_attribute_supplies_the_default_wait() {
    let svc = SqsService::new();
    let url = make_queue(&svc, "qattr").await;
    svc.handle(
        "SetQueueAttributes",
        json!({
            "QueueUrl": url,
            "Attributes": { "ReceiveMessageWaitTimeSeconds": "1" },
        }),
        &ctx(),
    )
    .await
    .expect("SetQueueAttributes");

    let start = Instant::now();
    svc.handle("ReceiveMessage", json!({ "QueueUrl": url }), &ctx())
        .await
        .expect("ReceiveMessage");

    assert!(
        start.elapsed() >= Duration::from_millis(900),
        "queue-level wait should apply, returned after {:?}",
        start.elapsed()
    );
}

/// An explicit WaitTimeSeconds beats the queue attribute, including
/// choosing to short-poll a long-polling queue.
#[tokio::test]
async fn request_wait_overrides_the_queue_attribute() {
    let svc = SqsService::new();
    let url = make_queue(&svc, "override").await;
    svc.handle(
        "SetQueueAttributes",
        json!({
            "QueueUrl": url,
            "Attributes": { "ReceiveMessageWaitTimeSeconds": "20" },
        }),
        &ctx(),
    )
    .await
    .expect("SetQueueAttributes");

    let start = Instant::now();
    svc.handle(
        "ReceiveMessage",
        json!({ "QueueUrl": url, "WaitTimeSeconds": 0 }),
        &ctx(),
    )
    .await
    .expect("ReceiveMessage");

    assert!(
        start.elapsed() < Duration::from_millis(500),
        "explicit 0 should short-poll despite the queue attribute"
    );
}

#[tokio::test]
async fn out_of_range_wait_is_rejected() {
    let svc = SqsService::new();
    let url = make_queue(&svc, "range").await;

    for bad in [-1, 21, 100] {
        let res = svc
            .handle(
                "ReceiveMessage",
                json!({ "QueueUrl": url, "WaitTimeSeconds": bad }),
                &ctx(),
            )
            .await;
        match res {
            Err(e) => assert_eq!(e.code, "InvalidParameterValue", "for {bad}: {e:?}"),
            Ok(v) => panic!("WaitTimeSeconds {bad} should be rejected, got {v:?}"),
        }
    }
}

/// A blocked long poll must not stall anything else. Polling holds no
/// lock across the sleep, but that is a property worth pinning: holding
/// a DashMap guard over an await is exactly how this would deadlock.
#[tokio::test]
async fn concurrent_long_polls_do_not_block_other_work() {
    let svc = Arc::new(SqsService::new());
    let url = make_queue(&svc, "concurrent").await;

    let mut waiters = Vec::new();
    for _ in 0..5 {
        let svc = Arc::clone(&svc);
        let url = url.clone();
        waiters.push(tokio::spawn(async move {
            svc.handle(
                "ReceiveMessage",
                json!({ "QueueUrl": url, "WaitTimeSeconds": 3 }),
                &ctx(),
            )
            .await
        }));
    }

    // Give the waiters time to be genuinely blocked.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Unrelated work must still complete promptly.
    let start = Instant::now();
    svc.handle("CreateQueue", json!({ "QueueName": "unrelated" }), &ctx())
        .await
        .expect("CreateQueue during long polls");
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "an unrelated operation stalled behind long polls, took {elapsed:?}"
    );

    // And a send wakes every waiter rather than leaving them stuck.
    for i in 0..5 {
        svc.handle(
            "SendMessage",
            json!({ "QueueUrl": url, "MessageBody": format!("m{i}") }),
            &ctx(),
        )
        .await
        .expect("SendMessage");
    }
    for w in waiters {
        w.await.expect("waiter task").expect("ReceiveMessage");
    }
}

/// FIFO queues cache a receive batch against `ReceiveRequestAttemptId`.
/// Long polling calls the inner receive repeatedly, so an empty batch
/// must not be memoised and replayed for the rest of the window.
#[tokio::test]
async fn fifo_replay_cache_does_not_pin_an_empty_batch() {
    let svc = Arc::new(SqsService::new());
    let out = svc
        .handle(
            "CreateQueue",
            json!({
                "QueueName": "replay.fifo",
                "Attributes": { "FifoQueue": "true", "ContentBasedDeduplication": "true" },
            }),
            &ctx(),
        )
        .await
        .expect("CreateQueue");
    let url = out["QueueUrl"].as_str().unwrap().to_string();

    let producer = {
        let svc = Arc::clone(&svc);
        let url = url.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            svc.handle(
                "SendMessage",
                json!({
                    "QueueUrl": url,
                    "MessageBody": "fifo-late",
                    "MessageGroupId": "g",
                }),
                &ctx(),
            )
            .await
            .expect("SendMessage");
        })
    };

    let received = svc
        .handle(
            "ReceiveMessage",
            json!({
                "QueueUrl": url,
                "WaitTimeSeconds": 10,
                "ReceiveRequestAttemptId": "attempt-1",
            }),
            &ctx(),
        )
        .await
        .expect("ReceiveMessage");
    producer.await.expect("producer");

    assert_eq!(
        message_count(&received),
        1,
        "an empty first poll must not be cached and replayed: {received:?}"
    );
}
