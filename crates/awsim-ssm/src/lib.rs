mod handler;
mod operations;
mod state;

pub use handler::SsmService;

#[cfg(test)]
mod tests {
    use awsim_core::RequestContext;
    use serde_json::json;

    use super::handler::SsmService;
    use awsim_core::ServiceHandler;

    fn ctx() -> RequestContext {
        RequestContext::new("ssm", "us-east-1")
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

    // -----------------------------------------------------------------------
    // PutParameter / GetParameter basics
    // -----------------------------------------------------------------------

    #[test]
    fn test_put_and_get_parameter() {
        let svc = SsmService::new();
        let ctx = ctx();

        let put = block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/env/db/host", "Value": "localhost", "Type": "String" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(put["Version"], 1);

        let get = block_on(svc.handle(
            "GetParameter",
            json!({ "Name": "/env/db/host" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(get["Parameter"]["Value"].as_str().unwrap(), "localhost");
        assert_eq!(get["Parameter"]["Version"], 1);
    }

    #[test]
    fn test_put_parameter_overwrite() {
        let svc = SsmService::new();
        let ctx = ctx();

        block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/x", "Value": "v1", "Type": "String" }),
            &ctx,
        ))
        .unwrap();

        let put2 = block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/x", "Value": "v2", "Type": "String", "Overwrite": true }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(put2["Version"], 2);

        let get = block_on(svc.handle("GetParameter", json!({ "Name": "/x" }), &ctx)).unwrap();
        assert_eq!(get["Parameter"]["Value"].as_str().unwrap(), "v2");
    }

    #[test]
    fn test_put_parameter_no_overwrite_conflicts() {
        let svc = SsmService::new();
        let ctx = ctx();

        block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/dup", "Value": "a", "Type": "String" }),
            &ctx,
        ))
        .unwrap();

        let err = block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/dup", "Value": "b", "Type": "String" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ParameterAlreadyExists");
    }

