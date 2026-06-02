pub mod error;
mod handler;
mod operations;
mod state;

use awsim_core::{AccountRegionStore, ParameterLookup};

pub use handler::SsmService;

/// Cross-service helper: implements [`ParameterLookup`] so other
/// crates can ask "does parameter `<name|arn>` exist in this account
/// and region?" without depending on awsim-ssm's internals. Mirrors
/// [`awsim_secretsmanager::SecretsManagerSecretLookup`].
pub struct SsmParameterLookup {
    store: AccountRegionStore<state::SsmState>,
}

impl SsmParameterLookup {
    pub fn new(store: AccountRegionStore<state::SsmState>) -> Self {
        Self { store }
    }
}

impl ParameterLookup for SsmParameterLookup {
    fn parameter_exists(&self, parameter_ref: &str, account: &str, region: &str) -> bool {
        let state = self.store.get(account, region);
        // Parameters are stored by their canonical name (which may
        // start with `/`). Accept both the plain name and the ARN
        // form `arn:aws:ssm:{region}:{account}:parameter/<name>`.
        if state.parameters.contains_key(parameter_ref) {
            return true;
        }
        if let Some(name) = parameter_ref.strip_prefix("arn:aws:ssm:") {
            // Skip past region:account: to the resource segment.
            let mut parts = name.splitn(3, ':');
            let _region = parts.next();
            let _account = parts.next();
            if let Some(resource) = parts.next()
                && let Some(name) = resource.strip_prefix("parameter")
            {
                // Resource is `parameter/<name>` or `parameter<name>` (when name starts with `/`).
                let name = name.strip_prefix('/').unwrap_or(name);
                let with_slash = format!("/{name}");
                if state.parameters.contains_key(name) || state.parameters.contains_key(&with_slash)
                {
                    return true;
                }
            }
        }
        false
    }
}

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

        let get =
            block_on(svc.handle("GetParameter", json!({ "Name": "/env/db/host" }), &ctx)).unwrap();
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
        let err =
            block_on(svc.handle("GetParameter", json!({ "Name": "/ghost" }), &ctx)).unwrap_err();
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

        for name in [
            "/app/prod/db/host",
            "/app/prod/db/port",
            "/app/prod/key",
            "/other/val",
        ] {
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
    fn test_put_parameter_allowed_pattern_matching() {
        let svc = SsmService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "PutParameter",
            json!({
                "Name": "/port",
                "Value": "8080",
                "Type": "String",
                "AllowedPattern": "^[0-9]{1,5}$",
            }),
            &ctx,
        ))
        .unwrap();
    }

    #[test]
    fn test_put_parameter_allowed_pattern_rejects_mismatch() {
        let svc = SsmService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "PutParameter",
            json!({
                "Name": "/port",
                "Value": "abc",
                "Type": "String",
                "AllowedPattern": "^[0-9]+$",
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("AllowedPattern"));
    }

    #[test]
    fn test_put_parameter_data_type_aws_ec2_image_requires_ami_value() {
        let svc = SsmService::new();
        let ctx = ctx();
        // Valid AMI id passes.
        block_on(svc.handle(
            "PutParameter",
            json!({
                "Name": "/ami/golden",
                "Value": "ami-0123456789abcdef0",
                "Type": "String",
                "DataType": "aws:ec2:image",
            }),
            &ctx,
        ))
        .unwrap();

        // Non-AMI Value is rejected.
        let err = block_on(svc.handle(
            "PutParameter",
            json!({
                "Name": "/ami/bad",
                "Value": "not-an-ami",
                "Type": "String",
                "DataType": "aws:ec2:image",
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("ami-"));
    }

    #[test]
    fn test_put_parameter_rejects_unknown_data_type() {
        let svc = SsmService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "PutParameter",
            json!({
                "Name": "/p",
                "Value": "v",
                "Type": "String",
                "DataType": "aws:rds:cluster",
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_put_parameter_allowed_pattern_rejects_bad_regex() {
        let svc = SsmService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "PutParameter",
            json!({
                "Name": "/oops",
                "Value": "anything",
                "Type": "String",
                "AllowedPattern": "(unclosed",
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("not a valid regular expression"));
    }

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
            block_on(svc.handle("GetParameterHistory", json!({ "Name": "/hist" }), &ctx)).unwrap();
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

    // -----------------------------------------------------------------------
    // Documents
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_get_delete_document() {
        let svc = SsmService::new();
        let ctx = ctx();

        block_on(svc.handle(
            "CreateDocument",
            json!({
                "Name": "MyDoc",
                "Content": "{\"schemaVersion\":\"2.2\"}",
                "DocumentType": "Command",
            }),
            &ctx,
        ))
        .unwrap();

        let got = block_on(svc.handle("GetDocument", json!({ "Name": "MyDoc" }), &ctx)).unwrap();
        assert_eq!(got["Name"].as_str().unwrap(), "MyDoc");
        assert_eq!(got["DocumentType"].as_str().unwrap(), "Command");

        let listed = block_on(svc.handle("ListDocuments", json!({}), &ctx)).unwrap();
        assert_eq!(listed["DocumentIdentifiers"].as_array().unwrap().len(), 1);

        block_on(svc.handle("DeleteDocument", json!({ "Name": "MyDoc" }), &ctx)).unwrap();

        let listed2 = block_on(svc.handle("ListDocuments", json!({}), &ctx)).unwrap();
        assert_eq!(listed2["DocumentIdentifiers"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_create_document_duplicate_fails() {
        let svc = SsmService::new();
        let ctx = ctx();

        block_on(svc.handle(
            "CreateDocument",
            json!({ "Name": "Dup", "Content": "{}", "DocumentType": "Command" }),
            &ctx,
        ))
        .unwrap();

        let err = block_on(svc.handle(
            "CreateDocument",
            json!({ "Name": "Dup", "Content": "{}", "DocumentType": "Command" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "DocumentAlreadyExists");
    }

    #[test]
    fn test_describe_and_update_document() {
        let svc = SsmService::new();
        let ctx = ctx();

        block_on(svc.handle(
            "CreateDocument",
            json!({ "Name": "UpdDoc", "Content": "v1", "DocumentType": "Automation" }),
            &ctx,
        ))
        .unwrap();

        let desc =
            block_on(svc.handle("DescribeDocument", json!({ "Name": "UpdDoc" }), &ctx)).unwrap();
        assert_eq!(desc["Document"]["DocumentVersion"].as_str().unwrap(), "1");

        block_on(svc.handle(
            "UpdateDocument",
            json!({ "Name": "UpdDoc", "Content": "v2" }),
            &ctx,
        ))
        .unwrap();

        let got = block_on(svc.handle("GetDocument", json!({ "Name": "UpdDoc" }), &ctx)).unwrap();
        assert_eq!(got["Content"].as_str().unwrap(), "v2");
        assert_eq!(got["DocumentVersion"].as_str().unwrap(), "2");
    }

    // -----------------------------------------------------------------------
    // Associations
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_describe_delete_association() {
        let svc = SsmService::new();
        let ctx = ctx();

        let created = block_on(svc.handle(
            "CreateAssociation",
            json!({
                "Name": "AWS-RunShellScript",
                "Targets": [{ "Key": "instanceids", "Values": ["i-12345678"] }]
            }),
            &ctx,
        ))
        .unwrap();

        let assoc_id = created["AssociationDescription"]["AssociationId"]
            .as_str()
            .unwrap();

        let described = block_on(svc.handle(
            "DescribeAssociation",
            json!({ "AssociationId": assoc_id }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(
            described["AssociationDescription"]["Name"]
                .as_str()
                .unwrap(),
            "AWS-RunShellScript"
        );

        let listed = block_on(svc.handle("ListAssociations", json!({}), &ctx)).unwrap();
        assert_eq!(listed["Associations"].as_array().unwrap().len(), 1);

        block_on(svc.handle(
            "DeleteAssociation",
            json!({ "AssociationId": assoc_id }),
            &ctx,
        ))
        .unwrap();

        let listed2 = block_on(svc.handle("ListAssociations", json!({}), &ctx)).unwrap();
        assert_eq!(listed2["Associations"].as_array().unwrap().len(), 0);
    }

    // -----------------------------------------------------------------------
    // Maintenance Windows
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_describe_delete_maintenance_window() {
        let svc = SsmService::new();
        let ctx = ctx();

        let created = block_on(svc.handle(
            "CreateMaintenanceWindow",
            json!({
                "Name": "MyWindow",
                "Schedule": "cron(0 2 ? * SUN *)",
                "Duration": 2,
                "Cutoff": 1,
                "AllowUnassociatedTargets": false
            }),
            &ctx,
        ))
        .unwrap();

        let window_id = created["WindowId"].as_str().unwrap();
        assert!(window_id.starts_with("mw-"));

        let windows = block_on(svc.handle("DescribeMaintenanceWindows", json!({}), &ctx)).unwrap();
        assert_eq!(windows["WindowIdentities"].as_array().unwrap().len(), 1);

        block_on(svc.handle(
            "DeleteMaintenanceWindow",
            json!({ "WindowId": window_id }),
            &ctx,
        ))
        .unwrap();

        let windows2 = block_on(svc.handle("DescribeMaintenanceWindows", json!({}), &ctx)).unwrap();
        assert_eq!(windows2["WindowIdentities"].as_array().unwrap().len(), 0);
    }

    // -----------------------------------------------------------------------
    // OpsCenter
    // -----------------------------------------------------------------------

    #[test]
    fn test_ops_item_lifecycle() {
        let svc = SsmService::new();
        let ctx = ctx();

        let created = block_on(svc.handle(
            "CreateOpsItem",
            json!({
                "Title": "DB connection failure",
                "Description": "Cannot reach prod DB",
                "Severity": "1",
            }),
            &ctx,
        ))
        .unwrap();

        let item_id = created["OpsItemId"].as_str().unwrap();
        assert!(item_id.starts_with("oi-"));

        let got =
            block_on(svc.handle("GetOpsItem", json!({ "OpsItemId": item_id }), &ctx)).unwrap();
        assert_eq!(
            got["OpsItem"]["Title"].as_str().unwrap(),
            "DB connection failure"
        );
        assert_eq!(got["OpsItem"]["Status"].as_str().unwrap(), "Open");

        block_on(svc.handle(
            "UpdateOpsItem",
            json!({ "OpsItemId": item_id, "Status": "Resolved" }),
            &ctx,
        ))
        .unwrap();

        let got2 =
            block_on(svc.handle("GetOpsItem", json!({ "OpsItemId": item_id }), &ctx)).unwrap();
        assert_eq!(got2["OpsItem"]["Status"].as_str().unwrap(), "Resolved");

        let items = block_on(svc.handle("DescribeOpsItems", json!({}), &ctx)).unwrap();
        assert_eq!(items["OpsItemSummaries"].as_array().unwrap().len(), 1);
    }

    // -----------------------------------------------------------------------
    // Run Command status transitions (tick-driven)
    // -----------------------------------------------------------------------

    #[test]
    fn test_send_command_status_walks_pending_to_success() {
        let svc = SsmService::new();
        let ctx = ctx();

        let sent = block_on(svc.handle(
            "SendCommand",
            json!({
                "DocumentName": "AWS-RunShellScript",
                "InstanceIds": ["i-0123456789abcdef0"],
            }),
            &ctx,
        ))
        .unwrap();
        let command_id = sent["Command"]["CommandId"].as_str().unwrap().to_string();
        assert_eq!(sent["Command"]["Status"].as_str().unwrap(), "Pending");

        // Freshly sent commands stay Pending through a tick.
        block_on(svc.tick());
        let inv = block_on(svc.handle(
            "GetCommandInvocation",
            json!({ "CommandId": command_id }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(inv["Status"].as_str().unwrap(), "Pending");
        // Resolved instance id flows through to the invocation view.
        assert_eq!(inv["InstanceId"].as_str().unwrap(), "i-0123456789abcdef0");

        let store = svc.store();
        let state = store.get(&ctx.account_id, &ctx.region);

        // Backdate to the InProgress threshold and tick.
        if let Some(mut c) = state.commands.get_mut(&command_id) {
            c.created_time = now_secs() - 1;
        }
        block_on(svc.tick());
        let inv = block_on(svc.handle(
            "GetCommandInvocation",
            json!({ "CommandId": command_id }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(inv["Status"].as_str().unwrap(), "InProgress");

        // Backdate past the Success threshold and tick.
        if let Some(mut c) = state.commands.get_mut(&command_id) {
            c.created_time = now_secs() - 2;
        }
        block_on(svc.tick());
        let inv = block_on(svc.handle(
            "GetCommandInvocation",
            json!({ "CommandId": command_id }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(inv["Status"].as_str().unwrap(), "Success");
        assert!(!inv["StandardOutputContent"].as_str().unwrap().is_empty());
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}
