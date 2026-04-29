//! Amazon MemoryDB for Redis emulator. Cluster, user, ACL, snapshot, subnet
//! group, and parameter group metadata.

mod operations;
pub mod state;

pub use state::MemoryDbState;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

pub struct MemoryDbService {
    store: AccountRegionStore<MemoryDbState>,
}

impl MemoryDbService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<MemoryDbState> {
        self.store.clone()
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<MemoryDbState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }

    /// Count active MemoryDB nodes for a given account+region — used
    /// by the billing meter to charge node-hours. AWS bills per-node
    /// for active clusters; nodes-per-cluster equals number_of_shards
    /// (single-node-per-shard primary, no replica modeling here).
    pub fn running_node_count(&self, account_id: &str, region: &str) -> u64 {
        let state = self.store.get(account_id, region);
        state
            .clusters
            .iter()
            .filter(|c| {
                matches!(
                    c.value().status.as_str(),
                    "available" | "creating" | "modifying" | "snapshotting"
                )
            })
            .map(|c| c.value().number_of_shards.max(1) as u64)
            .sum()
    }
}

impl Default for MemoryDbService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for MemoryDbService {
    fn service_name(&self) -> &str {
        "memorydb"
    }

    fn signing_name(&self) -> &str {
        "memorydb"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![]
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "MemoryDB request");
        let state = self.get_state(ctx);
        match operation {
            "CreateCluster" => operations::create_cluster(&state, &input, ctx),
            "DescribeClusters" => operations::describe_clusters(&state, &input, ctx),
            "DeleteCluster" => operations::delete_cluster(&state, &input, ctx),
            "UpdateCluster" => operations::update_cluster(&state, &input, ctx),
            "CreateUser" => operations::create_user(&state, &input, ctx),
            "DescribeUsers" => operations::describe_users(&state, &input, ctx),
            "DeleteUser" => operations::delete_user(&state, &input, ctx),
            "UpdateUser" => operations::update_user(&state, &input, ctx),
            "CreateACL" => operations::create_acl(&state, &input, ctx),
            "DescribeACLs" => operations::describe_acls(&state, &input, ctx),
            "DeleteACL" => operations::delete_acl(&state, &input, ctx),
            "CreateSubnetGroup" => operations::create_subnet_group(&state, &input, ctx),
            "DescribeSubnetGroups" => operations::describe_subnet_groups(&state, &input, ctx),
            "CreateParameterGroup" => operations::create_parameter_group(&state, &input, ctx),
            "DescribeParameterGroups" => operations::describe_parameter_groups(&state, &input, ctx),
            "CreateSnapshot" => operations::create_snapshot(&state, &input, ctx),
            "DescribeSnapshots" => operations::describe_snapshots(&state, &input, ctx),
            "DeleteSnapshot" => operations::delete_snapshot(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all = state::MemoryDbSnapshot {
            clusters: vec![],
            users: vec![],
            acls: vec![],
            snapshots: vec![],
            subnet_groups: vec![],
            parameter_groups: vec![],
        };
        for (_, st) in self.store.iter_all() {
            let s = st.to_snapshot();
            all.clusters.extend(s.clusters);
            all.users.extend(s.users);
            all.acls.extend(s.acls);
            all.snapshots.extend(s.snapshots);
            all.subnet_groups.extend(s.subnet_groups);
            all.parameter_groups.extend(s.parameter_groups);
        }
        serde_json::to_vec(&all).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: state::MemoryDbSnapshot =
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
        RequestContext::new("memorydb", "us-east-1")
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
    fn cluster_user_acl_lifecycle() {
        let svc = MemoryDbService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateUser",
            json!({ "UserName": "app", "AccessString": "on ~* +@all", "AuthenticationMode": { "Type": "password" } }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "CreateACL",
            json!({ "ACLName": "app-acl", "UserNames": ["app"] }),
            &ctx,
        ))
        .unwrap();
        let r = block_on(svc.handle(
            "CreateCluster",
            json!({
                "ClusterName": "primary",
                "NodeType": "db.t4g.small",
                "ACLName": "app-acl",
                "NumShards": 1
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(r["Cluster"]["Status"], "available");
        assert_eq!(r["Cluster"]["ClusterEndpoint"]["Port"], 6379);

        let described = block_on(svc.handle("DescribeClusters", json!({}), &ctx)).unwrap();
        assert_eq!(described["Clusters"].as_array().unwrap().len(), 1);

        block_on(svc.handle("DeleteCluster", json!({ "ClusterName": "primary" }), &ctx)).unwrap();
    }

    #[test]
    fn duplicate_cluster_rejected() {
        let svc = MemoryDbService::new();
        let ctx = ctx();
        let body = json!({
            "ClusterName": "dup",
            "NodeType": "db.t4g.small",
            "ACLName": "open-access"
        });
        block_on(svc.handle("CreateCluster", body.clone(), &ctx)).unwrap();
        let err = block_on(svc.handle("CreateCluster", body, &ctx)).unwrap_err();
        assert_eq!(err.code, "ClusterAlreadyExistsFault");
    }
}