    #[test]
    fn test_put_parameter_invalid_type() {
        let svc = SsmService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/p", "Value": "v", "Type": "BadType" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterType");
    }

    #[test]
    fn test_get_parameter_not_found() {
        let svc = SsmService::new();
        let ctx = ctx();
        let err = block_on(svc.handle("GetParameter", json!({ "Name": "/ghost" }), &ctx))
            .unwrap_err();
        assert_eq!(err.code, "ParameterNotFound");
    }

    // -----------------------------------------------------------------------
    // GetParameters (batch)
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_parameters_mixed() {
        let svc = SsmService::new();
        let ctx = ctx();

        block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/a", "Value": "1", "Type": "String" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/b", "Value": "2", "Type": "String" }),
            &ctx,
        ))
        .unwrap();

        let result = block_on(svc.handle(
            "GetParameters",
            json!({ "Names": ["/a", "/b", "/missing"] }),
            &ctx,
        ))
        .unwrap();

        assert_eq!(result["Parameters"].as_array().unwrap().len(), 2);
        assert_eq!(result["InvalidParameters"].as_array().unwrap().len(), 1);
    }

    // -----------------------------------------------------------------------
    // GetParametersByPath
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_parameters_by_path_recursive() {
        let svc = SsmService::new();
        let ctx = ctx();

        for name in ["/app/prod/db/host", "/app/prod/db/port", "/app/prod/key", "/other/val"] {
            block_on(svc.handle(
                "PutParameter",
                json!({ "Name": name, "Value": "v", "Type": "String" }),
                &ctx,
            ))
            .unwrap();
        }

        let result = block_on(svc.handle(
            "GetParametersByPath",
            json!({ "Path": "/app/prod", "Recursive": true }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["Parameters"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_get_parameters_by_path_non_recursive() {
        let svc = SsmService::new();
        let ctx = ctx();

        for name in ["/root/child1", "/root/child2", "/root/nested/deep"] {
            block_on(svc.handle(
                "PutParameter",
                json!({ "Name": name, "Value": "v", "Type": "String" }),
                &ctx,
            ))
            .unwrap();
        }

        let result = block_on(svc.handle(
            "GetParametersByPath",
            json!({ "Path": "/root", "Recursive": false }),
            &ctx,
        ))
        .unwrap();
        // Only direct children: child1 and child2; nested/deep is excluded
        assert_eq!(result["Parameters"].as_array().unwrap().len(), 2);
    }

    // -----------------------------------------------------------------------
    // DeleteParameter / DeleteParameters
    // -----------------------------------------------------------------------

    #[test]
    fn test_delete_parameter() {
        let svc = SsmService::new();
        let ctx = ctx();

        block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/del", "Value": "x", "Type": "String" }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle("DeleteParameter", json!({ "Name": "/del" }), &ctx)).unwrap();

        let err =
            block_on(svc.handle("GetParameter", json!({ "Name": "/del" }), &ctx)).unwrap_err();
        assert_eq!(err.code, "ParameterNotFound");
    }

    #[test]
    fn test_delete_parameter_not_found() {
        let svc = SsmService::new();
        let ctx = ctx();
        let err =
            block_on(svc.handle("DeleteParameter", json!({ "Name": "/ghost" }), &ctx)).unwrap_err();
        assert_eq!(err.code, "ParameterNotFound");
    }

    #[test]
    fn test_delete_parameters_batch() {
        let svc = SsmService::new();
        let ctx = ctx();

        block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/p1", "Value": "v", "Type": "String" }),
            &ctx,
        ))
        .unwrap();

        let result = block_on(svc.handle(
            "DeleteParameters",
            json!({ "Names": ["/p1", "/missing"] }),
            &ctx,
        ))
        .unwrap();

        assert_eq!(result["DeletedParameters"].as_array().unwrap().len(), 1);
        assert_eq!(result["InvalidParameters"].as_array().unwrap().len(), 1);
    }

    // -----------------------------------------------------------------------
    // DescribeParameters
    // -----------------------------------------------------------------------

    #[test]
    fn test_describe_parameters() {
        let svc = SsmService::new();
        let ctx = ctx();

        block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/desc/a", "Value": "1", "Type": "String" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/desc/b", "Value": "2", "Type": "SecureString" }),
            &ctx,
        ))
        .unwrap();

        let result = block_on(svc.handle("DescribeParameters", json!({}), &ctx)).unwrap();
        assert_eq!(result["Parameters"].as_array().unwrap().len(), 2);
    }

    // -----------------------------------------------------------------------
    // GetParameterHistory
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_parameter_history() {
        let svc = SsmService::new();
        let ctx = ctx();

        block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/hist", "Value": "v1", "Type": "String" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/hist", "Value": "v2", "Type": "String", "Overwrite": true }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/hist", "Value": "v3", "Type": "String", "Overwrite": true }),
            &ctx,
        ))
        .unwrap();

        let result =
            block_on(svc.handle("GetParameterHistory", json!({ "Name": "/hist" }), &ctx))
                .unwrap();
        // 2 history entries + 1 current = 3
        assert_eq!(result["Parameters"].as_array().unwrap().len(), 3);
    }

    // -----------------------------------------------------------------------
    // Tags
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_and_list_tags() {
        let svc = SsmService::new();
        let ctx = ctx();

        block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/tagged", "Value": "v", "Type": "String" }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "AddTagsToResource",
            json!({
                "ResourceType": "Parameter",
                "ResourceId": "/tagged",
                "Tags": [{ "Key": "env", "Value": "prod" }],
            }),
            &ctx,
        ))
        .unwrap();

        let result = block_on(svc.handle(
            "ListTagsForResource",
            json!({ "ResourceType": "Parameter", "ResourceId": "/tagged" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["TagList"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_remove_tags() {
        let svc = SsmService::new();
        let ctx = ctx();

        block_on(svc.handle(
            "PutParameter",
            json!({ "Name": "/rtag", "Value": "v", "Type": "String" }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "AddTagsToResource",
            json!({
                "ResourceType": "Parameter",
                "ResourceId": "/rtag",
                "Tags": [{ "Key": "remove", "Value": "me" }],
            }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "RemoveTagsFromResource",
            json!({
                "ResourceType": "Parameter",
                "ResourceId": "/rtag",
                "TagKeys": ["remove"],
            }),
            &ctx,
        ))
        .unwrap();

        let result = block_on(svc.handle(
            "ListTagsForResource",
            json!({ "ResourceType": "Parameter", "ResourceId": "/rtag" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["TagList"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_unknown_operation() {
        let svc = SsmService::new();
        let ctx = ctx();
        let err = block_on(svc.handle("BogusOp", json!({}), &ctx)).unwrap_err();
        assert_eq!(err.code, "UnknownOperationException");
    }
}
