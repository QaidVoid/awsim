//! CreateBucket is region-conditional on AWS: re-creating a bucket you
//! already own succeeds in us-east-1 and raises BucketAlreadyOwnedByYou
//! everywhere else. Idempotent provisioning relies on this, and us-east-1
//! is AWSim's default region, so it is the common path.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_s3::S3Service;
use serde_json::json;

fn ctx(region: &str) -> RequestContext {
    RequestContext::new("s3", region)
}

#[tokio::test]
async fn recreate_in_us_east_1_succeeds() {
    let svc = S3Service::new();
    let c = ctx("us-east-1");

    svc.handle("CreateBucket", json!({ "Bucket": "idem" }), &c)
        .await
        .expect("first CreateBucket");

    let second = svc
        .handle("CreateBucket", json!({ "Bucket": "idem" }), &c)
        .await;
    assert!(
        second.is_ok(),
        "re-creating an owned bucket in us-east-1 should succeed, got {second:?}"
    );
}

#[tokio::test]
async fn recreate_outside_us_east_1_conflicts() {
    let svc = S3Service::new();
    let c = ctx("eu-west-1");

    svc.handle(
        "CreateBucket",
        json!({
            "Bucket": "regional",
            "CreateBucketConfiguration": { "LocationConstraint": "eu-west-1" },
        }),
        &c,
    )
    .await
    .expect("first CreateBucket");

    let second = svc
        .handle(
            "CreateBucket",
            json!({
                "Bucket": "regional",
                "CreateBucketConfiguration": { "LocationConstraint": "eu-west-1" },
            }),
            &c,
        )
        .await;

    match second {
        Err(e) => assert_eq!(e.code, "BucketAlreadyOwnedByYou", "wrong error: {e:?}"),
        Ok(v) => panic!("re-create outside us-east-1 should conflict, got {v:?}"),
    }
}

#[tokio::test]
async fn repeated_apply_is_idempotent_in_us_east_1() {
    // The shape an IaC apply produces: create the same bucket repeatedly
    // and expect every run to succeed.
    let svc = S3Service::new();
    let c = ctx("us-east-1");
    for attempt in 0..3 {
        let res = svc
            .handle("CreateBucket", json!({ "Bucket": "iac" }), &c)
            .await;
        assert!(res.is_ok(), "apply {attempt} failed: {res:?}");
    }
}

#[tokio::test]
async fn recreate_does_not_disturb_existing_objects() {
    let svc = S3Service::new();
    let c = ctx("us-east-1");

    svc.handle("CreateBucket", json!({ "Bucket": "keep" }), &c)
        .await
        .expect("CreateBucket");
    svc.handle(
        "PutObject",
        json!({ "Bucket": "keep", "Key": "k", "Body": "v" }),
        &c,
    )
    .await
    .expect("PutObject");

    svc.handle("CreateBucket", json!({ "Bucket": "keep" }), &c)
        .await
        .expect("re-create");

    let got = svc
        .handle("GetObject", json!({ "Bucket": "keep", "Key": "k" }), &c)
        .await;
    assert!(got.is_ok(), "object should survive a re-create: {got:?}");
}
