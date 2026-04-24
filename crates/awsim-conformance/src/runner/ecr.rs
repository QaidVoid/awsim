use crate::chk;
use crate::runner::common::*;

pub async fn test_ecr(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_ecr::Client::new(&config);
    let mut results = Vec::new();

    // CreateRepository
    let create_r = client
        .create_repository()
        .repository_name("conformance-repo")
        .send()
        .await;
    results.push(chk!("CreateRepository", create_r, verbose));

    // DescribeRepositories
    results.push(chk!(
        "DescribeRepositories",
        client.describe_repositories().send().await,
        verbose
    ));

    // ListImages
    results.push(chk!(
        "ListImages",
        client
            .list_images()
            .repository_name("conformance-repo")
            .send()
            .await,
        verbose
    ));

    // DescribeImages
    results.push(chk!(
        "DescribeImages",
        client
            .describe_images()
            .repository_name("conformance-repo")
            .send()
            .await,
        verbose
    ));

    // GetAuthorizationToken
    results.push(chk!(
        "GetAuthorizationToken",
        client.get_authorization_token().send().await,
        verbose
    ));

    // TagResource (ECR)
    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource_arn("arn:aws:ecr:us-east-1:000000000000:repository/conformance-repo")
            .tags(
                aws_sdk_ecr::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTagsForResource (ECR)
    results.push(chk!(
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_arn("arn:aws:ecr:us-east-1:000000000000:repository/conformance-repo")
            .send()
            .await,
        verbose
    ));

    // UntagResource (ECR)
    results.push(chk!(
        "UntagResource",
        client
            .untag_resource()
            .resource_arn("arn:aws:ecr:us-east-1:000000000000:repository/conformance-repo")
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // PutImage (needs a manifest — use a minimal OCI manifest; may fail with schema error)
    let manifest = r#"{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{"mediaType":"application/vnd.docker.container.image.v1+json","size":7023,"digest":"sha256:b5b2b2c507a0944348e0303114d8d93aaaa081732b86451d9bce1f432a537bc7"},"layers":[]}"#;
    results.push(chk!(
        "PutImage",
        client
            .put_image()
            .repository_name("conformance-repo")
            .image_manifest(manifest)
            .image_tag("latest")
            .send()
            .await,
        verbose
    ));

    // BatchGetImage
    results.push(chk!(
        "BatchGetImage",
        client
            .batch_get_image()
            .repository_name("conformance-repo")
            .image_ids(
                aws_sdk_ecr::types::ImageIdentifier::builder()
                    .image_tag("latest")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // BatchDeleteImage
    results.push(chk!(
        "BatchDeleteImage",
        client
            .batch_delete_image()
            .repository_name("conformance-repo")
            .image_ids(
                aws_sdk_ecr::types::ImageIdentifier::builder()
                    .image_tag("latest")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // PutLifecyclePolicy
    results.push(chk!(
        "PutLifecyclePolicy",
        client
            .put_lifecycle_policy()
            .repository_name("conformance-repo")
            .lifecycle_policy_text(r#"{"rules":[{"rulePriority":1,"selection":{"tagStatus":"untagged","countType":"imageCountMoreThan","countNumber":5},"action":{"type":"expire"}}]}"#)
            .send()
            .await,
        verbose
    ));

    // GetLifecyclePolicy
    results.push(chk!(
        "GetLifecyclePolicy",
        client
            .get_lifecycle_policy()
            .repository_name("conformance-repo")
            .send()
            .await,
        verbose
    ));

    // DeleteLifecyclePolicy
    results.push(chk!(
        "DeleteLifecyclePolicy",
        client
            .delete_lifecycle_policy()
            .repository_name("conformance-repo")
            .send()
            .await,
        verbose
    ));

    // SetRepositoryPolicy
    results.push(chk!(
        "SetRepositoryPolicy",
        client
            .set_repository_policy()
            .repository_name("conformance-repo")
            .policy_text(r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":["ecr:GetDownloadUrlForLayer","ecr:BatchGetImage"]}]}"#)
            .send()
            .await,
        verbose
    ));

    // GetRepositoryPolicy
    results.push(chk!(
        "GetRepositoryPolicy",
        client
            .get_repository_policy()
            .repository_name("conformance-repo")
            .send()
            .await,
        verbose
    ));

    // DeleteRepositoryPolicy
    results.push(chk!(
        "DeleteRepositoryPolicy",
        client
            .delete_repository_policy()
            .repository_name("conformance-repo")
            .send()
            .await,
        verbose
    ));

    // StartImageScan
    results.push(chk!(
        "StartImageScan",
        client
            .start_image_scan()
            .repository_name("conformance-repo")
            .image_id(
                aws_sdk_ecr::types::ImageIdentifier::builder()
                    .image_tag("latest")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // DescribeImageScanFindings
    results.push(chk!(
        "DescribeImageScanFindings",
        client
            .describe_image_scan_findings()
            .repository_name("conformance-repo")
            .image_id(
                aws_sdk_ecr::types::ImageIdentifier::builder()
                    .image_tag("latest")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // BatchCheckLayerAvailability
    results.push(chk!(
        "BatchCheckLayerAvailability",
        client
            .batch_check_layer_availability()
            .repository_name("conformance-repo")
            .layer_digests("sha256:b5b2b2c507a0944348e0303114d8d93aaaa081732b86451d9bce1f432a537bc7")
            .send()
            .await,
        verbose
    ));

    // InitiateLayerUpload
    let layer_upload_r = client
        .initiate_layer_upload()
        .repository_name("conformance-repo")
        .send()
        .await;
    let layer_upload_id = layer_upload_r
        .as_ref()
        .ok()
        .and_then(|r| r.upload_id.clone());
    results.push(chk!("InitiateLayerUpload", layer_upload_r, verbose));

    if let Some(ref upload_id) = layer_upload_id {
        // UploadLayerPart
        results.push(chk!(
            "UploadLayerPart",
            client
                .upload_layer_part()
                .repository_name("conformance-repo")
                .upload_id(upload_id)
                .part_first_byte(0)
                .part_last_byte(3)
                .layer_part_blob(aws_sdk_ecr::primitives::Blob::new(b"test".to_vec()))
                .send()
                .await,
            verbose
        ));

        // CompleteLayerUpload
        results.push(chk!(
            "CompleteLayerUpload",
            client
                .complete_layer_upload()
                .repository_name("conformance-repo")
                .upload_id(upload_id)
                .layer_digests("sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08")
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("UploadLayerPart".to_string()));
        results.push(OpResult::Skipped("CompleteLayerUpload".to_string()));
    }

    // GetDownloadUrlForLayer
    results.push(chk!(
        "GetDownloadUrlForLayer",
        client
            .get_download_url_for_layer()
            .repository_name("conformance-repo")
            .layer_digest("sha256:b5b2b2c507a0944348e0303114d8d93aaaa081732b86451d9bce1f432a537bc7")
            .send()
            .await,
        verbose
    ));

    // PutImageTagMutability
    results.push(chk!(
        "PutImageTagMutability",
        client
            .put_image_tag_mutability()
            .repository_name("conformance-repo")
            .image_tag_mutability(aws_sdk_ecr::types::ImageTagMutability::Immutable)
            .send()
            .await,
        verbose
    ));

    // DescribeRegistry
    results.push(chk!(
        "DescribeRegistry",
        client.describe_registry().send().await,
        verbose
    ));

    // CreatePullThroughCacheRule
    results.push(chk!(
        "CreatePullThroughCacheRule",
        client
            .create_pull_through_cache_rule()
            .ecr_repository_prefix("dockerhub")
            .upstream_registry_url("registry-1.docker.io")
            .send()
            .await,
        verbose
    ));

    // DescribePullThroughCacheRules
    results.push(chk!(
        "DescribePullThroughCacheRules",
        client.describe_pull_through_cache_rules().send().await,
        verbose
    ));

    // DeleteRepository
    results.push(chk!(
        "DeleteRepository",
        client
            .delete_repository()
            .repository_name("conformance-repo")
            .send()
            .await,
        verbose
    ));

    results
}
