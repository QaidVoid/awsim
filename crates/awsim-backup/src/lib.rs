//! AWS Backup emulator: vaults, plans, selections, and a fast-forwarded job
//! lifecycle (jobs land in COMPLETED immediately so callers don't poll).

mod operations;
pub mod state;

pub use state::BackupState;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

pub struct BackupService {
    store: AccountRegionStore<BackupState>,
}

impl BackupService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<BackupState> {
        self.store.clone()
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<BackupState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for BackupService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for BackupService {
    fn service_name(&self) -> &str {
        "backup"
    }

    fn signing_name(&self) -> &str {
        "backup"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            // Vaults
            RouteDefinition {
                method: "PUT",
                path_pattern: "/backup-vaults/{BackupVaultName}",
                operation: "CreateBackupVault",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/backup-vaults/{BackupVaultName}",
                operation: "DescribeBackupVault",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/backup-vaults/{BackupVaultName}",
                operation: "DeleteBackupVault",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/backup-vaults",
                operation: "ListBackupVaults",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/backup-vaults/{BackupVaultName}/vault-lock",
                operation: "PutBackupVaultLockConfiguration",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/backup-vaults/{BackupVaultName}/vault-lock",
                operation: "DeleteBackupVaultLockConfiguration",
                required_query_param: None,
            },
            // Plans
            RouteDefinition {
                method: "PUT",
                path_pattern: "/backup/plans/",
                operation: "CreateBackupPlan",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/backup/plans/{BackupPlanId}/",
                operation: "GetBackupPlan",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/backup/plans",
                operation: "ListBackupPlans",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/backup/plans/{BackupPlanId}",
                operation: "DeleteBackupPlan",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/backup/plans/{BackupPlanId}",
                operation: "UpdateBackupPlan",
                required_query_param: None,
            },
            // Selections
            RouteDefinition {
                method: "PUT",
                path_pattern: "/backup/plans/{BackupPlanId}/selections/",
                operation: "CreateBackupSelection",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/backup/plans/{BackupPlanId}/selections/{SelectionId}",
                operation: "GetBackupSelection",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/backup/plans/{BackupPlanId}/selections",
                operation: "ListBackupSelections",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/backup/plans/{BackupPlanId}/selections/{SelectionId}",
                operation: "DeleteBackupSelection",
                required_query_param: None,
            },
            // Jobs
            RouteDefinition {
                method: "PUT",
                path_pattern: "/backup-jobs",
                operation: "StartBackupJob",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/backup-jobs/{BackupJobId}",
                operation: "DescribeBackupJob",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/backup-jobs",
                operation: "ListBackupJobs",
                required_query_param: None,
            },
        ]
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "Backup request");
        let state = self.get_state(ctx);
        match operation {
            "CreateBackupVault" => operations::vaults::create_backup_vault(&state, &input, ctx),
            "DescribeBackupVault" => operations::vaults::describe_backup_vault(&state, &input, ctx),
            "ListBackupVaults" => operations::vaults::list_backup_vaults(&state, &input, ctx),
            "DeleteBackupVault" => operations::vaults::delete_backup_vault(&state, &input, ctx),
            "PutBackupVaultLockConfiguration" => {
                operations::vaults::put_backup_vault_lock_configuration(&state, &input, ctx)
            }
            "DeleteBackupVaultLockConfiguration" => {
                operations::vaults::delete_backup_vault_lock_configuration(&state, &input, ctx)
            }
            "CreateBackupPlan" => operations::plans::create_backup_plan(&state, &input, ctx),
            "GetBackupPlan" => operations::plans::get_backup_plan(&state, &input, ctx),
            "ListBackupPlans" => operations::plans::list_backup_plans(&state, &input, ctx),
            "DeleteBackupPlan" => operations::plans::delete_backup_plan(&state, &input, ctx),
            "UpdateBackupPlan" => operations::plans::update_backup_plan(&state, &input, ctx),
            "CreateBackupSelection" => {
                operations::selections::create_backup_selection(&state, &input, ctx)
            }
            "GetBackupSelection" => {
                operations::selections::get_backup_selection(&state, &input, ctx)
            }
            "ListBackupSelections" => {
                operations::selections::list_backup_selections(&state, &input, ctx)
            }
            "DeleteBackupSelection" => {
                operations::selections::delete_backup_selection(&state, &input, ctx)
            }
            "StartBackupJob" => operations::jobs::start_backup_job(&state, &input, ctx),
            "DescribeBackupJob" => operations::jobs::describe_backup_job(&state, &input, ctx),
            "ListBackupJobs" => operations::jobs::list_backup_jobs(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all = state::BackupStateSnapshot {
            vaults: vec![],
            plans: vec![],
            selections: vec![],
            jobs: vec![],
        };
        for (_, st) in self.store.iter_all() {
            let s = st.to_snapshot();
            all.vaults.extend(s.vaults);
            all.plans.extend(s.plans);
            all.selections.extend(s.selections);
            all.jobs.extend(s.jobs);
        }
        serde_json::to_vec(&all).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: state::BackupStateSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;
        let st = self.store.get("000000000000", "us-east-1");
        st.restore_from_snapshot(snap);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> RequestContext {
        RequestContext::new("backup", "us-east-1")
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
            if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    #[test]
    fn vault_plan_selection_job_lifecycle() {
        let svc = BackupService::new();
        let ctx = ctx();

        // Create vault
        let v = block_on(svc.handle(
            "CreateBackupVault",
            json!({ "BackupVaultName": "my-vault" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(v["BackupVaultName"], "my-vault");

        // Plan
        let p = block_on(svc.handle(
            "CreateBackupPlan",
            json!({
                "BackupPlan": {
                    "BackupPlanName": "daily",
                    "Rules": [{
                        "RuleName": "default",
                        "TargetBackupVaultName": "my-vault",
                        "ScheduleExpression": "cron(0 5 ? * * *)",
                        "Lifecycle": { "DeleteAfterDays": 30 }
                    }]
                }
            }),
            &ctx,
        ))
        .unwrap();
        let plan_id = p["BackupPlanId"].as_str().unwrap().to_string();

        // Selection
        let s = block_on(svc.handle(
            "CreateBackupSelection",
            json!({
                "BackupPlanId": plan_id,
                "BackupSelection": {
                    "SelectionName": "everything-tagged-Backup",
                    "IamRoleArn": "arn:aws:iam::000000000000:role/BackupRole",
                    "ListOfTags": [{"ConditionType": "STRINGEQUALS", "ConditionKey": "Backup", "ConditionValue": "yes"}]
                }
            }),
            &ctx,
        ))
        .unwrap();
        let sel_id = s["SelectionId"].as_str().unwrap();
        assert!(!sel_id.is_empty());

        // Job
        let j = block_on(svc.handle(
            "StartBackupJob",
            json!({
                "BackupVaultName": "my-vault",
                "ResourceArn": "arn:aws:dynamodb:us-east-1:000000000000:table/orders",
                "IamRoleArn": "arn:aws:iam::000000000000:role/BackupRole"
            }),
            &ctx,
        ))
        .unwrap();
        let job_id = j["BackupJobId"].as_str().unwrap();
        let described =
            block_on(svc.handle("DescribeBackupJob", json!({ "BackupJobId": job_id }), &ctx))
                .unwrap();
        assert_eq!(described["State"], "COMPLETED");
        assert_eq!(described["ResourceType"], "DynamoDB");

        // Vault now has 1 recovery point
        let dv = block_on(svc.handle(
            "DescribeBackupVault",
            json!({ "BackupVaultName": "my-vault" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(dv["NumberOfRecoveryPoints"], 1);

        // Cannot delete vault with recovery points
        let err = block_on(svc.handle(
            "DeleteBackupVault",
            json!({ "BackupVaultName": "my-vault" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidRequestException");
    }

    #[test]
    fn delete_plan_cascades_selections() {
        let svc = BackupService::new();
        let ctx = ctx();
        let p = block_on(svc.handle(
            "CreateBackupPlan",
            json!({ "BackupPlan": { "BackupPlanName": "p", "Rules": [] } }),
            &ctx,
        ))
        .unwrap();
        let plan_id = p["BackupPlanId"].as_str().unwrap().to_string();
        block_on(svc.handle(
            "CreateBackupSelection",
            json!({
                "BackupPlanId": plan_id,
                "BackupSelection": { "SelectionName": "s", "IamRoleArn": "arn:aws:iam::000000000000:role/r", "Resources": ["*"] }
            }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle("DeleteBackupPlan", json!({ "BackupPlanId": plan_id }), &ctx)).unwrap();
        let listed = block_on(svc.handle(
            "ListBackupSelections",
            json!({ "BackupPlanId": plan_id }),
            &ctx,
        ))
        .unwrap();
        assert!(
            listed["BackupSelectionsList"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
