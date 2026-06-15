mod error;
mod ids;
mod operations;
mod state;

use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::{RdsState, RdsStateSnapshot};

/// The AWSim RDS service handler.
pub struct RdsService {
    store: AccountRegionStore<RdsState>,
}

impl RdsService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<RdsState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }

    /// Count active DB instances for a given account+region — used by
    /// the billing meter to charge instance-hours. AWS bills any RDS
    /// instance that's not in `creating`, `deleting`, or `stopped`,
    /// so we accept the most common live states.
    pub fn running_instance_count(&self, account_id: &str, region: &str) -> u64 {
        let state = self.store.get(account_id, region);
        state
            .instances
            .iter()
            .filter(|i| {
                matches!(
                    i.value().status.as_str(),
                    "available" | "backing-up" | "modifying" | "rebooting" | "starting"
                )
            })
            .count() as u64
    }
}

impl Default for RdsService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for RdsService {
    fn service_name(&self) -> &str {
        "rds"
    }

    fn signing_name(&self) -> &str {
        "rds"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsQuery
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "RDS request");
        let state = self.get_state(ctx);

        match operation {
            // DB Instances
            "CreateDBInstance" => operations::instances::create_db_instance(&state, &input, ctx),
            "CreateDBInstanceReadReplica" => {
                operations::instances::create_db_instance_read_replica(&state, &input, ctx)
            }
            "DeleteDBInstance" => operations::instances::delete_db_instance(&state, &input, ctx),
            "DescribeDBInstances" => {
                operations::instances::describe_db_instances(&state, &input, ctx)
            }
            "ModifyDBInstance" => operations::instances::modify_db_instance(&state, &input, ctx),
            "StartDBInstance" => operations::instances::start_db_instance(&state, &input, ctx),
            "StopDBInstance" => operations::instances::stop_db_instance(&state, &input, ctx),
            "RebootDBInstance" => operations::instances::reboot_db_instance(&state, &input, ctx),
            "RestoreDBInstanceFromDBSnapshot" => {
                operations::instances::restore_db_instance_from_db_snapshot(&state, &input, ctx)
            }

            // DB Clusters
            "CreateDBCluster" => operations::clusters::create_db_cluster(&state, &input, ctx),
            "DeleteDBCluster" => operations::clusters::delete_db_cluster(&state, &input, ctx),
            "DescribeDBClusters" => operations::clusters::describe_db_clusters(&state, &input, ctx),
            "ModifyDBCluster" => operations::clusters::modify_db_cluster(&state, &input, ctx),
            "StartDBCluster" => operations::clusters::start_db_cluster(&state, &input, ctx),
            "StopDBCluster" => operations::clusters::stop_db_cluster(&state, &input, ctx),
            "RebootDBCluster" => operations::clusters::reboot_db_cluster(&state, &input, ctx),
            "FailoverDBCluster" => operations::clusters::failover_db_cluster(&state, &input, ctx),
            "RestoreDBClusterFromSnapshot" => {
                operations::clusters::restore_db_cluster_from_snapshot(&state, &input, ctx)
            }
            "AddRoleToDBCluster" => {
                operations::clusters::add_role_to_db_cluster(&state, &input, ctx)
            }
            "RemoveRoleFromDBCluster" => {
                operations::clusters::remove_role_from_db_cluster(&state, &input, ctx)
            }
            "EnableHttpEndpoint" => operations::clusters::enable_http_endpoint(&state, &input, ctx),
            "DisableHttpEndpoint" => {
                operations::clusters::disable_http_endpoint(&state, &input, ctx)
            }
            "StartActivityStream" => {
                operations::clusters::start_activity_stream(&state, &input, ctx)
            }
            "StopActivityStream" => operations::clusters::stop_activity_stream(&state, &input, ctx),
            "CreateGlobalCluster" => {
                operations::clusters::create_global_cluster(&state, &input, ctx)
            }
            "DeleteGlobalCluster" => {
                operations::clusters::delete_global_cluster(&state, &input, ctx)
            }
            "RemoveFromGlobalCluster" => {
                operations::clusters::remove_from_global_cluster(&state, &input, ctx)
            }
            "DescribeGlobalClusters" => {
                operations::clusters::describe_global_clusters(&state, &input, ctx)
            }

            // DB Subnet Groups
            "CreateDBSubnetGroup" => {
                operations::subnet_groups::create_db_subnet_group(&state, &input, ctx)
            }
            "DeleteDBSubnetGroup" => {
                operations::subnet_groups::delete_db_subnet_group(&state, &input, ctx)
            }
            "DescribeDBSubnetGroups" => {
                operations::subnet_groups::describe_db_subnet_groups(&state, &input, ctx)
            }

            // DB Parameter Groups
            "CreateDBParameterGroup" => {
                operations::parameter_groups::create_db_parameter_group(&state, &input, ctx)
            }
            "DeleteDBParameterGroup" => {
                operations::parameter_groups::delete_db_parameter_group(&state, &input, ctx)
            }
            "DescribeDBParameterGroups" => {
                operations::parameter_groups::describe_db_parameter_groups(&state, &input, ctx)
            }

            // DB Cluster Parameter Groups
            "CreateDBClusterParameterGroup" => {
                operations::cluster_parameter_groups::create_db_cluster_parameter_group(
                    &state, &input, ctx,
                )
            }
            "DeleteDBClusterParameterGroup" => {
                operations::cluster_parameter_groups::delete_db_cluster_parameter_group(
                    &state, &input, ctx,
                )
            }
            "DescribeDBClusterParameterGroups" => {
                operations::cluster_parameter_groups::describe_db_cluster_parameter_groups(
                    &state, &input, ctx,
                )
            }
            "DescribeDBClusterParameters" => {
                operations::cluster_parameter_groups::describe_db_cluster_parameters(
                    &state, &input, ctx,
                )
            }
            "ModifyDBClusterParameterGroup" => {
                operations::cluster_parameter_groups::modify_db_cluster_parameter_group(
                    &state, &input, ctx,
                )
            }
            "ResetDBClusterParameterGroup" => {
                operations::cluster_parameter_groups::reset_db_cluster_parameter_group(
                    &state, &input, ctx,
                )
            }

            // Tags
            "AddTagsToResource" => operations::tags::add_tags_to_resource(&state, &input),
            "RemoveTagsFromResource" => operations::tags::remove_tags_from_resource(&state, &input),
            "ListTagsForResource" => operations::tags::list_tags_for_resource(&state, &input),

            // Engine versions / orderable options
            "DescribeDBEngineVersions" => {
                operations::engine_versions::describe_db_engine_versions(&state, &input)
            }
            "DescribeOrderableDBInstanceOptions" => {
                operations::engine_versions::describe_orderable_db_instance_options(&input)
            }
            "CreateCustomDBEngineVersion" => {
                operations::engine_versions::create_custom_db_engine_version(&state, &input, ctx)
            }
            "ModifyCustomDBEngineVersion" => {
                operations::engine_versions::modify_custom_db_engine_version(&state, &input, ctx)
            }
            "DeleteCustomDBEngineVersion" => {
                operations::engine_versions::delete_custom_db_engine_version(&state, &input, ctx)
            }

            // Snapshots
            "CreateDBSnapshot" => operations::snapshots::create_db_snapshot(&state, &input, ctx),
            "DeleteDBSnapshot" => operations::snapshots::delete_db_snapshot(&state, &input, ctx),
            "DescribeDBSnapshots" => {
                operations::snapshots::describe_db_snapshots(&state, &input, ctx)
            }
            "CopyDBSnapshot" => operations::snapshots::copy_db_snapshot(&state, &input, ctx),

            // Cluster snapshots
            "CreateDBClusterSnapshot" => {
                operations::cluster_snapshots::create_db_cluster_snapshot(&state, &input, ctx)
            }
            "DeleteDBClusterSnapshot" => {
                operations::cluster_snapshots::delete_db_cluster_snapshot(&state, &input, ctx)
            }
            "DescribeDBClusterSnapshots" => {
                operations::cluster_snapshots::describe_db_cluster_snapshots(&state, &input, ctx)
            }
            "CopyDBClusterSnapshot" => {
                operations::cluster_snapshots::copy_db_cluster_snapshot(&state, &input, ctx)
            }

            // Event subscriptions (stub)
            "DescribeEventSubscriptions" => {
                operations::snapshots::describe_event_subscriptions(&input)
            }

            // Log files (stub)
            "DescribeDBLogFiles" => operations::snapshots::describe_db_log_files(&input),

            // Cluster endpoints
            "DescribeDBClusterEndpoints" => {
                operations::cluster_endpoints::describe_db_cluster_endpoints(&state, &input, ctx)
            }
            "CreateDBClusterEndpoint" => {
                operations::cluster_endpoints::create_db_cluster_endpoint(&state, &input, ctx)
            }
            "DeleteDBClusterEndpoint" => {
                operations::cluster_endpoints::delete_db_cluster_endpoint(&state, &input, ctx)
            }

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    /// Apply staged `ModifyDBInstance` diffs whose maintenance window
    /// has come due. `ModifyDBInstance` with `ApplyImmediately=false`
    /// parks the requested values under `pending_modified_values`; AWS
    /// flushes them onto the live instance during the instance's
    /// weekly `PreferredMaintenanceWindow`. We mirror that here:
    /// each tick scans every account/region for instances that have a
    /// pending diff and whose window matches the current wall clock,
    /// applying and clearing the diff. Absolute-time gated and
    /// idempotent, so a missed or repeated tick never loses or
    /// double-applies state.
    async fn tick(&self) {
        let now = SystemTime::now();
        for (_, state) in self.store.iter_all() {
            for mut entry in state.instances.iter_mut() {
                let inst = entry.value_mut();
                if inst.pending_modified_values.is_empty() {
                    continue;
                }
                let matches = inst
                    .preferred_maintenance_window
                    .as_deref()
                    .map(|w| operations::instances::maintenance_window_matches(w, now))
                    .unwrap_or(false);
                if matches {
                    operations::instances::apply_pending_modified_values(inst);
                }
            }
            for mut entry in state.clusters.iter_mut() {
                let cluster = entry.value_mut();
                if cluster.pending_modified_values.is_empty() {
                    continue;
                }
                let matches = cluster
                    .preferred_maintenance_window
                    .as_deref()
                    .map(|w| operations::instances::maintenance_window_matches(w, now))
                    .unwrap_or(false);
                if matches {
                    operations::clusters::apply_pending_cluster_modified_values(cluster);
                }
            }
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut snapshot = RdsStateSnapshot::default();

        for (_, state) in self.store.iter_all() {
            snapshot
                .instances
                .extend(state.instances.iter().map(|e| e.value().clone()));
            snapshot
                .clusters
                .extend(state.clusters.iter().map(|e| e.value().clone()));
            snapshot
                .subnet_groups
                .extend(state.subnet_groups.iter().map(|e| e.value().clone()));
            snapshot
                .parameter_groups
                .extend(state.parameter_groups.iter().map(|e| e.value().clone()));
            snapshot.cluster_parameter_groups.extend(
                state
                    .cluster_parameter_groups
                    .iter()
                    .map(|e| e.value().clone()),
            );
            snapshot.tags.extend(
                state
                    .tags
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone())),
            );
            snapshot
                .snapshots
                .extend(state.snapshots.iter().map(|e| e.value().clone()));
            snapshot
                .cluster_snapshots
                .extend(state.cluster_snapshots.iter().map(|e| e.value().clone()));
            snapshot.cluster_endpoints.extend(
                state
                    .cluster_endpoints
                    .iter()
                    .flat_map(|e| e.value().clone()),
            );
            snapshot.custom_engine_versions.extend(
                state
                    .custom_engine_versions
                    .iter()
                    .map(|e| e.value().clone()),
            );
            snapshot
                .global_clusters
                .extend(state.global_clusters.iter().map(|e| e.value().clone()));
        }

        serde_json::to_vec(&snapshot).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snapshot: RdsStateSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;

        // Derive account+region from ARN of first entity.
        // ARN: arn:aws:rds:{region}:{account}:db:{identifier}
        let (account_id, region) = snapshot
            .instances
            .first()
            .map(|inst| parse_arn_account_region(&inst.arn))
            .or_else(|| {
                snapshot
                    .clusters
                    .first()
                    .map(|c| parse_arn_account_region(&c.arn))
            })
            .unwrap_or_else(|| ("000000000000".to_string(), "us-east-1".to_string()));

        let state = self.store.get(&account_id, &region);

        for inst in snapshot.instances {
            state.instances.insert(inst.identifier.clone(), inst);
        }
        for cluster in snapshot.clusters {
            state.clusters.insert(cluster.identifier.clone(), cluster);
        }
        for sg in snapshot.subnet_groups {
            state.subnet_groups.insert(sg.name.clone(), sg);
        }
        for pg in snapshot.parameter_groups {
            state.parameter_groups.insert(pg.name.clone(), pg);
        }
        for pg in snapshot.cluster_parameter_groups {
            state.cluster_parameter_groups.insert(pg.name.clone(), pg);
        }
        for (arn, tags) in snapshot.tags {
            state.tags.insert(arn, tags);
        }
        for snap in snapshot.snapshots {
            state
                .snapshots
                .insert(snap.snapshot_identifier.clone(), snap);
        }
        for snap in snapshot.cluster_snapshots {
            state
                .cluster_snapshots
                .insert(snap.snapshot_identifier.clone(), snap);
        }
        for ep in snapshot.cluster_endpoints {
            state
                .cluster_endpoints
                .entry(ep.cluster_identifier.clone())
                .or_default()
                .push(ep);
        }
        for cev in snapshot.custom_engine_versions {
            let key = (cev.engine.clone(), cev.engine_version.clone());
            state.custom_engine_versions.insert(key, cev);
        }
        for gc in snapshot.global_clusters {
            state.global_clusters.insert(gc.identifier.clone(), gc);
        }

        Ok(())
    }
}

/// Parse account_id and region from an RDS ARN.
/// Format: arn:aws:rds:{region}:{account}:{resource-type}:{name}
fn parse_arn_account_region(arn: &str) -> (String, String) {
    let parts: Vec<&str> = arn.splitn(7, ':').collect();
    if parts.len() >= 6 {
        (parts[4].to_string(), parts[3].to_string())
    } else {
        ("000000000000".to_string(), "us-east-1".to_string())
    }
}
