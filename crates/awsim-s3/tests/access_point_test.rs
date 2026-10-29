//! Objects reached through an Access Point, by ARN or alias, live in the
//! Access Point's bucket, and a deleted Access Point reaches nothing.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_s3::S3Service;
use serde_json::{Value, json};

const ARN: &str = "arn:aws:s3:us-east-1:000000000000:accesspoint/owner-1";

fn ctx() -> RequestContext {
    RequestContext::new("s3", "us-east-1")
}

async fn with_access_point(svc: &S3Service) -> String {
    svc.handle("CreateBucket", json!({ "Bucket": "data" }), &ctx())
        .await
        .expect("CreateBucket");
    let out = svc
        .handle(
            "CreateAccessPoint",
            json!({
                "Name": "owner-1",
                "AccountId": "000000000000",
                "CreateAccessPointRequest": { "Bucket": "data" },
            }),
            &ctx(),
        )
        .await
        .expect("CreateAccessPoint");
    assert_eq!(out["AccessPointArn"], ARN);
    out["Alias"].as_str().unwrap().to_string()
}

fn body(out: &Value) -> Vec<u8> {
    use base64::Engine;
    let raw = out["__raw_body"].as_str().expect("raw body");
    base64::engine::general_purpose::STANDARD
        .decode(raw)
        .unwrap()
}

#[tokio::test]
async fn test_objects_written_through_alias_are_read_through_arn_and_bucket() {
    let svc = S3Service::new();
    let alias = with_access_point(&svc).await;

    svc.handle(
        "PutObject",
        json!({ "Bucket": alias, "Key": "private/a.txt", "Body": "hello" }),
        &ctx(),
    )
    .await
    .expect("PutObject through alias");

    for bucket in [ARN, "data"] {
        let out = svc
            .handle(
                "GetObject",
                json!({ "Bucket": bucket, "Key": "private/a.txt" }),
                &ctx(),
            )
            .await
            .expect("GetObject");
        assert_eq!(body(&out), b"hello", "{bucket}");
    }

    let listed = svc
        .handle(
            "ListObjectsV2",
            json!({ "Bucket": ARN, "prefix": "private/" }),
            &ctx(),
        )
        .await
        .expect("ListObjectsV2 through ARN");
    assert_eq!(listed["KeyCount"], 1, "{listed}");
}

#[tokio::test]
async fn test_deleted_access_point_reaches_nothing() {
    let svc = S3Service::new();
    let alias = with_access_point(&svc).await;
    svc.handle(
        "DeleteAccessPoint",
        json!({ "Name": "owner-1", "AccountId": "000000000000" }),
        &ctx(),
    )
    .await
    .expect("DeleteAccessPoint");

    for bucket in [ARN.to_string(), alias] {
        let err = svc
            .handle("GetObject", json!({ "Bucket": bucket, "Key": "k" }), &ctx())
            .await
            .unwrap_err();
        assert_eq!(err.code, "NoSuchAccessPoint", "{bucket}");
    }
}

#[tokio::test]
async fn test_access_points_persist_with_s3_state() {
    let svc = S3Service::new();
    let alias = with_access_point(&svc).await;
    let snapshot = svc.snapshot().expect("snapshot");

    let restored = S3Service::new();
    restored.restore(&snapshot).expect("restore");
    let out = restored
        .handle(
            "GetAccessPoint",
            json!({ "Name": "owner-1", "AccountId": "000000000000" }),
            &ctx(),
        )
        .await
        .expect("GetAccessPoint after restore");
    assert_eq!(out["Alias"], alias);
    assert_eq!(out["Bucket"], "data");
}
