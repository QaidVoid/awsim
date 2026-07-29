//! Object Lock is a create-time property of a bucket.
//!
//! `PutObjectLockConfiguration` used to accept a retention rule on any
//! bucket, so a caller could write one, read it back, and believe locking
//! was on when nothing was protecting the objects.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_s3::S3Service;
use serde_json::json;

fn ctx() -> RequestContext {
    RequestContext::new("s3", "us-east-1")
}

async fn create(svc: &S3Service, bucket: &str, locked: bool) {
    let mut input = json!({ "Bucket": bucket });
    if locked {
        // The REST layer names the input after the header
        // `x-amz-bucket-object-lock-enabled`.
        input["BucketObjectLockEnabled"] = json!("true");
    }
    svc.handle("CreateBucket", input, &ctx())
        .await
        .expect("CreateBucket");
}

fn retention_rule(bucket: &str) -> serde_json::Value {
    json!({
        "Bucket": bucket,
        "ObjectLockConfiguration": {
            "ObjectLockEnabled": "Enabled",
            "Rule": { "DefaultRetention": { "Mode": "GOVERNANCE", "Days": 1 } },
        },
    })
}

#[tokio::test]
async fn creating_with_object_lock_enables_versioning() {
    let svc = S3Service::new();
    create(&svc, "locked", true).await;

    let out = svc
        .handle("GetBucketVersioning", json!({ "Bucket": "locked" }), &ctx())
        .await
        .expect("GetBucketVersioning");
    assert_eq!(
        out["VersioningConfiguration"]["Status"], "Enabled",
        "Object Lock implies versioning: {out:?}"
    );
}

#[tokio::test]
async fn object_lock_reports_enabled_before_any_rule_is_set() {
    let svc = S3Service::new();
    create(&svc, "lockedbare", true).await;

    let out = svc
        .handle(
            "GetObjectLockConfiguration",
            json!({ "Bucket": "lockedbare" }),
            &ctx(),
        )
        .await
        .expect("GetObjectLockConfiguration");
    assert_eq!(out["ObjectLockEnabled"], "Enabled");
}

#[tokio::test]
async fn retention_rule_is_rejected_on_an_unlocked_bucket() {
    let svc = S3Service::new();
    create(&svc, "plain", false).await;

    let err = svc
        .handle(
            "PutObjectLockConfiguration",
            retention_rule("plain"),
            &ctx(),
        )
        .await
        .expect_err("a bucket without Object Lock must refuse a retention rule");
    assert_eq!(err.code, "InvalidBucketState", "{err:?}");
}

#[tokio::test]
async fn retention_rule_is_accepted_on_a_locked_bucket() {
    let svc = S3Service::new();
    create(&svc, "lockedrule", true).await;

    svc.handle(
        "PutObjectLockConfiguration",
        retention_rule("lockedrule"),
        &ctx(),
    )
    .await
    .expect("PutObjectLockConfiguration");

    let out = svc
        .handle(
            "GetObjectLockConfiguration",
            json!({ "Bucket": "lockedrule" }),
            &ctx(),
        )
        .await
        .expect("GetObjectLockConfiguration");
    assert_eq!(out["Rule"]["DefaultRetention"]["Mode"], "GOVERNANCE");
}

#[tokio::test]
async fn versioning_cannot_be_suspended_while_object_lock_is_on() {
    let svc = S3Service::new();
    create(&svc, "nosuspend", true).await;

    let err = svc
        .handle(
            "PutBucketVersioning",
            json!({
                "Bucket": "nosuspend",
                "VersioningConfiguration": { "Status": "Suspended" },
            }),
            &ctx(),
        )
        .await
        .expect_err("suspending versioning would strand locked versions");
    assert_eq!(err.code, "InvalidBucketState", "{err:?}");
}

/// A plain bucket keeps the old behaviour: versioning is free to move.
#[tokio::test]
async fn versioning_still_suspends_on_an_unlocked_bucket() {
    let svc = S3Service::new();
    create(&svc, "freevers", false).await;

    for status in ["Enabled", "Suspended"] {
        svc.handle(
            "PutBucketVersioning",
            json!({
                "Bucket": "freevers",
                "VersioningConfiguration": { "Status": status },
            }),
            &ctx(),
        )
        .await
        .unwrap_or_else(|e| panic!("PutBucketVersioning {status}: {e:?}"));
    }
}
