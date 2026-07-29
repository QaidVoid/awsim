//! Queue URLs are handed to a client and used as the target for every
//! subsequent SendMessage / ReceiveMessage / DeleteMessage call, so they
//! must point at an address the client can reach, and must keep parsing
//! back to a queue name whatever authority they were issued under.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_sqs::SqsService;
use serde_json::json;

fn ctx_with_authority(authority: Option<&str>) -> RequestContext {
    let mut ctx = RequestContext::new("sqs", "us-east-1");
    ctx.endpoint_authority = authority.map(|s| s.to_string());
    ctx
}

async fn create_queue(svc: &SqsService, ctx: &RequestContext, name: &str) -> String {
    let out = svc
        .handle("CreateQueue", json!({ "QueueName": name }), ctx)
        .await
        .expect("CreateQueue failed");
    out["QueueUrl"].as_str().expect("no QueueUrl").to_string()
}

/// The returned URL must carry the port AWSim is actually reachable on.
#[tokio::test]
async fn queue_url_reflects_resolved_authority() {
    let svc = SqsService::new();
    let ctx = ctx_with_authority(Some("localhost:4599"));
    let url = create_queue(&svc, &ctx, "porttest").await;

    assert!(
        url.contains("4599"),
        "queue URL should carry the reachable port, got `{url}`"
    );
    assert!(
        !url.contains("4566"),
        "queue URL leaked the hardcoded default port, got `{url}`"
    );
    assert_eq!(
        url,
        "http://sqs.us-east-1.localhost:4599/000000000000/porttest"
    );
}

/// Testcontainers maps to a random host port, so this is the shape that
/// matters most for the module story.
#[tokio::test]
async fn queue_url_works_for_random_mapped_port() {
    let svc = SqsService::new();
    let ctx = ctx_with_authority(Some("localhost:32871"));
    let url = create_queue(&svc, &ctx, "mapped").await;
    assert!(url.contains("32871"), "got `{url}`");
}

/// Docker Compose: a sibling container reaches AWSim by service name, so
/// `localhost` in the returned URL would resolve to the caller itself.
#[tokio::test]
async fn queue_url_works_for_container_hostname() {
    let svc = SqsService::new();
    let ctx = ctx_with_authority(Some("awsim:4566"));
    let url = create_queue(&svc, &ctx, "composed").await;
    assert!(
        url.contains("awsim:4566"),
        "queue URL should address the container by name, got `{url}`"
    );
}

/// Unresolved contexts (unit tests, background tasks) keep the historical
/// endpoint so nothing regresses.
#[tokio::test]
async fn queue_url_falls_back_when_unresolved() {
    let svc = SqsService::new();
    let ctx = ctx_with_authority(None);
    let url = create_queue(&svc, &ctx, "fallback").await;
    assert_eq!(
        url,
        "http://sqs.us-east-1.localhost:4566/000000000000/fallback"
    );
}

/// A URL is only useful if it round-trips: the client sends it straight
/// back on the next call.
#[tokio::test]
async fn returned_url_round_trips_through_send_and_receive() {
    let svc = SqsService::new();
    let ctx = ctx_with_authority(Some("localhost:4599"));
    let url = create_queue(&svc, &ctx, "roundtrip").await;

    svc.handle(
        "SendMessage",
        json!({ "QueueUrl": url, "MessageBody": "hello" }),
        &ctx,
    )
    .await
    .expect("SendMessage against the returned URL failed");

    let recv = svc
        .handle("ReceiveMessage", json!({ "QueueUrl": url }), &ctx)
        .await
        .expect("ReceiveMessage against the returned URL failed");

    let messages = recv["Messages"].as_array().expect("no Messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["Body"], "hello");
}

/// Parsing is host-agnostic, so a URL issued before the reachable address
/// changed, or under the old hardcoded endpoint, still resolves.
#[tokio::test]
async fn queue_urls_from_any_authority_still_resolve() {
    let svc = SqsService::new();
    let ctx = ctx_with_authority(Some("localhost:4599"));
    create_queue(&svc, &ctx, "shared").await;

    for url in [
        "http://sqs.us-east-1.localhost:4566/000000000000/shared",
        "http://sqs.us-east-1.localhost:4599/000000000000/shared",
        "https://sqs.us-east-1.aws.example.com/000000000000/shared",
        "http://awsim:4566/000000000000/shared",
    ] {
        let res = svc
            .handle(
                "SendMessage",
                json!({ "QueueUrl": url, "MessageBody": "x" }),
                &ctx,
            )
            .await;
        assert!(res.is_ok(), "URL `{url}` should still resolve: {res:?}");
    }
}
