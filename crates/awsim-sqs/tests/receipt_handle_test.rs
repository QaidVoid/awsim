//! AWS rejects a receipt handle that does not correspond to a message
//! currently in flight. Returning success instead lets a consumer that
//! deletes with a stale or malformed handle pass its local tests and then
//! silently fail to delete against real SQS.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_sqs::SqsService;
use serde_json::json;

fn ctx() -> RequestContext {
    RequestContext::new("sqs", "us-east-1")
}

async fn queue_with_message(svc: &SqsService, name: &str) -> String {
    let out = svc
        .handle("CreateQueue", json!({ "QueueName": name }), &ctx())
        .await
        .expect("CreateQueue");
    let url = out["QueueUrl"].as_str().unwrap().to_string();
    svc.handle(
        "SendMessage",
        json!({ "QueueUrl": url, "MessageBody": "payload" }),
        &ctx(),
    )
    .await
    .expect("SendMessage");
    url
}

#[tokio::test]
async fn delete_with_malformed_handle_is_rejected() {
    let svc = SqsService::new();
    let url = queue_with_message(&svc, "malformed").await;

    let res = svc
        .handle(
            "DeleteMessage",
            json!({ "QueueUrl": url, "ReceiptHandle": "garbage" }),
            &ctx(),
        )
        .await;

    match res {
        Err(e) => assert_eq!(e.code, "ReceiptHandleIsInvalid", "wrong error: {e:?}"),
        Ok(v) => panic!("malformed receipt handle should be rejected, got {v:?}"),
    }
}

#[tokio::test]
async fn delete_with_unknown_but_wellformed_handle_is_rejected() {
    let svc = SqsService::new();
    let url = queue_with_message(&svc, "unknown").await;

    let res = svc
        .handle(
            "DeleteMessage",
            json!({
                "QueueUrl": url,
                "ReceiptHandle": "00000000-0000-0000-0000-000000000000",
            }),
            &ctx(),
        )
        .await;
    assert!(res.is_err(), "unknown handle should be rejected: {res:?}");
}

#[tokio::test]
async fn delete_with_a_real_handle_still_works() {
    let svc = SqsService::new();
    let url = queue_with_message(&svc, "valid").await;

    let recv = svc
        .handle("ReceiveMessage", json!({ "QueueUrl": url }), &ctx())
        .await
        .expect("ReceiveMessage");
    let handle = recv["Messages"][0]["ReceiptHandle"]
        .as_str()
        .expect("no ReceiptHandle")
        .to_string();

    svc.handle(
        "DeleteMessage",
        json!({ "QueueUrl": url, "ReceiptHandle": handle }),
        &ctx(),
    )
    .await
    .expect("delete with a handle from receive should succeed");

    // And the message must be gone rather than merely reported deleted.
    let again = svc
        .handle("ReceiveMessage", json!({ "QueueUrl": url }), &ctx())
        .await
        .expect("ReceiveMessage");
    let count = again["Messages"].as_array().map(|a| a.len()).unwrap_or(0);
    assert_eq!(count, 0, "message should be gone after delete");
}

#[tokio::test]
async fn deleting_the_same_handle_twice_is_rejected_the_second_time() {
    let svc = SqsService::new();
    let url = queue_with_message(&svc, "twice").await;

    let recv = svc
        .handle("ReceiveMessage", json!({ "QueueUrl": url }), &ctx())
        .await
        .expect("ReceiveMessage");
    let handle = recv["Messages"][0]["ReceiptHandle"]
        .as_str()
        .unwrap()
        .to_string();

    svc.handle(
        "DeleteMessage",
        json!({ "QueueUrl": url, "ReceiptHandle": handle }),
        &ctx(),
    )
    .await
    .expect("first delete");

    let res = svc
        .handle(
            "DeleteMessage",
            json!({ "QueueUrl": url, "ReceiptHandle": handle }),
            &ctx(),
        )
        .await;
    assert!(res.is_err(), "second delete should be rejected: {res:?}");
}

#[tokio::test]
async fn batch_delete_reports_invalid_entries_as_failed() {
    let svc = SqsService::new();
    let url = queue_with_message(&svc, "batch").await;

    let recv = svc
        .handle("ReceiveMessage", json!({ "QueueUrl": url }), &ctx())
        .await
        .expect("ReceiveMessage");
    let good = recv["Messages"][0]["ReceiptHandle"]
        .as_str()
        .unwrap()
        .to_string();

    let out = svc
        .handle(
            "DeleteMessageBatch",
            json!({
                "QueueUrl": url,
                "Entries": [
                    { "Id": "ok", "ReceiptHandle": good },
                    { "Id": "bad", "ReceiptHandle": "garbage" },
                ],
            }),
            &ctx(),
        )
        .await
        .expect("DeleteMessageBatch");

    let successful = out["Successful"].as_array().cloned().unwrap_or_default();
    let failed = out["Failed"].as_array().cloned().unwrap_or_default();

    assert_eq!(successful.len(), 1, "expected one success: {out:?}");
    assert_eq!(successful[0]["Id"], "ok");
    assert_eq!(failed.len(), 1, "expected one failure: {out:?}");
    assert_eq!(failed[0]["Id"], "bad");
    assert_eq!(failed[0]["Code"], "ReceiptHandleIsInvalid");
}
