pub mod error;
mod operations;
mod state;

use std::time::SystemTime;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use state::{ClusterState, EksState};

pub struct EksService {
    store: AccountRegionStore<EksState>,
}

impl EksService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for EksService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for EksService {
    fn service_name(&self) -> &str {
        "eks"
    }

    fn signing_name(&self) -> &str {
        "eks"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "POST",
                path_pattern: "/clusters",
                operation: "CreateCluster",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/clusters",
                operation: "ListClusters",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/clusters/{name}",
                operation: "DescribeCluster",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/clusters/{name}",
                operation: "DeleteCluster",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/clusters/{name}/update-config",
                operation: "UpdateClusterConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/clusters/{name}/encryption-config/associate",
                operation: "AssociateEncryptionConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/clusters/{clusterName}/node-groups",
                operation: "CreateNodegroup",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/clusters/{clusterName}/node-groups",
                operation: "ListNodegroups",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/clusters/{clusterName}/node-groups/{nodegroupName}",
                operation: "DescribeNodegroup",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/clusters/{clusterName}/node-groups/{nodegroupName}",
                operation: "DeleteNodegroup",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/clusters/{clusterName}/fargate-profiles",
                operation: "CreateFargateProfile",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/clusters/{clusterName}/fargate-profiles",
                operation: "ListFargateProfiles",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/clusters/{clusterName}/fargate-profiles/{fargateProfileName}",
                operation: "DescribeFargateProfile",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/clusters/{clusterName}/fargate-profiles/{fargateProfileName}",
                operation: "DeleteFargateProfile",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/clusters/{clusterName}/addons",
                operation: "CreateAddon",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/clusters/{clusterName}/addons",
                operation: "ListAddons",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/clusters/{clusterName}/addons/{addonName}",
                operation: "DescribeAddon",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/clusters/{clusterName}/addons/{addonName}/update",
                operation: "UpdateAddon",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/clusters/{clusterName}/addons/{addonName}",
                operation: "DeleteAddon",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/tags/{resourceArn}",
                operation: "TagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/tags/{resourceArn}",
                operation: "UntagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/tags/{resourceArn}",
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
        debug!(operation = %operation, "EKS operation");
        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            "CreateCluster" => operations::clusters::create_cluster(&state, &input, ctx),
            "DescribeCluster" => operations::clusters::describe_cluster(&state, &input, ctx),
            "DeleteCluster" => operations::clusters::delete_cluster(&state, &input, ctx),
            "ListClusters" => operations::clusters::list_clusters(&state, &input, ctx),
            "UpdateClusterConfig" => {
                operations::clusters::update_cluster_config(&state, &input, ctx)
            }
            "AssociateEncryptionConfig" => {
                operations::clusters::associate_encryption_config(&state, &input, ctx)
            }
            "CreateNodegroup" => operations::nodegroups::create_nodegroup(&state, &input, ctx),
            "DescribeNodegroup" => operations::nodegroups::describe_nodegroup(&state, &input, ctx),
            "DeleteNodegroup" => operations::nodegroups::delete_nodegroup(&state, &input, ctx),
            "ListNodegroups" => operations::nodegroups::list_nodegroups(&state, &input, ctx),
            "CreateFargateProfile" => {
                operations::fargate_profiles::create_fargate_profile(&state, &input, ctx)
            }
            "DescribeFargateProfile" => {
                operations::fargate_profiles::describe_fargate_profile(&state, &input, ctx)
            }
            "DeleteFargateProfile" => {
                operations::fargate_profiles::delete_fargate_profile(&state, &input, ctx)
            }
            "ListFargateProfiles" => {
                operations::fargate_profiles::list_fargate_profiles(&state, &input, ctx)
            }
            "CreateAddon" => operations::addons::create_addon(&state, &input, ctx),
            "DescribeAddon" => operations::addons::describe_addon(&state, &input, ctx),
            "ListAddons" => operations::addons::list_addons(&state, &input, ctx),
            "UpdateAddon" => operations::addons::update_addon(&state, &input, ctx),
            "DeleteAddon" => operations::addons::delete_addon(&state, &input, ctx),
            "TagResource" => operations::tags::tag_resource(&state, &input, ctx),
            "UntagResource" => operations::tags::untag_resource(&state, &input, ctx),
            "ListTagsForResource" => operations::tags::list_tags_for_resource(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    /// Promote transient cluster/nodegroup lifecycles and reap
    /// clusters whose `DELETING` window has elapsed.
    ///
    /// Each pass observes every resource's state machine at the
    /// current wall clock, which flips `CREATING`/`UPDATING` to their
    /// scheduled successor once the deadline passes, then removes any
    /// cluster that has reached its armed reap deadline. Absolute-time
    /// gated and idempotent: a missed or repeated tick never loses or
    /// double-applies state, and the scan touches only in-memory
    /// counters so it stays well under the tick budget.
    async fn tick(&self) {
        let now = SystemTime::now();
        for (_, state) in self.store.iter_all() {
            // Promote transient nodegroups (CREATING -> ACTIVE).
            for entry in state.nodegroups.iter() {
                entry.value().sm.observe(now);
            }
            // Promote transient clusters, then collect any cluster
            // whose DELETING reap deadline has passed.
            let mut reap: Vec<String> = Vec::new();
            for entry in state.clusters.iter() {
                let c = entry.value();
                let observed = c.sm.observe(now).state;
                if observed == ClusterState::Deleting && c.reap_at.is_some_and(|at| now >= at) {
                    reap.push(entry.key().clone());
                }
            }
            for name in reap {
                state.clusters.remove(&name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> RequestContext {
        RequestContext::new("eks", "us-east-1")
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
    fn list_clusters_paginates() {
        let svc = EksService::new();
        let ctx = ctx();
        for name in ["c1", "c2", "c3"] {
            block_on(svc.handle(
                "CreateCluster",
                json!({ "name": name, "roleArn": "arn:aws:iam::000000000000:role/eks" }),
                &ctx,
            ))
            .unwrap();
        }

        let mut seen: Vec<String> = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut input = json!({ "maxResults": 2 });
            if let Some(t) = &token {
                input["nextToken"] = json!(t);
            }
            let page = block_on(svc.handle("ListClusters", input, &ctx)).unwrap();
            for c in page["clusters"].as_array().unwrap() {
                seen.push(c.as_str().unwrap().to_string());
            }
            match page["nextToken"].as_str() {
                Some(t) => token = Some(t.to_string()),
                None => break,
            }
        }
        seen.sort();
        seen.dedup();
        assert_eq!(
            seen.len(),
            3,
            "every cluster returned exactly once across pages"
        );
    }

    #[test]
    fn associate_encryption_config_replaces_cluster_encryption_config() {
        let svc = EksService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateCluster",
            json!({
                "name": "demo",
                "roleArn": "arn:aws:iam::000000000000:role/eks",
            }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "AssociateEncryptionConfig",
            json!({
                "name": "demo",
                "encryptionConfig": [{
                    "resources": ["secrets"],
                    "provider": { "keyArn": "arn:aws:kms:us-east-1:000000000000:key/k" }
                }]
            }),
            &ctx,
        ))
        .unwrap();

        let desc =
            block_on(svc.handle("DescribeCluster", json!({ "name": "demo" }), &ctx)).unwrap();
        let cfg = desc["cluster"]["encryptionConfig"].as_array().unwrap();
        assert_eq!(cfg.len(), 1);
        assert_eq!(cfg[0]["resources"][0], "secrets");
    }

    #[test]
    fn create_nodegroup_requires_subnets() {
        let svc = EksService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateCluster",
            json!({ "name": "c", "roleArn": "arn:aws:iam::000000000000:role/eks" }),
            &ctx,
        ))
        .unwrap();
        let err = block_on(svc.handle(
            "CreateNodegroup",
            json!({ "clusterName": "c", "nodegroupName": "ng" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
        assert!(err.message.contains("subnets"));
    }

    #[test]
    fn create_nodegroup_rejects_oversize_disk() {
        let svc = EksService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateCluster",
            json!({ "name": "c", "roleArn": "arn:aws:iam::000000000000:role/eks" }),
            &ctx,
        ))
        .unwrap();
        let err = block_on(svc.handle(
            "CreateNodegroup",
            json!({
                "clusterName": "c",
                "nodegroupName": "ng",
                "subnets": ["subnet-1"],
                "diskSize": 1_000_000,
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn associate_encryption_config_rejects_empty_array() {
        let svc = EksService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateCluster",
            json!({ "name": "demo", "roleArn": "arn:aws:iam::000000000000:role/eks" }),
            &ctx,
        ))
        .unwrap();

        let err = block_on(svc.handle(
            "AssociateEncryptionConfig",
            json!({ "name": "demo", "encryptionConfig": [] }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn cluster_reports_creating_then_active_after_observe() {
        use std::time::{Duration, SystemTime};

        let svc = EksService::new();
        let ctx = ctx();
        let created = block_on(svc.handle(
            "CreateCluster",
            json!({ "name": "life", "roleArn": "arn:aws:iam::000000000000:role/eks" }),
            &ctx,
        ))
        .unwrap();
        // CreateCluster lands the cluster in CREATING; the default
        // (non-fast) delay keeps it there for a real Describe at now.
        // Under AWSIM_LIFECYCLE_FAST the transition collapses to zero so
        // it reports ACTIVE immediately -- accept either.
        if !awsim_core::lifecycle::fast_mode() {
            assert_eq!(created["cluster"]["status"], "CREATING");
            let desc =
                block_on(svc.handle("DescribeCluster", json!({ "name": "life" }), &ctx)).unwrap();
            assert_eq!(desc["cluster"]["status"], "CREATING");
        }

        // Drive the deadline deterministically by observing the stored
        // state machine at a future wall clock, then a polling Describe
        // (observing at `now`) sees the promotion.
        let state = svc.store.get(&ctx.account_id, &ctx.region);
        let later = SystemTime::now() + Duration::from_secs(3600);
        assert_eq!(
            state.clusters.get("life").unwrap().sm.observe(later).state,
            ClusterState::Active
        );
        let desc =
            block_on(svc.handle("DescribeCluster", json!({ "name": "life" }), &ctx)).unwrap();
        assert_eq!(desc["cluster"]["status"], "ACTIVE");
    }

    #[test]
    fn delete_cluster_marks_deleting_then_tick_reaps() {
        use std::time::{Duration, SystemTime};

        let svc = EksService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateCluster",
            json!({ "name": "doomed", "roleArn": "arn:aws:iam::000000000000:role/eks" }),
            &ctx,
        ))
        .unwrap();
        // Promote to ACTIVE first so the delete starts from a steady state.
        let state = svc.store.get(&ctx.account_id, &ctx.region);
        let later = SystemTime::now() + Duration::from_secs(3600);
        state.clusters.get("doomed").unwrap().sm.observe(later);

        let deleted =
            block_on(svc.handle("DeleteCluster", json!({ "name": "doomed" }), &ctx)).unwrap();
        assert_eq!(deleted["cluster"]["status"], "DELETING");
        // A polling Describe still sees the cluster in DELETING.
        let desc =
            block_on(svc.handle("DescribeCluster", json!({ "name": "doomed" }), &ctx)).unwrap();
        assert_eq!(desc["cluster"]["status"], "DELETING");

        // Force the reap deadline into the past and tick once.
        state.clusters.get_mut("doomed").unwrap().reap_at =
            Some(SystemTime::now() - Duration::from_secs(1));
        block_on(svc.tick());

        let err =
            block_on(svc.handle("DescribeCluster", json!({ "name": "doomed" }), &ctx)).unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn nodegroup_reports_creating_then_active_after_observe() {
        use crate::state::NodegroupState;
        use std::time::{Duration, SystemTime};

        let svc = EksService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateCluster",
            json!({ "name": "c", "roleArn": "arn:aws:iam::000000000000:role/eks" }),
            &ctx,
        ))
        .unwrap();
        let created = block_on(svc.handle(
            "CreateNodegroup",
            json!({ "clusterName": "c", "nodegroupName": "ng", "subnets": ["subnet-1"] }),
            &ctx,
        ))
        .unwrap();
        if !awsim_core::lifecycle::fast_mode() {
            assert_eq!(created["nodegroup"]["status"], "CREATING");
        }

        let state = svc.store.get(&ctx.account_id, &ctx.region);
        let later = SystemTime::now() + Duration::from_secs(3600);
        let key = ("c".to_string(), "ng".to_string());
        assert_eq!(
            state.nodegroups.get(&key).unwrap().sm.observe(later).state,
            NodegroupState::Active
        );
        let desc = block_on(svc.handle(
            "DescribeNodegroup",
            json!({ "clusterName": "c", "nodegroupName": "ng" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(desc["nodegroup"]["status"], "ACTIVE");
    }
}
