//! Amazon EFS emulator: file systems, mount targets, access points, and the
//! lifecycle/backup policy knobs Terraform/CDK templates touch.

mod operations;
pub mod state;

pub use state::EfsState;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

pub struct EfsService {
    store: AccountRegionStore<EfsState>,
}

impl EfsService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<EfsState> {
        self.store.clone()
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<EfsState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for EfsService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for EfsService {
    fn service_name(&self) -> &str {
        "elasticfilesystem"
    }

    fn signing_name(&self) -> &str {
        "elasticfilesystem"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            // File systems
            RouteDefinition {
                method: "POST",
                path_pattern: "/2015-02-01/file-systems",
                operation: "CreateFileSystem",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-02-01/file-systems",
                operation: "DescribeFileSystems",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2015-02-01/file-systems/{FileSystemId}",
                operation: "DeleteFileSystem",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/2015-02-01/file-systems/{FileSystemId}",
                operation: "UpdateFileSystem",
                required_query_param: None,
            },
            // Lifecycle / backup
            RouteDefinition {
                method: "PUT",
                path_pattern: "/2015-02-01/file-systems/{FileSystemId}/lifecycle-configuration",
                operation: "PutLifecycleConfiguration",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-02-01/file-systems/{FileSystemId}/lifecycle-configuration",
                operation: "DescribeLifecycleConfiguration",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-02-01/file-systems/{FileSystemId}/backup-policy",
                operation: "DescribeBackupPolicy",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/2015-02-01/file-systems/{FileSystemId}/backup-policy",
                operation: "PutBackupPolicy",
                required_query_param: None,
            },
            // Mount targets
            RouteDefinition {
                method: "POST",
                path_pattern: "/2015-02-01/mount-targets",
                operation: "CreateMountTarget",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-02-01/mount-targets",
                operation: "DescribeMountTargets",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2015-02-01/mount-targets/{MountTargetId}",
                operation: "DeleteMountTarget",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-02-01/mount-targets/{MountTargetId}/security-groups",
                operation: "DescribeMountTargetSecurityGroups",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/2015-02-01/mount-targets/{MountTargetId}/security-groups",
                operation: "ModifyMountTargetSecurityGroups",
                required_query_param: None,
            },
            // Access points
            RouteDefinition {
                method: "POST",
                path_pattern: "/2015-02-01/access-points",
                operation: "CreateAccessPoint",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-02-01/access-points",
                operation: "DescribeAccessPoints",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2015-02-01/access-points/{AccessPointId}",
                operation: "DeleteAccessPoint",
                required_query_param: None,
            },
            // Tags
            RouteDefinition {
                method: "POST",
                path_pattern: "/2015-02-01/resource-tags/{ResourceId}",
                operation: "TagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2015-02-01/resource-tags/{ResourceId}",
                operation: "UntagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-02-01/resource-tags/{ResourceId}",
                operation: "ListTagsForResource",
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
        debug!(operation, "EFS request");
        let state = self.get_state(ctx);
        match operation {
            "CreateFileSystem" => operations::file_systems::create_file_system(&state, &input, ctx),
            "DescribeFileSystems" => {
                operations::file_systems::describe_file_systems(&state, &input, ctx)
            }
            "DeleteFileSystem" => operations::file_systems::delete_file_system(&state, &input, ctx),
            "UpdateFileSystem" => operations::file_systems::update_file_system(&state, &input, ctx),
            "UpdateFileSystemProtection" => {
                operations::file_systems::update_file_system_protection(&state, &input, ctx)
            }
            "PutFileSystemPolicy" => {
                operations::file_systems::put_file_system_policy(&state, &input, ctx)
            }
            "DescribeFileSystemPolicy" => {
                operations::file_systems::describe_file_system_policy(&state, &input, ctx)
            }
            "DeleteFileSystemPolicy" => {
                operations::file_systems::delete_file_system_policy(&state, &input, ctx)
            }
            "PutLifecycleConfiguration" => {
                operations::file_systems::put_lifecycle_configuration(&state, &input, ctx)
            }
            "DescribeLifecycleConfiguration" => {
                operations::file_systems::describe_lifecycle_configuration(&state, &input, ctx)
            }
            "DescribeBackupPolicy" => {
                operations::file_systems::describe_backup_policy(&state, &input, ctx)
            }
            "PutBackupPolicy" => operations::file_systems::put_backup_policy(&state, &input, ctx),
            "CreateMountTarget" => {
                operations::mount_targets::create_mount_target(&state, &input, ctx)
            }
            "DescribeMountTargets" => {
                operations::mount_targets::describe_mount_targets(&state, &input, ctx)
            }
            "DeleteMountTarget" => {
                operations::mount_targets::delete_mount_target(&state, &input, ctx)
            }
            "DescribeMountTargetSecurityGroups" => {
                operations::mount_targets::describe_mount_target_security_groups(
                    &state, &input, ctx,
                )
            }
            "ModifyMountTargetSecurityGroups" => {
                operations::mount_targets::modify_mount_target_security_groups(&state, &input, ctx)
            }
            "CreateAccessPoint" => {
                operations::access_points::create_access_point(&state, &input, ctx)
            }
            "DescribeAccessPoints" => {
                operations::access_points::describe_access_points(&state, &input, ctx)
            }
            "DeleteAccessPoint" => {
                operations::access_points::delete_access_point(&state, &input, ctx)
            }
            "TagResource" => operations::tags::tag_resource(&state, &input, ctx),
            "UntagResource" => operations::tags::untag_resource(&state, &input, ctx),
            "ListTagsForResource" => operations::tags::list_tags_for_resource(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all = state::EfsStateSnapshot {
            file_systems: vec![],
            mount_targets: vec![],
            access_points: vec![],
        };
        for (_, st) in self.store.iter_all() {
            let s = st.to_snapshot();
            all.file_systems.extend(s.file_systems);
            all.mount_targets.extend(s.mount_targets);
            all.access_points.extend(s.access_points);
        }
        serde_json::to_vec(&all).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: state::EfsStateSnapshot =
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
        RequestContext::new("elasticfilesystem", "us-east-1")
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
    fn full_lifecycle() {
        let svc = EfsService::new();
        let ctx = ctx();

        let fs = block_on(svc.handle(
            "CreateFileSystem",
            json!({ "CreationToken": "tok-1", "Encrypted": true, "Tags": [{"Key":"Name","Value":"data"}] }),
            &ctx,
        ))
        .unwrap();
        let fs_id = fs["FileSystemId"].as_str().unwrap().to_string();
        assert!(fs_id.starts_with("fs-"));
        assert_eq!(fs["LifeCycleState"], "available");

        // Idempotency: replay with identical args returns the same FS.
        let again = block_on(svc.handle(
            "CreateFileSystem",
            json!({ "CreationToken": "tok-1", "Encrypted": true, "Tags": [{"Key":"Name","Value":"data"}] }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(again["FileSystemId"], fs_id);

        let mt = block_on(svc.handle(
            "CreateMountTarget",
            json!({ "FileSystemId": fs_id, "SubnetId": "subnet-abc" }),
            &ctx,
        ))
        .unwrap();
        assert!(mt["MountTargetId"].as_str().unwrap().starts_with("fsmt-"));

        let described = block_on(svc.handle(
            "DescribeFileSystems",
            json!({ "FileSystemId": fs_id }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(described["FileSystems"][0]["NumberOfMountTargets"], 1);

        let mt_id = mt["MountTargetId"].as_str().unwrap();
        block_on(svc.handle("DeleteMountTarget", json!({ "MountTargetId": mt_id }), &ctx)).unwrap();
        block_on(svc.handle("DeleteFileSystem", json!({ "FileSystemId": fs_id }), &ctx)).unwrap();
    }

    #[test]
    fn delete_blocks_when_mount_targets_present() {
        let svc = EfsService::new();
        let ctx = ctx();
        let fs = block_on(svc.handle("CreateFileSystem", json!({ "CreationToken": "t" }), &ctx))
            .unwrap();
        let fs_id = fs["FileSystemId"].as_str().unwrap().to_string();
        block_on(svc.handle(
            "CreateMountTarget",
            json!({ "FileSystemId": fs_id, "SubnetId": "subnet-x" }),
            &ctx,
        ))
        .unwrap();
        let err = block_on(svc.handle("DeleteFileSystem", json!({ "FileSystemId": fs_id }), &ctx))
            .unwrap_err();
        assert_eq!(err.code, "FileSystemInUse");
    }
}
