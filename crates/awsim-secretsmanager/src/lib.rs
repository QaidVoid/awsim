pub mod authz;
pub mod error;
mod operations;
pub mod state;
mod util;

pub use authz::{SecretsManagerResourcePolicyLookup, SecretsManagerSecretLookup};

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, LambdaInvoker, Protocol, RequestContext, ServiceHandler, arn,
};
use serde_json::Value;
use std::sync::Arc;
use tracing::debug;

use state::SecretsState;

/// The Secrets Manager service handler.
pub struct SecretsManagerService {
    store: AccountRegionStore<SecretsState>,
    lambda_invoker: Option<Arc<dyn LambdaInvoker>>,
}

impl SecretsManagerService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
            lambda_invoker: None,
        }
    }

    /// Attach a Lambda invoker so `RotateSecret` can dispatch the
    /// four-step rotation state machine against the customer's
    /// rotation Lambda. When absent, `RotateSecret` falls back to the
    /// in-process simulation (used by tests and bare deployments).
    pub fn with_lambda_invoker(mut self, invoker: Arc<dyn LambdaInvoker>) -> Self {
        self.lambda_invoker = Some(invoker);
        self
    }

    pub fn store(&self) -> AccountRegionStore<SecretsState> {
        self.store.clone()
    }

    pub fn lambda_invoker(&self) -> Option<&Arc<dyn LambdaInvoker>> {
        self.lambda_invoker.as_ref()
    }
}

impl Default for SecretsManagerService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for SecretsManagerService {
    fn service_name(&self) -> &str {
        "secretsmanager"
    }

    fn signing_name(&self) -> &str {
        "secretsmanager"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_1
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "SecretsManager request");
        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            "CreateSecret" => {
                operations::secrets::create_secret(&state, &input, ctx, Some(&self.store))
            }
            "GetSecretValue" => operations::secrets::get_secret_value(&state, &input, ctx),
            "PutSecretValue" => operations::secrets::put_secret_value(&state, &input, ctx),
            "DescribeSecret" => operations::secrets::describe_secret(&state, &input, ctx),
            "ListSecrets" => operations::secrets::list_secrets(&state, &input, ctx),
            "UpdateSecret" => operations::secrets::update_secret(&state, &input, ctx),
            "DeleteSecret" => operations::secrets::delete_secret(&state, &input, ctx),
            "RestoreSecret" => operations::secrets::restore_secret(&state, &input, ctx),
            "TagResource" => operations::secrets::tag_resource(&state, &input, ctx),
            "UntagResource" => operations::secrets::untag_resource(&state, &input, ctx),
            "RotateSecret" => operations::secrets::rotate_secret(
                &state,
                &input,
                ctx,
                self.lambda_invoker.as_deref(),
            ),
            "CancelRotateSecret" => operations::secrets::cancel_rotate_secret(&state, &input, ctx),
            "ValidateResourcePolicy" => {
                operations::secrets::validate_resource_policy(&state, &input, ctx)
            }
            "GetRandomPassword" => operations::secrets::get_random_password(&state, &input, ctx),
            "ReplicateSecretToRegions" => operations::secrets::replicate_secret_to_regions(
                &state,
                &input,
                ctx,
                Some(&self.store),
            ),
            "RemoveRegionsFromReplication" => operations::secrets::remove_regions_from_replication(
                &state,
                &input,
                ctx,
                Some(&self.store),
            ),
            "StopReplicationToReplica" => operations::secrets::stop_replication_to_replica(
                &state,
                &input,
                ctx,
                Some(&self.store),
            ),
            "ListSecretVersionIds" => {
                operations::secrets::list_secret_version_ids(&state, &input, ctx)
            }
            "BatchGetSecretValue" => {
                operations::secrets::batch_get_secret_value(&state, &input, ctx)
            }
            "UpdateSecretVersionStage" => {
                operations::secrets::update_secret_version_stage(&state, &input, ctx)
            }
            "PutResourcePolicy" => operations::secrets::put_resource_policy(&state, &input, ctx),
            "GetResourcePolicy" => operations::secrets::get_resource_policy(&state, &input, ctx),
            "DeleteResourcePolicy" => {
                operations::secrets::delete_resource_policy(&state, &input, ctx)
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn iam_action(&self, operation: &str) -> Option<String> {
        match operation {
            "CreateSecret"
            | "GetSecretValue"
            | "PutSecretValue"
            | "DescribeSecret"
            | "ListSecrets"
            | "UpdateSecret"
            | "DeleteSecret"
            | "RestoreSecret"
            | "TagResource"
            | "UntagResource"
            | "RotateSecret"
            | "CancelRotateSecret"
            | "ValidateResourcePolicy"
            | "GetRandomPassword"
            | "ReplicateSecretToRegions"
            | "RemoveRegionsFromReplication"
            | "StopReplicationToReplica"
            | "ListSecretVersionIds"
            | "BatchGetSecretValue"
            | "UpdateSecretVersionStage"
            | "PutResourcePolicy"
            | "GetResourcePolicy"
            | "DeleteResourcePolicy" => Some(format!("secretsmanager:{operation}")),
            _ => None,
        }
    }

    fn iam_resource(&self, operation: &str, input: &Value, ctx: &RequestContext) -> Option<String> {
        match operation {
            "ListSecrets" | "GetRandomPassword" | "BatchGetSecretValue" | "CreateSecret" => {
                Some("*".to_string())
            }
            _ => {
                let secret_id = input.get("SecretId").and_then(|v| v.as_str())?;
                if secret_id.starts_with("arn:") {
                    Some(secret_id.to_string())
                } else {
                    Some(arn::build(
                        ctx,
                        "secretsmanager",
                        format!("secret:{secret_id}"),
                    ))
                }
            }
        }
    }

    /// Fire automatic rotation for any secret whose `NextRotationDate`
    /// has come due. AWS rotates on a schedule set via
    /// `RotateSecret` (`AutomaticallyAfterDays` or a `ScheduleExpression`);
    /// we mirror that by scanning every account/region each tick and
    /// running the rotation state machine for due secrets. Absolute-time
    /// gated and idempotent: each rotation advances `NextRotationDate`,
    /// so a missed or repeated tick never rotates twice for one deadline.
    async fn tick(&self) {
        let invoker = self.lambda_invoker.as_deref();
        for ((account_id, region), state) in self.store.iter_all() {
            operations::secrets::run_due_rotations(&state, &account_id, &region, invoker);
        }
    }
}

#[cfg(test)]
mod tests {
    use awsim_core::{RequestContext, ServiceHandler};
    use serde_json::json;

