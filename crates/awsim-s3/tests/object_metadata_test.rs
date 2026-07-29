//! S3 object metadata must survive a round trip and come back on both
//! GetObject and HeadObject.
//!
//! `Content-Type` and the caching headers were never read off the
//! request, so every object came back as `application/octet-stream`: a
//! browser would not render an image or a stylesheet served from S3.
//! User metadata was stored but only surfaced on GET, because
//! `x-amz-meta-*` was written into the XML body, which a HEAD discards.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_s3::S3Service;
use serde_json::json;

fn ctx() -> RequestContext {
    RequestContext::new("s3", "us-east-1")
}

async fn put_with_metadata(svc: &S3Service, bucket: &str, key: &str) {
    svc.handle("CreateBucket", json!({ "Bucket": bucket }), &ctx())
        .await
        .expect("CreateBucket");
    svc.handle(
        "PutObject",
        json!({
            "Bucket": bucket,
            "Key": key,
            "Body": "body",
            "ContentType": "text/plain",
            "CacheControl": "max-age=99",
            "ContentDisposition": "attachment; filename=x.txt",
            "ContentLanguage": "en-GB",
            // The REST layer converts `x-amz-meta-owner` to `MetaOwner`
            // before the handler sees it, so mimic the wire shape.
            "MetaOwner": "alice",
        }),
        &ctx(),
    )
    .await
    .expect("PutObject");
}

#[tokio::test]
async fn get_object_returns_the_stored_content_type() {
    let svc = S3Service::new();
    put_with_metadata(&svc, "metaget", "o.txt").await;

    let out = svc
        .handle(
            "GetObject",
            json!({ "Bucket": "metaget", "Key": "o.txt" }),
            &ctx(),
        )
        .await
        .expect("GetObject");
    assert_eq!(
        out["ContentType"], "text/plain",
        "the supplied Content-Type was not stored: {out:?}"
    );
}

#[tokio::test]
async fn head_object_returns_the_same_metadata_as_get() {
    let svc = S3Service::new();
    put_with_metadata(&svc, "metahead", "o.txt").await;

    let head = svc
        .handle(
            "HeadObject",
            json!({ "Bucket": "metahead", "Key": "o.txt" }),
            &ctx(),
        )
        .await
        .expect("HeadObject");

    assert_eq!(head["ContentType"], "text/plain");
    assert_eq!(head["CacheControl"], "max-age=99");
    assert_eq!(head["ContentDisposition"], "attachment; filename=x.txt");
    assert_eq!(head["ContentLanguage"], "en-GB");
    assert_eq!(
        head["x-amz-meta-owner"], "alice",
        "user metadata must be present on HeadObject: {head:?}"
    );
}

#[tokio::test]
async fn copy_object_preserves_metadata() {
    let svc = S3Service::new();
    put_with_metadata(&svc, "metacopy", "src.txt").await;

    svc.handle(
        "CopyObject",
        json!({
            "Bucket": "metacopy",
            "Key": "dst.txt",
            "CopySource": "/metacopy/src.txt",
        }),
        &ctx(),
    )
    .await
    .expect("CopyObject");

    let head = svc
        .handle(
            "HeadObject",
            json!({ "Bucket": "metacopy", "Key": "dst.txt" }),
            &ctx(),
        )
        .await
        .expect("HeadObject");
    assert_eq!(
        head["ContentType"], "text/plain",
        "a copy should carry the source's Content-Type"
    );
}

/// AWS answers DeleteObject with 204, whether or not the key existed.
#[tokio::test]
async fn delete_object_returns_204() {
    let svc = S3Service::new();
    put_with_metadata(&svc, "deltest", "o.txt").await;

    for key in ["o.txt", "never-existed.txt"] {
        let out = svc
            .handle(
                "DeleteObject",
                json!({ "Bucket": "deltest", "Key": key }),
                &ctx(),
            )
            .await
            .expect("DeleteObject");
        assert_eq!(
            out["__status_code"], 204,
            "DeleteObject on `{key}` should answer 204"
        );
    }
}

/// Omitting Content-Type keeps the AWS default rather than erroring.
#[tokio::test]
async fn content_type_defaults_when_absent() {
    let svc = S3Service::new();
    svc.handle("CreateBucket", json!({ "Bucket": "defaultct" }), &ctx())
        .await
        .expect("CreateBucket");
    svc.handle(
        "PutObject",
        json!({ "Bucket": "defaultct", "Key": "o.bin", "Body": "x" }),
        &ctx(),
    )
    .await
    .expect("PutObject");

    let head = svc
        .handle(
            "HeadObject",
            json!({ "Bucket": "defaultct", "Key": "o.bin" }),
            &ctx(),
        )
        .await
        .expect("HeadObject");
    assert_eq!(head["ContentType"], "application/octet-stream");
}
