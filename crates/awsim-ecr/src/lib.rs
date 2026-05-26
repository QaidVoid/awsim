pub mod error;
mod handler;
mod operations;
pub mod routes;
mod state;

pub use handler::EcrService;
pub use routes::router;

#[cfg(test)]
mod tests {
    use awsim_core::RequestContext;
    use serde_json::json;

    use super::handler::EcrService;
    use awsim_core::ServiceHandler;

    fn ctx() -> RequestContext {
        RequestContext::new("ecr", "us-east-1")
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        fn noop_clone(_: *const ()) -> RawWaker {
            noop_raw_waker()
        }
        fn noop(_: *const ()) {}
        fn noop_raw_waker() -> RawWaker {
            static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(f);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => {}
            }
        }
    }

    #[test]
    fn test_create_repository() {
        let svc = EcrService::new();
        let ctx = ctx();
        let result = block_on(svc.handle(
            "CreateRepository",
            json!({ "repositoryName": "my-repo" }),
            &ctx,
        ))
        .unwrap();
        let arn = result["repository"]["repositoryArn"].as_str().unwrap();
        assert!(arn.contains("my-repo"), "arn={arn}");
        let uri = result["repository"]["repositoryUri"].as_str().unwrap();
        assert!(uri.ends_with("/my-repo"), "uri={uri}");
    }

    #[test]
    fn test_create_repository_persists_kms_encryption_config() {
        let svc = EcrService::new();
        let ctx = ctx();
        let result = block_on(svc.handle(
            "CreateRepository",
            json!({
                "repositoryName": "kms-repo",
                "encryptionConfiguration": {
                    "encryptionType": "KMS",
                    "kmsKey": "arn:aws:kms:us-east-1:000000000000:key/abc"
                }
            }),
            &ctx,
        ))
        .unwrap();
        let enc = &result["repository"]["encryptionConfiguration"];
        assert_eq!(enc["encryptionType"], "KMS");
        assert_eq!(enc["kmsKey"], "arn:aws:kms:us-east-1:000000000000:key/abc");
    }

    #[test]
    fn test_create_repository_rejects_kms_without_key() {
        let svc = EcrService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateRepository",
            json!({
                "repositoryName": "kms-bad",
                "encryptionConfiguration": { "encryptionType": "KMS" }
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn test_create_repository_rejects_aes256_with_kms_key() {
        let svc = EcrService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateRepository",
            json!({
                "repositoryName": "aes-with-key",
                "encryptionConfiguration": {
                    "encryptionType": "AES256",
                    "kmsKey": "arn:aws:kms:us-east-1:000000000000:key/abc"
                }
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn test_create_repository_duplicate() {
        let svc = EcrService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateRepository",
            json!({ "repositoryName": "dup-repo" }),
            &ctx,
        ))
        .unwrap();
        let err = block_on(svc.handle(
            "CreateRepository",
            json!({ "repositoryName": "dup-repo" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "RepositoryAlreadyExistsException");
    }

    #[test]
    fn test_describe_repositories_all() {
        let svc = EcrService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateRepository",
            json!({ "repositoryName": "repo-a" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "CreateRepository",
            json!({ "repositoryName": "repo-b" }),
            &ctx,
        ))
        .unwrap();
        let result = block_on(svc.handle("DescribeRepositories", json!({}), &ctx)).unwrap();
        assert_eq!(result["repositories"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_describe_repositories_by_name() {
        let svc = EcrService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateRepository",
            json!({ "repositoryName": "repo-x" }),
            &ctx,
        ))
        .unwrap();
        let result = block_on(svc.handle(
            "DescribeRepositories",
            json!({ "repositoryNames": ["repo-x"] }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["repositories"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_delete_repository_non_empty_without_force() {
        let svc = EcrService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateRepository",
            json!({ "repositoryName": "nonempty" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "PutImage",
            json!({
                "repositoryName": "nonempty",
                "imageManifest": r#"{"schemaVersion":2}"#,
                "imageTag": "latest"
            }),
            &ctx,
        ))
        .unwrap();
        let err = block_on(svc.handle(
            "DeleteRepository",
            json!({ "repositoryName": "nonempty", "force": false }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "RepositoryNotEmptyException");
    }

    #[test]
    fn test_delete_repository_force() {
        let svc = EcrService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateRepository",
            json!({ "repositoryName": "forcedel" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "PutImage",
            json!({
                "repositoryName": "forcedel",
                "imageManifest": r#"{"schemaVersion":2}"#,
                "imageTag": "latest"
            }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "DeleteRepository",
            json!({ "repositoryName": "forcedel", "force": true }),
            &ctx,
        ))
        .unwrap();
        let repos = block_on(svc.handle("DescribeRepositories", json!({}), &ctx)).unwrap();
        assert_eq!(repos["repositories"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_get_authorization_token() {
        let svc = EcrService::new();
        let ctx = ctx();
        let result = block_on(svc.handle("GetAuthorizationToken", json!({}), &ctx)).unwrap();
        let data = result["authorizationData"].as_array().unwrap();
        assert_eq!(data.len(), 1);
        assert!(data[0]["authorizationToken"].as_str().is_some());
        assert!(
            data[0]["proxyEndpoint"]
                .as_str()
                .unwrap()
                .contains("us-east-1")
        );
    }

    #[test]
    fn test_put_and_list_images() {
        let svc = EcrService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateRepository",
            json!({ "repositoryName": "img-repo" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "PutImage",
            json!({
                "repositoryName": "img-repo",
                "imageManifest": r#"{"schemaVersion":2,"tag":"v1"}"#,
                "imageTag": "v1"
            }),
            &ctx,
        ))
        .unwrap();
        let list =
            block_on(svc.handle("ListImages", json!({ "repositoryName": "img-repo" }), &ctx))
                .unwrap();
        assert_eq!(list["imageIds"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_batch_get_image() {
        let svc = EcrService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateRepository",
            json!({ "repositoryName": "get-repo" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "PutImage",
            json!({
                "repositoryName": "get-repo",
                "imageManifest": r#"{"schemaVersion":2}"#,
                "imageTag": "stable"
            }),
            &ctx,
        ))
        .unwrap();
        let result = block_on(svc.handle(
            "BatchGetImage",
            json!({
                "repositoryName": "get-repo",
                "imageIds": [{ "imageTag": "stable" }]
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["images"].as_array().unwrap().len(), 1);
        assert_eq!(result["failures"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_batch_delete_image() {
        let svc = EcrService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateRepository",
            json!({ "repositoryName": "del-img-repo" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "PutImage",
            json!({
                "repositoryName": "del-img-repo",
                "imageManifest": r#"{"schemaVersion":2}"#,
                "imageTag": "old"
            }),
            &ctx,
        ))
        .unwrap();
        let result = block_on(svc.handle(
            "BatchDeleteImage",
            json!({
                "repositoryName": "del-img-repo",
                "imageIds": [{ "imageTag": "old" }]
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["imageIds"].as_array().unwrap().len(), 1);
        assert_eq!(result["failures"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_tags() {
        let svc = EcrService::new();
        let ctx = ctx();
        let created = block_on(svc.handle(
            "CreateRepository",
            json!({ "repositoryName": "tagged-repo" }),
            &ctx,
        ))
        .unwrap();
        let arn = created["repository"]["repositoryArn"]
            .as_str()
            .unwrap()
            .to_string();

        block_on(svc.handle(
            "TagResource",
            json!({ "resourceArn": arn, "tags": [{ "Key": "env", "Value": "prod" }] }),
            &ctx,
        ))
        .unwrap();

        let tags = block_on(svc.handle("ListTagsForResource", json!({ "resourceArn": arn }), &ctx))
            .unwrap();
        assert_eq!(tags["tags"].as_array().unwrap().len(), 1);

        block_on(svc.handle(
            "UntagResource",
            json!({ "resourceArn": arn, "tagKeys": ["env"] }),
            &ctx,
        ))
        .unwrap();

        let tags2 =
            block_on(svc.handle("ListTagsForResource", json!({ "resourceArn": arn }), &ctx))
                .unwrap();
        assert_eq!(tags2["tags"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_unknown_operation() {
        let svc = EcrService::new();
        let ctx = ctx();
        let err = block_on(svc.handle("NonExistentOp", json!({}), &ctx)).unwrap_err();
        assert_eq!(err.code, "UnknownOperationException");
    }
}
