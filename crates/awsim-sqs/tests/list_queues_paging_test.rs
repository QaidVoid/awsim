//! ListQueues accepted `MaxResults` and `NextToken` and ignored both.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_sqs::SqsService;
use serde_json::{Value, json};

fn ctx() -> RequestContext {
    RequestContext::new("sqs", "us-east-1")
}

async fn service_with_queues() -> SqsService {
    let svc = SqsService::new();
    for name in ["alpha", "beta", "gamma", "delta", "other"] {
        svc.handle("CreateQueue", json!({ "QueueName": name }), &ctx())
            .await
            .expect("CreateQueue");
    }
    svc
}

fn urls(out: &Value) -> Vec<String> {
    out["QueueUrls"]
        .as_array()
        .unwrap_or_else(|| panic!("QueueUrls should be a list: {out}"))
        .iter()
        .map(|u| u.as_str().unwrap_or_default().to_string())
        .collect()
}

#[tokio::test]
async fn max_results_limits_the_page() {
    let svc = service_with_queues().await;
    let out = svc
        .handle("ListQueues", json!({ "MaxResults": 2 }), &ctx())
        .await
        .expect("ListQueues");
    assert_eq!(urls(&out).len(), 2);
    assert!(
        out["NextToken"].as_str().is_some(),
        "a truncated page needs a token: {out}"
    );
}

#[tokio::test]
async fn the_token_walks_to_the_next_page() {
    let svc = service_with_queues().await;
    let first = svc
        .handle("ListQueues", json!({ "MaxResults": 2 }), &ctx())
        .await
        .expect("ListQueues");
    let token = first["NextToken"].as_str().expect("token").to_string();

    let second = svc
        .handle(
            "ListQueues",
            json!({ "MaxResults": 2, "NextToken": token }),
            &ctx(),
        )
        .await
        .expect("ListQueues");
    let page1 = urls(&first);
    let page2 = urls(&second);
    assert_eq!(page2.len(), 2);
    assert!(
        page1.iter().all(|u| !page2.contains(u)),
        "the second page must not repeat the first: {page1:?} then {page2:?}"
    );
}

#[tokio::test]
async fn the_prefix_filter_still_applies() {
    let svc = service_with_queues().await;
    let out = svc
        .handle("ListQueues", json!({ "QueueNamePrefix": "g" }), &ctx())
        .await
        .expect("ListQueues");
    assert_eq!(urls(&out).len(), 1);
}

#[tokio::test]
async fn no_parameters_lists_every_queue() {
    let svc = service_with_queues().await;
    let out = svc
        .handle("ListQueues", json!({}), &ctx())
        .await
        .expect("ListQueues");
    assert_eq!(urls(&out).len(), 5);
    assert!(out.get("NextToken").is_none(), "{out}");
}
