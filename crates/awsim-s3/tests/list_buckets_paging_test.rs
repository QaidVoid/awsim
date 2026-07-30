//! ListBuckets accepted its query parameters and ignored all of them.
//!
//! `max-buckets`, `continuation-token` and `prefix` were parsed off the
//! request and never read, so a caller asking for one page got every
//! bucket back and no token to continue with.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_s3::S3Service;
use serde_json::{Value, json};

fn ctx() -> RequestContext {
    RequestContext::new("s3", "us-east-1")
}

async fn service_with_buckets() -> S3Service {
    let svc = S3Service::new();
    for name in ["alpha", "beta", "gamma", "delta", "other"] {
        svc.handle("CreateBucket", json!({ "Bucket": name }), &ctx())
            .await
            .expect("CreateBucket");
    }
    svc
}

fn names(out: &Value) -> Vec<String> {
    out["Buckets"]["Bucket"]
        .as_array()
        .unwrap_or_else(|| panic!("Buckets should nest Bucket: {out}"))
        .iter()
        .map(|b| b["Name"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[tokio::test]
async fn max_buckets_limits_the_page() {
    let svc = service_with_buckets().await;
    let out = svc
        .handle("ListBuckets", json!({ "max-buckets": "2" }), &ctx())
        .await
        .expect("ListBuckets");
    assert_eq!(names(&out), vec!["alpha", "beta"]);
    assert!(
        out["ContinuationToken"].as_str().is_some(),
        "a truncated page needs a token to continue with: {out}"
    );
}

#[tokio::test]
async fn the_token_walks_to_the_next_page() {
    let svc = service_with_buckets().await;
    let first = svc
        .handle("ListBuckets", json!({ "max-buckets": "2" }), &ctx())
        .await
        .expect("ListBuckets");
    let token = first["ContinuationToken"].as_str().expect("token");

    let second = svc
        .handle(
            "ListBuckets",
            json!({ "max-buckets": "2", "continuation-token": token }),
            &ctx(),
        )
        .await
        .expect("ListBuckets");
    assert_eq!(names(&second), vec!["delta", "gamma"]);
}

#[tokio::test]
async fn prefix_filters_the_listing() {
    let svc = service_with_buckets().await;
    let out = svc
        .handle("ListBuckets", json!({ "prefix": "g" }), &ctx())
        .await
        .expect("ListBuckets");
    assert_eq!(names(&out), vec!["gamma"]);
}

/// Omitting the parameters still lists everything, so the default path
/// is not regressed by adding paging.
#[tokio::test]
async fn no_parameters_lists_every_bucket() {
    let svc = service_with_buckets().await;
    let out = svc
        .handle("ListBuckets", json!({}), &ctx())
        .await
        .expect("ListBuckets");
    assert_eq!(names(&out).len(), 5);
    assert!(out.get("ContinuationToken").is_none(), "{out}");
}
