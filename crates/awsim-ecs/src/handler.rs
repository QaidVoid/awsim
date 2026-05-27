use std::sync::Arc;

use awsim_core::{
    AccountRegionStore, AwsError, CloudMapRegistrar, ParameterLookup, PrincipalLookup, Protocol,
    RequestContext, SecretLookup, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use crate::operations::{clusters, container_instances, extras, services, task_definitions, tasks};
use crate::state::EcsState;

/// The ECS service handler.
pub struct EcsService {
    store: AccountRegionStore<EcsState>,
    /// IAM principal lookup used to validate `taskRoleArn` /
    /// `executionRoleArn` at RegisterTaskDefinition. `None` keeps the
    /// service working in standalone test setups that don't wire IAM.
    iam_lookup: Option<Arc<dyn PrincipalLookup>>,
    /// SecretsManager lookup used to validate
    /// `containerDefinitions[].repositoryCredentials.credentialsParameter`
    /// at RegisterTaskDefinition and `containerDefinitions[].secrets[]`
    /// SecretsManager refs at RunTask. `None` skips the validation.
    secrets_lookup: Option<Arc<dyn SecretLookup>>,
    /// SSM Parameter lookup used to validate
    /// `containerDefinitions[].secrets[]` references that point at
    /// SSM parameters. `None` skips the validation.
    parameters_lookup: Option<Arc<dyn ParameterLookup>>,
    /// Cloud Map registrar used to publish ECS services into a Cloud
    /// Map service whenever CreateService passes `serviceRegistries[]`.
    cloudmap_registrar: Option<Arc<dyn CloudMapRegistrar>>,
}

impl EcsService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
            iam_lookup: None,
            secrets_lookup: None,
            parameters_lookup: None,
            cloudmap_registrar: None,
        }
    }

    /// Plug in the IAM principal lookup so task / execution role ARNs
    /// are verified against the IAM store at registration time.
    pub fn with_iam_lookup(mut self, lookup: Arc<dyn PrincipalLookup>) -> Self {
        self.iam_lookup = Some(lookup);
        self
    }

    /// Plug in the SecretsManager lookup so private-registry
    /// credentials referenced via `repositoryCredentials.credentialsParameter`
    /// are verified against the secrets store at registration time.
    pub fn with_secrets_lookup(mut self, lookup: Arc<dyn SecretLookup>) -> Self {
        self.secrets_lookup = Some(lookup);
        self
    }

    /// Plug in the SSM Parameter Store lookup so container
    /// `secrets[].valueFrom` references that point at SSM parameters
    /// are validated when RunTask materialises the task.
    pub fn with_parameters_lookup(mut self, lookup: Arc<dyn ParameterLookup>) -> Self {
        self.parameters_lookup = Some(lookup);
        self
    }

    /// Plug in the Cloud Map registrar so CreateService with
    /// `serviceRegistries[]` registers an instance per registry, and
    /// DeleteService cleans them up.
    pub fn with_cloudmap_registrar(mut self, registrar: Arc<dyn CloudMapRegistrar>) -> Self {
        self.cloudmap_registrar = Some(registrar);
        self
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
            "RegisterTaskDefinition" => task_definitions::register_task_definition(
                &state,
                &input,
                ctx,
                self.iam_lookup.as_deref(),
                self.secrets_lookup.as_deref(),
            ),
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
            "CreateService" => {
                services::create_service(&state, &input, ctx, self.cloudmap_registrar.as_deref())
            }
            "DeleteService" => {
                services::delete_service(&state, &input, ctx, self.cloudmap_registrar.as_deref())
            }
            "DescribeServices" => services::describe_services(&state, &input, ctx),
            "ListServices" => services::list_services(&state, &input, ctx),
            "UpdateService" => services::update_service(&state, &input, ctx),

            // Tasks
            "RunTask" => tasks::run_task(
                &state,
                &input,
                ctx,
                self.secrets_lookup.as_deref(),
                self.parameters_lookup.as_deref(),
            ),
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
