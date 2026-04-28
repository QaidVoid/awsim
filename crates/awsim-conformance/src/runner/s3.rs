use crate::chk;
use crate::runner::common::*;

pub async fn test_s3(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_s3::Client::new(&config);
    let mut results = Vec::new();

    // CreateBucket
    results.push(chk!(
        "CreateBucket",
        client
            .create_bucket()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // HeadBucket
    results.push(chk!(
        "HeadBucket",
        client
            .head_bucket()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // ListBuckets
    results.push(chk!(
        "ListBuckets",
        client.list_buckets().send().await,
        verbose
    ));

    // PutObject
    results.push(chk!(
        "PutObject",
        client
            .put_object()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .body(aws_sdk_s3::primitives::ByteStream::from_static(
                b"hello conformance"
            ))
            .send()
            .await,
        verbose
    ));

    // HeadObject
    results.push(chk!(
        "HeadObject",
        client
            .head_object()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .send()
            .await,
        verbose
    ));

    // GetObject
    results.push(chk!(
        "GetObject",
        client
            .get_object()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .send()
            .await,
        verbose
    ));

    // ListObjectsV2
    results.push(chk!(
        "ListObjectsV2",
        client
            .list_objects_v2()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // CopyObject
    results.push(chk!(
        "CopyObject",
        client
            .copy_object()
            .bucket("conformance-bucket")
            .key("test-copy.txt")
            .copy_source("conformance-bucket/test-object.txt")
            .send()
            .await,
        verbose
    ));

    // GetBucketLocation
    results.push(chk!(
        "GetBucketLocation",
        client
            .get_bucket_location()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketVersioning
    results.push(chk!(
        "PutBucketVersioning",
        client
            .put_bucket_versioning()
            .bucket("conformance-bucket")
            .versioning_configuration(
                aws_sdk_s3::types::VersioningConfiguration::builder()
                    .status(aws_sdk_s3::types::BucketVersioningStatus::Enabled)
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketVersioning
    results.push(chk!(
        "GetBucketVersioning",
        client
            .get_bucket_versioning()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketTagging
    results.push(chk!(
        "PutBucketTagging",
        client
            .put_bucket_tagging()
            .bucket("conformance-bucket")
            .tagging(
                aws_sdk_s3::types::Tagging::builder()
                    .tag_set(
                        aws_sdk_s3::types::Tag::builder()
                            .key("env")
                            .value("conformance")
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketTagging
    results.push(chk!(
        "GetBucketTagging",
        client
            .get_bucket_tagging()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteBucketTagging
    results.push(chk!(
        "DeleteBucketTagging",
        client
            .delete_bucket_tagging()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutObjectTagging
    results.push(chk!(
        "PutObjectTagging",
        client
            .put_object_tagging()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .tagging(
                aws_sdk_s3::types::Tagging::builder()
                    .tag_set(
                        aws_sdk_s3::types::Tag::builder()
                            .key("type")
                            .value("test")
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetObjectTagging
    results.push(chk!(
        "GetObjectTagging",
        client
            .get_object_tagging()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .send()
            .await,
        verbose
    ));

    // DeleteObjectTagging
    results.push(chk!(
        "DeleteObjectTagging",
        client
            .delete_object_tagging()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .send()
            .await,
        verbose
    ));

    // PutBucketPolicy
    let policy_doc = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"s3:GetObject","Resource":"arn:aws:s3:::conformance-bucket/*"}]}"#;
    results.push(chk!(
        "PutBucketPolicy",
        client
            .put_bucket_policy()
            .bucket("conformance-bucket")
            .policy(policy_doc)
            .send()
            .await,
        verbose
    ));

    // GetBucketPolicy
    results.push(chk!(
        "GetBucketPolicy",
        client
            .get_bucket_policy()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteBucketPolicy
    results.push(chk!(
        "DeleteBucketPolicy",
        client
            .delete_bucket_policy()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketCors
    results.push(chk!(
        "PutBucketCors",
        client
            .put_bucket_cors()
            .bucket("conformance-bucket")
            .cors_configuration(
                aws_sdk_s3::types::CorsConfiguration::builder()
                    .cors_rules(
                        aws_sdk_s3::types::CorsRule::builder()
                            .allowed_methods("GET")
                            .allowed_origins("*")
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketCors
    results.push(chk!(
        "GetBucketCors",
        client
            .get_bucket_cors()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteBucketCors
    results.push(chk!(
        "DeleteBucketCors",
        client
            .delete_bucket_cors()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // GetBucketAcl
    results.push(chk!(
        "GetBucketAcl",
        client
            .get_bucket_acl()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketAcl
    results.push(chk!(
        "PutBucketAcl",
        client
            .put_bucket_acl()
            .bucket("conformance-bucket")
            .acl(aws_sdk_s3::types::BucketCannedAcl::Private)
            .send()
            .await,
        verbose
    ));

    // GetObjectAcl
    results.push(chk!(
        "GetObjectAcl",
        client
            .get_object_acl()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .send()
            .await,
        verbose
    ));

    // PutBucketLifecycleConfiguration
    results.push(chk!(
        "PutBucketLifecycleConfiguration",
        client
            .put_bucket_lifecycle_configuration()
            .bucket("conformance-bucket")
            .lifecycle_configuration(
                aws_sdk_s3::types::BucketLifecycleConfiguration::builder()
                    .rules(
                        aws_sdk_s3::types::LifecycleRule::builder()
                            .id("conformance-rule")
                            .status(aws_sdk_s3::types::ExpirationStatus::Enabled)
                            .expiration(
                                aws_sdk_s3::types::LifecycleExpiration::builder()
                                    .days(30)
                                    .build(),
                            )
                            .filter(
                                aws_sdk_s3::types::LifecycleRuleFilter::builder()
                                    .prefix("logs/")
                                    .build()
                            )
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketLifecycleConfiguration
    results.push(chk!(
        "GetBucketLifecycleConfiguration",
        client
            .get_bucket_lifecycle_configuration()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteBucketLifecycleConfiguration
    results.push(chk!(
        "DeleteBucketLifecycleConfiguration",
        client
            .delete_bucket_lifecycle()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketEncryption
    results.push(chk!(
        "PutBucketEncryption",
        client
            .put_bucket_encryption()
            .bucket("conformance-bucket")
            .server_side_encryption_configuration(
                aws_sdk_s3::types::ServerSideEncryptionConfiguration::builder()
                    .rules(
                        aws_sdk_s3::types::ServerSideEncryptionRule::builder()
                            .apply_server_side_encryption_by_default(
                                aws_sdk_s3::types::ServerSideEncryptionByDefault::builder()
                                    .sse_algorithm(aws_sdk_s3::types::ServerSideEncryption::Aes256)
                                    .build()
                                    .unwrap(),
                            )
                            .build(),
                    )
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketEncryption
    results.push(chk!(
        "GetBucketEncryption",
        client
            .get_bucket_encryption()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteBucketEncryption
    results.push(chk!(
        "DeleteBucketEncryption",
        client
            .delete_bucket_encryption()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketLogging
    results.push(chk!(
        "PutBucketLogging",
        client
            .put_bucket_logging()
            .bucket("conformance-bucket")
            .bucket_logging_status(aws_sdk_s3::types::BucketLoggingStatus::builder().build(),)
            .send()
            .await,
        verbose
    ));

    // GetBucketLogging
    results.push(chk!(
        "GetBucketLogging",
        client
            .get_bucket_logging()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketWebsite
    results.push(chk!(
        "PutBucketWebsite",
        client
            .put_bucket_website()
            .bucket("conformance-bucket")
            .website_configuration(
                aws_sdk_s3::types::WebsiteConfiguration::builder()
                    .index_document(
                        aws_sdk_s3::types::IndexDocument::builder()
                            .suffix("index.html")
                            .build()
                            .unwrap(),
                    )
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketWebsite
    results.push(chk!(
        "GetBucketWebsite",
        client
            .get_bucket_website()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteBucketWebsite
    results.push(chk!(
        "DeleteBucketWebsite",
        client
            .delete_bucket_website()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketOwnershipControls
    results.push(chk!(
        "PutBucketOwnershipControls",
        client
            .put_bucket_ownership_controls()
            .bucket("conformance-bucket")
            .ownership_controls(
                aws_sdk_s3::types::OwnershipControls::builder()
                    .rules(
                        aws_sdk_s3::types::OwnershipControlsRule::builder()
                            .object_ownership(
                                aws_sdk_s3::types::ObjectOwnership::BucketOwnerEnforced
                            )
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketOwnershipControls
    results.push(chk!(
        "GetBucketOwnershipControls",
        client
            .get_bucket_ownership_controls()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteBucketOwnershipControls
    results.push(chk!(
        "DeleteBucketOwnershipControls",
        client
            .delete_bucket_ownership_controls()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutPublicAccessBlock
    results.push(chk!(
        "PutPublicAccessBlock",
        client
            .put_public_access_block()
            .bucket("conformance-bucket")
            .public_access_block_configuration(
                aws_sdk_s3::types::PublicAccessBlockConfiguration::builder()
                    .block_public_acls(true)
                    .block_public_policy(true)
                    .ignore_public_acls(true)
                    .restrict_public_buckets(true)
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetPublicAccessBlock
    results.push(chk!(
        "GetPublicAccessBlock",
        client
            .get_public_access_block()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeletePublicAccessBlock
    results.push(chk!(
        "DeletePublicAccessBlock",
        client
            .delete_public_access_block()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketRequestPayment
    results.push(chk!(
        "PutBucketRequestPayment",
        client
            .put_bucket_request_payment()
            .bucket("conformance-bucket")
            .request_payment_configuration(
                aws_sdk_s3::types::RequestPaymentConfiguration::builder()
                    .payer(aws_sdk_s3::types::Payer::BucketOwner)
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketRequestPayment
    results.push(chk!(
        "GetBucketRequestPayment",
        client
            .get_bucket_request_payment()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketAccelerateConfiguration
    results.push(chk!(
        "PutBucketAccelerateConfiguration",
        client
            .put_bucket_accelerate_configuration()
            .bucket("conformance-bucket")
            .accelerate_configuration(
                aws_sdk_s3::types::AccelerateConfiguration::builder()
                    .status(aws_sdk_s3::types::BucketAccelerateStatus::Enabled)
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketAccelerateConfiguration
    results.push(chk!(
        "GetBucketAccelerateConfiguration",
        client
            .get_bucket_accelerate_configuration()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketNotificationConfiguration
    results.push(chk!(
        "PutBucketNotificationConfiguration",
        client
            .put_bucket_notification_configuration()
            .bucket("conformance-bucket")
            .notification_configuration(
                aws_sdk_s3::types::NotificationConfiguration::builder().build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketNotificationConfiguration
    results.push(chk!(
        "GetBucketNotificationConfiguration",
        client
            .get_bucket_notification_configuration()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketReplication (requires versioning enabled — already enabled above)
    results.push(chk!(
        "PutBucketReplication",
        client
            .put_bucket_replication()
            .bucket("conformance-bucket")
            .replication_configuration(
                aws_sdk_s3::types::ReplicationConfiguration::builder()
                    .role("arn:aws:iam::000000000000:role/replication-role")
                    .rules(
                        aws_sdk_s3::types::ReplicationRule::builder()
                            .status(aws_sdk_s3::types::ReplicationRuleStatus::Enabled)
                            .destination(
                                aws_sdk_s3::types::Destination::builder()
                                    .bucket("arn:aws:s3:::conformance-bucket-dest")
                                    .build()
                                    .unwrap(),
                            )
                            .filter(
                                aws_sdk_s3::types::ReplicationRuleFilter::builder()
                                    .prefix("")
                                    .build()
                            )
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketReplication
    results.push(chk!(
        "GetBucketReplication",
        client
            .get_bucket_replication()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteBucketReplication
    results.push(chk!(
        "DeleteBucketReplication",
        client
            .delete_bucket_replication()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // Multipart upload
    let mpu_r = client
        .create_multipart_upload()
        .bucket("conformance-bucket")
        .key("multipart-object.txt")
        .send()
        .await;
    let upload_id = mpu_r.as_ref().ok().and_then(|r| r.upload_id.clone());
    results.push(chk!("CreateMultipartUpload", mpu_r, verbose));

    if let Some(ref uid) = upload_id {
        // UploadPart (minimum 5MB for real S3 but sim should accept smaller)
        let part_data = vec![b'x'; 5 * 1024 * 1024]; // 5MB
        let up_r = client
            .upload_part()
            .bucket("conformance-bucket")
            .key("multipart-object.txt")
            .upload_id(uid)
            .part_number(1)
            .body(aws_sdk_s3::primitives::ByteStream::from(part_data))
            .send()
            .await;
        let etag = up_r.as_ref().ok().and_then(|r| r.e_tag.clone());
        results.push(chk!("UploadPart", up_r, verbose));

        // ListParts
        results.push(chk!(
            "ListParts",
            client
                .list_parts()
                .bucket("conformance-bucket")
                .key("multipart-object.txt")
                .upload_id(uid)
                .send()
                .await,
            verbose
        ));

        // ListMultipartUploads
        results.push(chk!(
            "ListMultipartUploads",
            client
                .list_multipart_uploads()
                .bucket("conformance-bucket")
                .send()
                .await,
            verbose
        ));

        if let Some(et) = etag {
            // CompleteMultipartUpload
            results.push(chk!(
                "CompleteMultipartUpload",
                client
                    .complete_multipart_upload()
                    .bucket("conformance-bucket")
                    .key("multipart-object.txt")
                    .upload_id(uid)
                    .multipart_upload(
                        aws_sdk_s3::types::CompletedMultipartUpload::builder()
                            .parts(
                                aws_sdk_s3::types::CompletedPart::builder()
                                    .part_number(1)
                                    .e_tag(et)
                                    .build(),
                            )
                            .build(),
                    )
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("CompleteMultipartUpload".to_string()));
        }

        // AbortMultipartUpload (create a fresh one to abort)
        let abort_mpu = client
            .create_multipart_upload()
            .bucket("conformance-bucket")
            .key("abort-multipart.txt")
            .send()
            .await;
        if let Some(abort_uid) = abort_mpu.ok().and_then(|r| r.upload_id) {
            results.push(chk!(
                "AbortMultipartUpload",
                client
                    .abort_multipart_upload()
                    .bucket("conformance-bucket")
                    .key("abort-multipart.txt")
                    .upload_id(abort_uid)
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("AbortMultipartUpload".to_string()));
        }
    } else {
        for op in &[
            "UploadPart",
            "ListParts",
            "ListMultipartUploads",
            "CompleteMultipartUpload",
            "AbortMultipartUpload",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    // ListObjects (v1)
    results.push(chk!(
        "ListObjects",
        client
            .list_objects()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // ListObjectVersions
    results.push(chk!(
        "ListObjectVersions",
        client
            .list_object_versions()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // GetObjectAttributes
    results.push(chk!(
        "GetObjectAttributes",
        client
            .get_object_attributes()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .object_attributes(aws_sdk_s3::types::ObjectAttributes::Etag)
            .object_attributes(aws_sdk_s3::types::ObjectAttributes::ObjectSize)
            .send()
            .await,
        verbose
    ));

    // DeleteObject
    results.push(chk!(
        "DeleteObject",
        client
            .delete_object()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .send()
            .await,
        verbose
    ));

    // DeleteObjects (also clean up multipart-object.txt)
    results.push(chk!(
        "DeleteObjects",
        client
            .delete_objects()
            .bucket("conformance-bucket")
            .delete(
                aws_sdk_s3::types::Delete::builder()
                    .objects(
                        aws_sdk_s3::types::ObjectIdentifier::builder()
                            .key("test-copy.txt")
                            .build()
                            .unwrap(),
                    )
                    .objects(
                        aws_sdk_s3::types::ObjectIdentifier::builder()
                            .key("multipart-object.txt")
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // After enabling versioning earlier in this run, plain DeleteObject
    // calls leave delete markers behind that count as bucket contents and
    // would block DeleteBucket with BucketNotEmpty. Drain every remaining
    // version + delete marker before tearing the bucket down.
    if let Ok(versions) = client
        .list_object_versions()
        .bucket("conformance-bucket")
        .send()
        .await
    {
        for v in versions.versions() {
            if let (Some(key), Some(vid)) = (v.key(), v.version_id()) {
                let _ = client
                    .delete_object()
                    .bucket("conformance-bucket")
                    .key(key)
                    .version_id(vid)
                    .send()
                    .await;
            }
        }
        for dm in versions.delete_markers() {
            if let (Some(key), Some(vid)) = (dm.key(), dm.version_id()) {
                let _ = client
                    .delete_object()
                    .bucket("conformance-bucket")
                    .key(key)
                    .version_id(vid)
                    .send()
                    .await;
            }
        }
    }

    // DeleteBucket
    results.push(chk!(
        "DeleteBucket",
        client
            .delete_bucket()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    results
}
