//! Amazon S3 Glacier emulator. Vaults, archives (single-shot upload), and a
//! fast-forwarded job lifecycle (jobs land in Succeeded immediately so callers
//! don't have to poll).

mod operations;
pub mod state;

pub use state::GlacierState;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

pub struct GlacierService {
    store: AccountRegionStore<GlacierState>,
}

impl GlacierService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<GlacierState> {
        self.store.clone()
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<GlacierState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for GlacierService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for GlacierService {
    fn service_name(&self) -> &str {
        "glacier"
    }

    fn signing_name(&self) -> &str {
        "glacier"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            // Vaults
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{accountId}/vaults/{vaultName}",
                operation: "CreateVault",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/{accountId}/vaults/{vaultName}",
                operation: "DescribeVault",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/{accountId}/vaults",
                operation: "ListVaults",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{accountId}/vaults/{vaultName}",
                operation: "DeleteVault",
                required_query_param: None,
            },
            // Archives
            RouteDefinition {
                method: "POST",
                path_pattern: "/{accountId}/vaults/{vaultName}/archives",
                operation: "UploadArchive",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{accountId}/vaults/{vaultName}/archives/{archiveId}",
                operation: "DeleteArchive",
                required_query_param: None,
            },
            // Jobs
            RouteDefinition {
                method: "POST",
                path_pattern: "/{accountId}/vaults/{vaultName}/jobs",
                operation: "InitiateJob",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/{accountId}/vaults/{vaultName}/jobs/{jobId}",
                operation: "DescribeJob",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/{accountId}/vaults/{vaultName}/jobs",
                operation: "ListJobs",
                required_query_param: None,
            },
            // Notifications
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{accountId}/vaults/{vaultName}/notification-configuration",
                operation: "SetVaultNotifications",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/{accountId}/vaults/{vaultName}/notification-configuration",
                operation: "GetVaultNotifications",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{accountId}/vaults/{vaultName}/notification-configuration",
                operation: "DeleteVaultNotifications",
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
        debug!(operation, "Glacier request");
        let state = self.get_state(ctx);
        match operation {
            "CreateVault" => operations::create_vault(&state, &input, ctx),
            "DescribeVault" => operations::describe_vault(&state, &input, ctx),
            "ListVaults" => operations::list_vaults(&state, &input, ctx),
            "DeleteVault" => operations::delete_vault(&state, &input, ctx),
            "UploadArchive" => operations::upload_archive(&state, &input, ctx),
            "DeleteArchive" => operations::delete_archive(&state, &input, ctx),
            "InitiateJob" => operations::initiate_job(&state, &input, ctx),
            "DescribeJob" => operations::describe_job(&state, &input, ctx),
            "ListJobs" => operations::list_jobs(&state, &input, ctx),
            "SetVaultNotifications" => operations::set_vault_notifications(&state, &input, ctx),
            "GetVaultNotifications" => operations::get_vault_notifications(&state, &input, ctx),
            "DeleteVaultNotifications" => {
                operations::delete_vault_notifications(&state, &input, ctx)
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all = state::GlacierSnapshot {
            vaults: vec![],
            archives: vec![],
            jobs: vec![],
        };
        for (_, st) in self.store.iter_all() {
            let s = st.to_snapshot();
            all.vaults.extend(s.vaults);
            all.archives.extend(s.archives);
            all.jobs.extend(s.jobs);
        }
        serde_json::to_vec(&all).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: state::GlacierSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;
        let st = self.store.get("000000000000", "us-east-1");
        st.restore_from_snapshot(snap);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    use serde_json::json;

    fn ctx() -> RequestContext {
        RequestContext::new("glacier", "us-east-1")
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
    fn vault_archive_job_lifecycle() {
        let svc = GlacierService::new();
        let ctx = ctx();

        block_on(svc.handle("CreateVault", json!({ "vaultName": "long-term" }), &ctx)).unwrap();

        let body = b"some archived bytes";
        let upload = block_on(svc.handle(
            "UploadArchive",
            json!({ "vaultName": "long-term", "body": B64.encode(body) }),
            &ctx,
        ))
        .unwrap();
        let archive_id = upload["ArchiveId"].as_str().unwrap().to_string();
        assert!(!upload["Checksum"].as_str().unwrap().is_empty());

        let v = block_on(svc.handle("DescribeVault", json!({ "vaultName": "long-term" }), &ctx))
            .unwrap();
        assert_eq!(v["NumberOfArchives"], 1);
        assert_eq!(v["SizeInBytes"], body.len());

        // Vault delete blocked
        let err = block_on(svc.handle("DeleteVault", json!({ "vaultName": "long-term" }), &ctx))
            .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");

        // Job
        let job = block_on(svc.handle(
            "InitiateJob",
            json!({
                "vaultName": "long-term",
                "jobParameters": { "Type": "archive-retrieval", "ArchiveId": archive_id }
            }),
            &ctx,
        ))
        .unwrap();
        let job_id = job["JobId"].as_str().unwrap();
        let described = block_on(svc.handle(
            "DescribeJob",
            json!({ "vaultName": "long-term", "jobId": job_id }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(described["StatusCode"], "Succeeded");
        assert_eq!(described["Completed"], true);

        // Delete archive then vault
        block_on(svc.handle(
            "DeleteArchive",
            json!({ "vaultName": "long-term", "archiveId": archive_id }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle("DeleteVault", json!({ "vaultName": "long-term" }), &ctx)).unwrap();
    }
}
