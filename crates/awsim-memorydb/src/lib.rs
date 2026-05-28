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
            "FailoverShard" => operations::failover_shard(&state, &input, ctx),
            "CreateUser" => operations::create_user(&state, &input, ctx),
            "DescribeUsers" => operations::describe_users(&state, &input, ctx),
            "DeleteUser" => operations::delete_user(&state, &input, ctx),
            "UpdateUser" => operations::update_user(&state, &input, ctx),
            "CreateACL" => operations::create_acl(&state, &input, ctx),
            "DescribeACLs" => operations::describe_acls(&state, &input, ctx),
            "DeleteACL" => operations::delete_acl(&state, &input, ctx),
            "UpdateACL" => operations::update_acl(&state, &input, ctx),
            "CreateSubnetGroup" => operations::create_subnet_group(&state, &input, ctx),
            "DescribeSubnetGroups" => operations::describe_subnet_groups(&state, &input, ctx),
            "DeleteSubnetGroup" => operations::delete_subnet_group(&state, &input, ctx),
            "UpdateSubnetGroup" => operations::update_subnet_group(&state, &input, ctx),
            "CreateParameterGroup" => operations::create_parameter_group(&state, &input, ctx),
            "DescribeParameterGroups" => operations::describe_parameter_groups(&state, &input, ctx),
            "DeleteParameterGroup" => operations::delete_parameter_group(&state, &input, ctx),
            "ResetParameterGroup" => operations::reset_parameter_group(&state, &input, ctx),
            "DescribeServiceUpdates" => operations::describe_service_updates(&state, &input, ctx),
            "DescribeEngineVersions" => operations::describe_engine_versions(&state, &input, ctx),
            "BatchUpdateCluster" => operations::batch_update_cluster(&state, &input, ctx),
            "CreateSnapshot" => operations::create_snapshot(&state, &input, ctx),
            "CopySnapshot" => operations::copy_snapshot(&state, &input, ctx),
            "DescribeSnapshots" => operations::describe_snapshots(&state, &input, ctx),
            "DeleteSnapshot" => operations::delete_snapshot(&state, &input, ctx),
            "TagResource" => operations::tag_resource(&state, &input, ctx),
            "UntagResource" => operations::untag_resource(&state, &input, ctx),
            "ListTags" => operations::list_tags(&state, &input, ctx),
            "DescribeEvents" => operations::describe_events(&state, &input, ctx),
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
            tags: Default::default(),
            events: vec![],
        };
        for (_, st) in self.store.iter_all() {
            let s = st.to_snapshot();
            all.clusters.extend(s.clusters);
            all.users.extend(s.users);
            all.acls.extend(s.acls);
            all.snapshots.extend(s.snapshots);
            all.subnet_groups.extend(s.subnet_groups);
            all.parameter_groups.extend(s.parameter_groups);
            all.tags.extend(s.tags);
            all.events.extend(s.events);
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
            json!({ "UserName": "app", "AccessString": "on ~* +@all", "AuthenticationMode": { "Type": "password", "Passwords": ["hunter2hunter2"] } }),
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
    fn snapshot_round_trips_all_resource_fields() {
        let svc = MemoryDbService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateUser",
            json!({
                "UserName": "rt-user",
                "AccessString": "on ~* +@all",
                "AuthenticationMode": { "Type": "iam" },
            }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "CreateACL",
            json!({ "ACLName": "rt-acl", "UserNames": ["rt-user"] }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "CreateSubnetGroup",
            json!({ "SubnetGroupName": "rt-sg", "SubnetIds": ["subnet-a"] }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "CreateParameterGroup",
            json!({ "ParameterGroupName": "rt-pg", "Family": "memorydb_valkey8" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "CreateCluster",
            json!({
                "ClusterName": "rt-cluster",
                "NodeType": "db.r6g.xlarge",
                "ACLName": "rt-acl",
                "Engine": "valkey",
                "EngineVersion": "8.0",
                "MaintenanceWindow": "tue:02:00-tue:04:00",
                "SnapshotWindow": "05:00-06:00",
                "SnapshotRetentionLimit": 7,
                "NumShards": 2,
                "NumReplicasPerShard": 1,
                "MultiRegionClusterName": "rt-mr",
            }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "TagResource",
            json!({
                "ResourceArn": "arn:aws:memorydb:us-east-1:000000000000:cluster/rt-cluster",
                "Tags": [{ "Key": "team", "Value": "data" }],
            }),
            &ctx,
        ))
        .unwrap();
        let bytes = svc.snapshot().expect("encode");
        let restored = MemoryDbService::new();
        restored.restore(&bytes).expect("decode");
        let cluster = block_on(restored.handle(
            "DescribeClusters",
            json!({ "ClusterName": "rt-cluster", "ShowShardDetails": true }),
            &ctx,
        ))
        .unwrap();
        let c = &cluster["Clusters"][0];
        assert_eq!(c["Engine"], "valkey");
        assert_eq!(c["EngineVersion"], "8.0");
        assert_eq!(c["MaintenanceWindow"], "tue:02:00-tue:04:00");
        assert_eq!(c["SnapshotWindow"], "05:00-06:00");
        assert_eq!(c["SnapshotRetentionLimit"], 7);
        assert_eq!(c["MultiRegionClusterName"], "rt-mr");
        assert_eq!(c["Shards"].as_array().unwrap().len(), 2);
        let acls = block_on(restored.handle("DescribeACLs", json!({ "ACLName": "rt-acl" }), &ctx))
            .unwrap();
        assert!(
            acls["ACLs"][0]["UserNames"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n == "rt-user")
        );
        let tags = block_on(restored.handle(
            "ListTags",
            json!({
                "ResourceArn": "arn:aws:memorydb:us-east-1:000000000000:cluster/rt-cluster",
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(tags["TagList"][0]["Key"], "team");
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