    use super::SecretsManagerService;

    fn ctx() -> RequestContext {
        RequestContext::new("secretsmanager", "us-east-1")
    }

    fn ctx_region(region: &str) -> RequestContext {
        RequestContext::new("secretsmanager", region)
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
    fn test_create_secret_basic() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        let result = block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "my-secret", "SecretString": "s3cr3t" }),
            &ctx,
        ))
        .unwrap();
        assert!(result["ARN"].as_str().unwrap().contains("my-secret"));
        assert_eq!(result["Name"].as_str().unwrap(), "my-secret");
        assert!(result["VersionId"].as_str().is_some());
    }

    #[test]
    fn test_create_secret_duplicate() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "dup", "SecretString": "val" }),
            &ctx,
        ))
        .unwrap();
        let err = block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "dup", "SecretString": "val2" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ResourceExistsException");
    }

    #[test]
    fn test_create_secret_rejects_reserved_aws_prefix() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "aws/managed", "SecretString": "hi" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidRequestException");
    }

    #[test]
    fn test_create_secret_rejects_invalid_chars() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "bad name with spaces", "SecretString": "hi" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn test_create_secret_no_value() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        let err =
            block_on(svc.handle("CreateSecret", json!({ "Name": "empty" }), &ctx)).unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn test_get_secret_value() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "my-secret", "SecretString": "hello" }),
            &ctx,
        ))
        .unwrap();
        let result =
            block_on(svc.handle("GetSecretValue", json!({ "SecretId": "my-secret" }), &ctx))
                .unwrap();
        assert_eq!(result["SecretString"].as_str().unwrap(), "hello");
        assert_eq!(result["Name"].as_str().unwrap(), "my-secret");
    }

    #[test]
    fn test_get_secret_by_arn() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        let created = block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "arn-secret", "SecretString": "data" }),
            &ctx,
        ))
        .unwrap();
        let arn = created["ARN"].as_str().unwrap();
        let result =
            block_on(svc.handle("GetSecretValue", json!({ "SecretId": arn }), &ctx)).unwrap();
        assert_eq!(result["SecretString"].as_str().unwrap(), "data");
    }

    #[test]
    fn test_get_secret_not_found() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        let err = block_on(svc.handle("GetSecretValue", json!({ "SecretId": "ghost" }), &ctx))
            .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn test_put_secret_value_rotates_stages() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "rotate-secret", "SecretString": "v1" }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "PutSecretValue",
            json!({ "SecretId": "rotate-secret", "SecretString": "v2" }),
            &ctx,
        ))
        .unwrap();

        // AWSCURRENT should return v2
        let current = block_on(svc.handle(
            "GetSecretValue",
            json!({ "SecretId": "rotate-secret", "VersionStage": "AWSCURRENT" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(current["SecretString"].as_str().unwrap(), "v2");

        // AWSPREVIOUS should return v1
        let prev = block_on(svc.handle(
            "GetSecretValue",
            json!({ "SecretId": "rotate-secret", "VersionStage": "AWSPREVIOUS" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(prev["SecretString"].as_str().unwrap(), "v1");
    }

    #[test]
    fn test_describe_secret() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "desc-secret", "SecretString": "x", "Description": "my desc" }),
            &ctx,
        ))
        .unwrap();
        let result =
            block_on(svc.handle("DescribeSecret", json!({ "SecretId": "desc-secret" }), &ctx))
                .unwrap();
        assert_eq!(result["Name"].as_str().unwrap(), "desc-secret");
        assert_eq!(result["Description"].as_str().unwrap(), "my desc");
        // Value must not be present in metadata
        assert!(result["SecretString"].is_null());
    }

    #[test]
    fn test_list_secrets() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "s1", "SecretString": "a" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "s2", "SecretString": "b" }),
            &ctx,
        ))
        .unwrap();
        let result = block_on(svc.handle("ListSecrets", json!({}), &ctx)).unwrap();
        assert_eq!(result["SecretList"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_update_secret_description() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "upd-secret", "SecretString": "val", "Description": "old" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "UpdateSecret",
            json!({ "SecretId": "upd-secret", "Description": "new" }),
            &ctx,
        ))
        .unwrap();
        let desc =
            block_on(svc.handle("DescribeSecret", json!({ "SecretId": "upd-secret" }), &ctx))
                .unwrap();
        assert_eq!(desc["Description"].as_str().unwrap(), "new");
    }

    #[test]
    fn test_delete_and_restore_secret() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "del-secret", "SecretString": "x" }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "DeleteSecret",
            json!({ "SecretId": "del-secret", "RecoveryWindowInDays": 7 }),
            &ctx,
        ))
        .unwrap();

        // GetSecretValue on a deleted secret should fail
        let err = block_on(svc.handle("GetSecretValue", json!({ "SecretId": "del-secret" }), &ctx))
            .unwrap_err();
        assert_eq!(err.code, "InvalidRequestException");

        // Restore it
        block_on(svc.handle("RestoreSecret", json!({ "SecretId": "del-secret" }), &ctx)).unwrap();

        // Should be accessible again
        let val = block_on(svc.handle("GetSecretValue", json!({ "SecretId": "del-secret" }), &ctx))
            .unwrap();
        assert_eq!(val["SecretString"].as_str().unwrap(), "x");
    }

    #[test]
    fn test_force_delete_secret() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "force-del", "SecretString": "gone" }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "DeleteSecret",
            json!({ "SecretId": "force-del", "ForceDeleteWithoutRecovery": true }),
            &ctx,
        ))
        .unwrap();

        // Immediately gone
        let err = block_on(svc.handle("GetSecretValue", json!({ "SecretId": "force-del" }), &ctx))
            .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn test_tag_and_untag_resource() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "tagged", "SecretString": "v" }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "TagResource",
            json!({
                "SecretId": "tagged",
                "Tags": [{ "Key": "env", "Value": "prod" }, { "Key": "team", "Value": "ops" }]
            }),
            &ctx,
        ))
        .unwrap();

        let desc =
            block_on(svc.handle("DescribeSecret", json!({ "SecretId": "tagged" }), &ctx)).unwrap();
        assert_eq!(desc["Tags"].as_array().unwrap().len(), 2);

        block_on(svc.handle(
            "UntagResource",
            json!({ "SecretId": "tagged", "TagKeys": ["env"] }),
            &ctx,
        ))
        .unwrap();

        let desc2 =
            block_on(svc.handle("DescribeSecret", json!({ "SecretId": "tagged" }), &ctx)).unwrap();
        assert_eq!(desc2["Tags"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_unknown_operation() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        let err = block_on(svc.handle("FooBar", json!({}), &ctx)).unwrap_err();
        assert_eq!(err.code, "UnknownOperationException");
    }

    #[test]
    fn test_list_secret_version_ids() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "versioned", "SecretString": "v1" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "PutSecretValue",
            json!({ "SecretId": "versioned", "SecretString": "v2" }),
            &ctx,
        ))
        .unwrap();

        let result = block_on(svc.handle(
            "ListSecretVersionIds",
            json!({ "SecretId": "versioned" }),
            &ctx,
        ))
        .unwrap();
        let versions = result["Versions"].as_array().unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(result["Name"].as_str().unwrap(), "versioned");
    }

    #[test]
    fn test_batch_get_secret_value() {
        let svc = SecretsManagerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "s1", "SecretString": "val1" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "s2", "SecretString": "val2" }),
            &ctx,
        ))
        .unwrap();

        let result = block_on(svc.handle(
            "BatchGetSecretValue",
            json!({ "SecretIdList": ["s1", "s2", "nonexistent"] }),
            &ctx,
        ))
        .unwrap();

        let values = result["SecretValues"].as_array().unwrap();
        let errors = result["Errors"].as_array().unwrap();
        assert_eq!(values.len(), 2);
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0]["ErrorCode"].as_str().unwrap(),
            "ResourceNotFoundException"
        );
    }

    #[test]
    fn test_create_secret_replica_readable_in_target_region() {
        let svc = SecretsManagerService::new();
        let east = ctx();
        let west = ctx_region("us-west-2");

        block_on(svc.handle(
            "CreateSecret",
            json!({
                "Name": "repl",
                "SecretString": "shared",
                "AddReplicaRegions": [{ "Region": "us-west-2" }],
            }),
            &east,
        ))
        .unwrap();

        // The replica record resolves in us-west-2 and yields the value.
        let val =
            block_on(svc.handle("GetSecretValue", json!({ "SecretId": "repl" }), &west)).unwrap();
        assert_eq!(val["SecretString"].as_str().unwrap(), "shared");
    }

    #[test]
    fn test_remove_regions_deletes_replica_record() {
        let svc = SecretsManagerService::new();
        let east = ctx();
        let west = ctx_region("us-west-2");

        block_on(svc.handle(
            "CreateSecret",
            json!({
                "Name": "repl2",
                "SecretString": "v",
                "AddReplicaRegions": [{ "Region": "us-west-2" }],
            }),
            &east,
        ))
        .unwrap();
        // Present before removal.
        block_on(svc.handle("GetSecretValue", json!({ "SecretId": "repl2" }), &west)).unwrap();

        block_on(svc.handle(
            "RemoveRegionsFromReplication",
            json!({ "SecretId": "repl2", "RemoveReplicaRegions": ["us-west-2"] }),
            &east,
        ))
        .unwrap();

        // The replica record is gone from us-west-2.
        let err = block_on(svc.handle("GetSecretValue", json!({ "SecretId": "repl2" }), &west))
            .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");

        // The primary no longer lists the replica.
        let desc =
            block_on(svc.handle("DescribeSecret", json!({ "SecretId": "repl2" }), &east)).unwrap();
        assert!(desc["ReplicationStatus"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_replicate_secret_to_regions_creates_replica() {
        let svc = SecretsManagerService::new();
        let east = ctx();
        let west = ctx_region("us-west-2");

        block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "lazy", "SecretString": "v" }),
            &east,
        ))
        .unwrap();

        let resp = block_on(svc.handle(
            "ReplicateSecretToRegions",
            json!({ "SecretId": "lazy", "AddReplicaRegions": [{ "Region": "us-west-2" }] }),
            &east,
        ))
        .unwrap();
        assert_eq!(resp["ReplicationStatus"].as_array().unwrap().len(), 1);

        let val =
            block_on(svc.handle("GetSecretValue", json!({ "SecretId": "lazy" }), &west)).unwrap();
        assert_eq!(val["SecretString"].as_str().unwrap(), "v");
    }

    #[test]
    fn test_tick_rotates_due_secret() {
        use crate::state::SecretsState;
        use awsim_core::AwsError;
        use serde_json::Value;
        use std::sync::Arc;

        struct NoopInvoker;
        impl awsim_core::LambdaInvoker for NoopInvoker {
            fn invoke(
                &self,
                _function_name: &str,
                _payload: &Value,
                _account: &str,
                _region: &str,
            ) -> Result<Value, AwsError> {
                Ok(json!({}))
            }
        }

        let svc = SecretsManagerService::new().with_lambda_invoker(Arc::new(NoopInvoker));
        let east = ctx();

        block_on(svc.handle(
            "CreateSecret",
            json!({ "Name": "auto", "SecretString": "v0" }),
            &east,
        ))
        .unwrap();
        block_on(svc.handle(
            "RotateSecret",
            json!({
                "SecretId": "auto",
                "RotationLambdaARN": "arn:aws:lambda:us-east-1:000000000000:function:rot",
                "RotationRules": { "AutomaticallyAfterDays": 1 }
            }),
            &east,
        ))
        .unwrap();

        // Force the deadline into the past.
        let state: Arc<SecretsState> = svc.store().get("000000000000", "us-east-1");
        let prev_rotated = {
            let mut s = state.secrets.get_mut("auto").unwrap();
            s.next_rotation_date = Some(crate::util::now_epoch_f64() - 1.0);
            s.last_rotated_date.unwrap()
        };

        block_on(svc.tick());

        let s = state.secrets.get("auto").unwrap();
        assert!(
            s.last_rotated_date.unwrap() >= prev_rotated,
            "tick should rotate the due secret"
        );
        assert!(
            s.next_rotation_date.unwrap() > crate::util::now_epoch_f64(),
            "tick should advance next_rotation_date"
        );
    }
}
