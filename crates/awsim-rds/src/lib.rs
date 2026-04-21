mod error;
mod ids;
mod operations;
mod state;

use std::sync::Arc;

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
            "CreateDBInstance" => {
                operations::instances::create_db_instance(&state, &input, ctx)
            }
            "DeleteDBInstance" => {
                operations::instances::delete_db_instance(&state, &input, ctx)
            }
            "DescribeDBInstances" => {
                operations::instances::describe_db_instances(&state, &input, ctx)
            }
            "ModifyDBInstance" => {
                operations::instances::modify_db_instance(&state, &input, ctx)
            }
            "StartDBInstance" => {
                operations::instances::start_db_instance(&state, &input, ctx)
            }
            "StopDBInstance" => {
                operations::instances::stop_db_instance(&state, &input, ctx)
            }
            "RebootDBInstance" => {
                operations::instances::reboot_db_instance(&state, &input, ctx)
            }

            // DB Clusters
            "CreateDBCluster" => {
                operations::clusters::create_db_cluster(&state, &input, ctx)
            }
            "DeleteDBCluster" => {
                operations::clusters::delete_db_cluster(&state, &input, ctx)
            }
            "DescribeDBClusters" => {
                operations::clusters::describe_db_clusters(&state, &input, ctx)
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

            // Tags
            "AddTagsToResource" => operations::tags::add_tags_to_resource(&state, &input),
            "RemoveTagsFromResource" => {
                operations::tags::remove_tags_from_resource(&state, &input)
            }
            "ListTagsForResource" => operations::tags::list_tags_for_resource(&state, &input),

            _ => Err(AwsError::unknown_operation(operation)),
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
            snapshot.tags.extend(
                state
                    .tags
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone())),
            );
        }

        serde_json::to_vec(&snapshot).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snapshot: RdsStateSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;

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
        for (arn, tags) in snapshot.tags {
            state.tags.insert(arn, tags);
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
