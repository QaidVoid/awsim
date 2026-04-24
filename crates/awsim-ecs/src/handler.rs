use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{clusters, container_instances, extras, services, task_definitions, tasks};
use crate::state::EcsState;

/// The ECS service handler.
pub struct EcsService {
    store: AccountRegionStore<EcsState>,
}

impl EcsService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for EcsService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ServiceHandler for EcsService {
    fn service_name(&self) -> &str {
        "ecs"
    }

    fn signing_name(&self) -> &str {
        "ecs"
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
        debug!(operation = %operation, "ECS operation");

        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            // Clusters
            "CreateCluster" => clusters::create_cluster(&state, &input, ctx),
            "DeleteCluster" => clusters::delete_cluster(&state, &input, ctx),
            "DescribeClusters" => clusters::describe_clusters(&state, &input, ctx),
            "ListClusters" => clusters::list_clusters(&state, &input, ctx),

            // Task Definitions
            "RegisterTaskDefinition" => {
                task_definitions::register_task_definition(&state, &input, ctx)
            }
            "DeregisterTaskDefinition" => {
                task_definitions::deregister_task_definition(&state, &input, ctx)
            }
            "DescribeTaskDefinition" => {
                task_definitions::describe_task_definition(&state, &input, ctx)
            }
            "ListTaskDefinitions" => task_definitions::list_task_definitions(&state, &input, ctx),
            "ListTaskDefinitionFamilies" => {
                task_definitions::list_task_definition_families(&state, &input, ctx)
            }

            // Services
            "CreateService" => services::create_service(&state, &input, ctx),
            "DeleteService" => services::delete_service(&state, &input, ctx),
            "DescribeServices" => services::describe_services(&state, &input, ctx),
            "ListServices" => services::list_services(&state, &input, ctx),
            "UpdateService" => services::update_service(&state, &input, ctx),

            // Tasks
            "RunTask" => tasks::run_task(&state, &input, ctx),
            "StopTask" => tasks::stop_task(&state, &input, ctx),
            "DescribeTasks" => tasks::describe_tasks(&state, &input, ctx),
            "ListTasks" => tasks::list_tasks(&state, &input, ctx),

            // Tagging
            "TagResource" => extras::tag_resource(&state, &input, ctx),
            "UntagResource" => extras::untag_resource(&state, &input, ctx),
            "ListTagsForResource" => extras::list_tags_for_resource(&state, &input, ctx),

            // Capacity Providers
            "PutClusterCapacityProviders" => {
                extras::put_cluster_capacity_providers(&state, &input, ctx)
            }
            "DescribeCapacityProviders" => extras::describe_capacity_providers(&state, &input, ctx),

            // Account Settings
            "PutAccountSetting" => extras::put_account_setting(&state, &input, ctx),
            "PutAccountSettingDefault" => extras::put_account_setting(&state, &input, ctx),
            "ListAccountSettings" => extras::list_account_settings(&state, &input, ctx),

            // Container agent
            "DiscoverPollEndpoint" => extras::discover_poll_endpoint(&state, &input, ctx),
            "UpdateContainerAgent" => extras::update_container_agent(&state, &input, ctx),

            // Container Instances + Attributes
            "DescribeContainerInstances" => {
                container_instances::describe_container_instances(&state, &input, ctx)
            }
            "ListContainerInstances" => {
                container_instances::list_container_instances(&state, &input, ctx)
            }
            "ListAttributes" => container_instances::list_attributes(&state, &input, ctx),
            "PutAttributes" => container_instances::put_attributes(&state, &input, ctx),
            "DeleteAttributes" => container_instances::delete_attributes(&state, &input, ctx),
            "ListServicesByNamespace" => {
                container_instances::list_services_by_namespace(&state, &input, ctx)
            }

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
