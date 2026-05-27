//! Tag validation conformance.
//!
//! Every service that accepts tags routes through
//! `awsim_core::tags::validate(...)`, which enforces AWS's documented
//! limits: 50 tags per resource, 128-char keys, 256-char values, no
//! `aws:` prefix on writes, no duplicate keys. This test exercises a
//! representative SNS topic + SQS queue + Kinesis stream through the
//! 5 failure paths the spec calls out, ensuring the middleware
//! actually fires when wired from a real service handler.

use awsim_core::{RequestContext, ServiceHandler};
use serde_json::{Value, json};

fn ctx(service: &str) -> RequestContext {
    RequestContext::new(service, "us-east-1")
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop_clone(_: *const ()) -> RawWaker {
        noop_raw_waker()
    }
    fn noop(_: *const ()) {}
    fn noop_raw_waker() -> RawWaker {
        static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
    let mut cx = Context::from_waker(&waker);
    let mut fut = std::pin::pin!(f);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => {}
        }
    }
}

fn make_tags(count: usize) -> Value {
    let mut arr = Vec::with_capacity(count);
    for i in 0..count {
        arr.push(json!({ "Key": format!("k{i}"), "Value": format!("v{i}") }));
    }
    Value::Array(arr)
}

// ---------------------------------------------------------------------------
// SNS
// ---------------------------------------------------------------------------

fn seed_sns_topic() -> (awsim_sns::SnsService, String) {
    let svc = awsim_sns::SnsService::new();
    let c = ctx(svc.service_name());
    let created = block_on(svc.handle("CreateTopic", json!({ "Name": "tag-topic" }), &c)).unwrap();
    let arn = created["TopicArn"].as_str().unwrap().to_string();
    (svc, arn)
}

#[test]
fn sns_rejects_more_than_50_tags() {
    let (svc, arn) = seed_sns_topic();
    let err = block_on(svc.handle(
        "TagResource",
        json!({ "ResourceArn": arn, "Tags": make_tags(51) }),
        &ctx(svc.service_name()),
    ))
    .unwrap_err();
    assert!(
        err.code == "ValidationException"
            || err.code == "InvalidParameter"
            || err.code == "InvalidParameterValue"
            || err.code == "TooManyTagsException",
        "expected tag-cap exception, got {err:?}"
    );
}

#[test]
fn sns_rejects_oversize_key() {
    let (svc, arn) = seed_sns_topic();
    let oversize = "k".repeat(129);
    let err = block_on(svc.handle(
        "TagResource",
        json!({
            "ResourceArn": arn,
            "Tags": [{ "Key": oversize, "Value": "v" }],
        }),
        &ctx(svc.service_name()),
    ))
    .unwrap_err();
    assert!(
        err.code.contains("Validation") || err.code.contains("InvalidParameter"),
        "expected validation exception, got {err:?}"
    );
}

#[test]
fn sns_rejects_oversize_value() {
    let (svc, arn) = seed_sns_topic();
    let oversize = "v".repeat(257);
    let err = block_on(svc.handle(
        "TagResource",
        json!({
            "ResourceArn": arn,
            "Tags": [{ "Key": "k", "Value": oversize }],
        }),
        &ctx(svc.service_name()),
    ))
    .unwrap_err();
    assert!(
        err.code.contains("Validation") || err.code.contains("InvalidParameter"),
        "expected validation exception, got {err:?}"
    );
}

#[test]
fn sns_rejects_aws_prefixed_key() {
    let (svc, arn) = seed_sns_topic();
    let err = block_on(svc.handle(
        "TagResource",
        json!({
            "ResourceArn": arn,
            "Tags": [{ "Key": "aws:internal", "Value": "v" }],
        }),
        &ctx(svc.service_name()),
    ))
    .unwrap_err();
    assert!(
        err.code.contains("Validation") || err.code.contains("InvalidParameter"),
        "expected validation exception, got {err:?}"
    );
}

#[test]
fn sns_rejects_duplicate_keys() {
    let (svc, arn) = seed_sns_topic();
    let err = block_on(svc.handle(
        "TagResource",
        json!({
            "ResourceArn": arn,
            "Tags": [
                { "Key": "k", "Value": "a" },
                { "Key": "k", "Value": "b" },
            ],
        }),
        &ctx(svc.service_name()),
    ))
    .unwrap_err();
    assert!(
        err.code.contains("Validation") || err.code.contains("InvalidParameter"),
        "expected duplicate-key exception, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// SQS
// ---------------------------------------------------------------------------

fn seed_sqs_queue() -> (awsim_sqs::SqsService, String) {
    let svc = awsim_sqs::SqsService::new();
    let c = ctx(svc.service_name());
    let out = block_on(svc.handle("CreateQueue", json!({ "QueueName": "tag-q" }), &c)).unwrap();
    let url = out["QueueUrl"].as_str().unwrap().to_string();
    (svc, url)
}

#[test]
fn sqs_rejects_more_than_50_tags() {
    let (svc, url) = seed_sqs_queue();
    let mut tags = serde_json::Map::new();
    for i in 0..51 {
        tags.insert(format!("k{i}"), Value::String(format!("v{i}")));
    }
    let err = block_on(svc.handle(
        "TagQueue",
        json!({ "QueueUrl": url, "Tags": tags }),
        &ctx(svc.service_name()),
    ))
    .unwrap_err();
    assert!(
        err.code.contains("Validation") || err.code.contains("InvalidParameter"),
        "expected tag-cap exception, got {err:?}"
    );
}

#[test]
fn sqs_rejects_aws_prefixed_tag_key() {
    let (svc, url) = seed_sqs_queue();
    let err = block_on(svc.handle(
        "TagQueue",
        json!({
            "QueueUrl": url,
            "Tags": { "aws:reserved": "v" },
        }),
        &ctx(svc.service_name()),
    ))
    .unwrap_err();
    assert!(
        err.code.contains("Validation") || err.code.contains("InvalidParameter"),
        "expected validation exception, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Kinesis
// ---------------------------------------------------------------------------

#[test]
fn kinesis_rejects_oversize_tag_key_on_add_tags_to_stream() {
    let svc = awsim_kinesis::KinesisService::new();
    let c = ctx(svc.service_name());
    block_on(svc.handle(
        "CreateStream",
        json!({"StreamName": "tagged-stream", "ShardCount": 1}),
        &c,
    ))
    .unwrap();
    let oversize = "k".repeat(129);
    let mut tags = serde_json::Map::new();
    tags.insert(oversize, Value::String("v".into()));
    let err = block_on(svc.handle(
        "AddTagsToStream",
        json!({"StreamName": "tagged-stream", "Tags": tags}),
        &c,
    ))
    .unwrap_err();
    assert!(
        err.code.contains("Validation") || err.code.contains("InvalidParameter"),
        "expected validation exception, got {err:?}"
    );
}
