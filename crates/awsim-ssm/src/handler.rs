use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{
    activations, automation, commands, compliance, documents, maintenance, opsmeta, parameters,
    patch_baselines, policies, sessions, tags,
};
use crate::state::SsmState;

/// The SSM Parameter Store service handler.
pub struct SsmService {
    store: AccountRegionStore<SsmState>,
}

impl SsmService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    /// Borrow the inner store so other crates (ECS container
    /// `secrets[]`, Lambda Parameter resolution, etc.) can wire a
    /// [`ParameterLookup`](awsim_core::ParameterLookup) over it and
    /// validate parameter references against the live state.
    pub fn store(&self) -> AccountRegionStore<SsmState> {
        self.store.clone()
    }
}

impl Default for SsmService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ServiceHandler for SsmService {
    fn service_name(&self) -> &str {
        "ssm"
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
        debug!(operation = %operation, "SSM operation");

        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            "PutParameter" => parameters::put_parameter(&state, &input, ctx),
            "GetParameter" => parameters::get_parameter(&state, &input, ctx),
            "GetParameters" => parameters::get_parameters(&state, &input, ctx),
            "GetParametersByPath" => parameters::get_parameters_by_path(&state, &input, ctx),
            "DeleteParameter" => parameters::delete_parameter(&state, &input, ctx),
            "DeleteParameters" => parameters::delete_parameters(&state, &input, ctx),
            "DescribeParameters" => parameters::describe_parameters(&state, &input, ctx),
            "GetParameterHistory" => parameters::get_parameter_history(&state, &input, ctx),
            "LabelParameterVersion" => parameters::label_parameter_version(&state, &input, ctx),
            "AddTagsToResource" => tags::add_tags_to_resource(&state, &input, ctx),
            "RemoveTagsFromResource" => tags::remove_tags_from_resource(&state, &input, ctx),
            "ListTagsForResource" => tags::list_tags_for_resource(&state, &input, ctx),
            "PutInventory" => commands::put_inventory(&state, &input, ctx),
            "GetInventory" => commands::get_inventory(&state, &input, ctx),
            "GetInventorySchema" => commands::get_inventory_schema(&state, &input, ctx),
            "SendCommand" => commands::send_command(&state, &input, ctx),
            "ListCommands" => commands::list_commands(&state, &input, ctx),
            "GetCommandInvocation" => commands::get_command_invocation(&state, &input, ctx),

            // Documents
            "CreateDocument" => documents::create_document(&state, &input, ctx),
            "GetDocument" => documents::get_document(&state, &input, ctx),
            "DescribeDocument" => documents::describe_document(&state, &input, ctx),
            "UpdateDocument" => documents::update_document(&state, &input, ctx),
            "DeleteDocument" => documents::delete_document(&state, &input, ctx),
            "ListDocuments" => documents::list_documents(&state, &input, ctx),

            // Associations
            "CreateAssociation" => documents::create_association(&state, &input, ctx),
            "DescribeAssociation" => documents::describe_association(&state, &input, ctx),
            "DeleteAssociation" => documents::delete_association(&state, &input, ctx),
            "ListAssociations" => documents::list_associations(&state, &input, ctx),

            // Maintenance Windows
            "CreateMaintenanceWindow" => documents::create_maintenance_window(&state, &input, ctx),
            "DescribeMaintenanceWindows" => {
                documents::describe_maintenance_windows(&state, &input, ctx)
            }
            "DeleteMaintenanceWindow" => documents::delete_maintenance_window(&state, &input, ctx),
            "GetMaintenanceWindow" => maintenance::get_maintenance_window(&state, &input, ctx),
            "UpdateMaintenanceWindow" => {
                maintenance::update_maintenance_window(&state, &input, ctx)
            }
            "RegisterTargetWithMaintenanceWindow" => {
                maintenance::register_target_with_maintenance_window(&state, &input, ctx)
            }
            "RegisterTaskWithMaintenanceWindow" => {
                maintenance::register_task_with_maintenance_window(&state, &input, ctx)
            }
            "DescribeMaintenanceWindowTargets" => {
                maintenance::describe_maintenance_window_targets(&state, &input, ctx)
            }
            "DescribeMaintenanceWindowTasks" => {
                maintenance::describe_maintenance_window_tasks(&state, &input, ctx)
            }

            // OpsCenter
            "CreateOpsItem" => documents::create_ops_item(&state, &input, ctx),
            "GetOpsItem" => documents::get_ops_item(&state, &input, ctx),
            "UpdateOpsItem" => documents::update_ops_item(&state, &input, ctx),
            "DescribeOpsItems" => documents::describe_ops_items(&state, &input, ctx),

            // Patch Baselines
            "CreatePatchBaseline" => patch_baselines::create_patch_baseline(&state, &input, ctx),
            "GetPatchBaseline" => patch_baselines::get_patch_baseline(&state, &input, ctx),
            "DescribePatchBaselines" => {
                patch_baselines::describe_patch_baselines(&state, &input, ctx)
            }
            "DeletePatchBaseline" => patch_baselines::delete_patch_baseline(&state, &input, ctx),
            "UpdatePatchBaseline" => patch_baselines::update_patch_baseline(&state, &input, ctx),
            "RegisterDefaultPatchBaseline" => {
                patch_baselines::register_default_patch_baseline(&state, &input, ctx)
            }
            "GetDefaultPatchBaseline" => {
                patch_baselines::get_default_patch_baseline(&state, &input, ctx)
            }

            // Automation
            "StartAutomationExecution" => {
                automation::start_automation_execution(&state, &input, ctx)
            }
            "GetAutomationExecution" => automation::get_automation_execution(&state, &input, ctx),
            "DescribeAutomationExecutions" => {
                automation::describe_automation_executions(&state, &input, ctx)
            }
            "StopAutomationExecution" => automation::stop_automation_execution(&state, &input, ctx),

            // Sessions
            "StartSession" => sessions::start_session(&state, &input, ctx),
            "DescribeSessions" => sessions::describe_sessions(&state, &input, ctx),
            "TerminateSession" => sessions::terminate_session(&state, &input, ctx),
            "ResumeSession" => sessions::resume_session(&state, &input, ctx),

            // Resource Data Sync
            "CreateResourceDataSync" => maintenance::create_resource_data_sync(&state, &input, ctx),
            "ListResourceDataSync" => maintenance::list_resource_data_sync(&state, &input, ctx),
            "DeleteResourceDataSync" => maintenance::delete_resource_data_sync(&state, &input, ctx),

            // Misc
            "UpdateAssociation" => maintenance::update_association(&state, &input, ctx),
            "UpdateAssociationStatus" => {
                maintenance::update_association_status(&state, &input, ctx)
            }
            "GetServiceSetting" => maintenance::get_service_setting(&state, &input, ctx),
            "ListInventoryEntries" => maintenance::list_inventory_entries(&state, &input, ctx),
            "ListComplianceSummaries" => {
                maintenance::list_compliance_summaries(&state, &input, ctx)
            }

            // OpsMetadata
            "CreateOpsMetadata" => opsmeta::create_ops_metadata(&state, &input, ctx),
            "GetOpsMetadata" => opsmeta::get_ops_metadata(&state, &input, ctx),
            "UpdateOpsMetadata" => opsmeta::update_ops_metadata(&state, &input, ctx),
            "DeleteOpsMetadata" => opsmeta::delete_ops_metadata(&state, &input, ctx),
            "ListOpsMetadata" => opsmeta::list_ops_metadata(&state, &input, ctx),

            // OpsItem extras
            "DeleteOpsItem" => opsmeta::delete_ops_item(&state, &input, ctx),
            "GetOpsSummary" => opsmeta::get_ops_summary(&state, &input, ctx),
            "ListOpsItemEvents" => opsmeta::list_ops_item_events(&state, &input, ctx),
            "ListOpsItemRelatedItems" => opsmeta::list_ops_item_related_items(&state, &input, ctx),
            "AssociateOpsItemRelatedItem" => {
                opsmeta::associate_ops_item_related_item(&state, &input, ctx)
            }
            "DisassociateOpsItemRelatedItem" => {
                opsmeta::disassociate_ops_item_related_item(&state, &input, ctx)
            }

            // Activations / managed instances
            "CreateActivation" => activations::create_activation(&state, &input, ctx),
            "DeleteActivation" => activations::delete_activation(&state, &input, ctx),
            "DescribeActivations" => activations::describe_activations(&state, &input, ctx),
            "DescribeInstanceInformation" => {
                activations::describe_instance_information(&state, &input, ctx)
            }
            "DescribeInstanceProperties" => {
                activations::describe_instance_properties(&state, &input, ctx)
            }
            "DeregisterManagedInstance" => {
                activations::deregister_managed_instance(&state, &input, ctx)
            }
            "UpdateManagedInstanceRole" => {
                activations::update_managed_instance_role(&state, &input, ctx)
            }

            // Compliance
            "PutComplianceItems" => compliance::put_compliance_items(&state, &input, ctx),
            "ListComplianceItems" => compliance::list_compliance_items(&state, &input, ctx),
            "ListResourceComplianceSummaries" => {
                compliance::list_resource_compliance_summaries(&state, &input, ctx)
            }

            // Resource policies
            "PutResourcePolicy" => policies::put_resource_policy(&state, &input, ctx),
            "GetResourcePolicies" => policies::get_resource_policies(&state, &input, ctx),
            "DeleteResourcePolicy" => policies::delete_resource_policy(&state, &input, ctx),

            // Maintenance window extras
            "DeregisterTargetFromMaintenanceWindow" => {
                maintenance::deregister_target_from_maintenance_window(&state, &input, ctx)
            }
            "DeregisterTaskFromMaintenanceWindow" => {
                maintenance::deregister_task_from_maintenance_window(&state, &input, ctx)
            }
            "GetMaintenanceWindowTask" => {
                maintenance::get_maintenance_window_task(&state, &input, ctx)
            }
            "UpdateMaintenanceWindowTarget" => {
                maintenance::update_maintenance_window_target(&state, &input, ctx)
            }
            "UpdateMaintenanceWindowTask" => {
                maintenance::update_maintenance_window_task(&state, &input, ctx)
            }

            // Patches extras
            "DescribeInstancePatches" => {
                maintenance::describe_instance_patches(&state, &input, ctx)
            }
            "DescribeInstancePatchStates" => {
                maintenance::describe_instance_patch_states(&state, &input, ctx)
            }
            "DescribeAvailablePatches" => {
                maintenance::describe_available_patches(&state, &input, ctx)
            }
            "DescribePatchGroups" => maintenance::describe_patch_groups(&state, &input, ctx),
            "DescribePatchGroupState" => {
                maintenance::describe_patch_group_state(&state, &input, ctx)
            }
            "RegisterPatchBaselineForPatchGroup" => {
                maintenance::register_patch_baseline_for_patch_group(&state, &input, ctx)
            }
            "DeregisterPatchBaselineForPatchGroup" => {
                maintenance::deregister_patch_baseline_for_patch_group(&state, &input, ctx)
            }

            // Association extras
            "DescribeInstanceAssociationsStatus" => {
                maintenance::describe_instance_associations_status(&state, &input, ctx)
            }
            "DescribeEffectiveInstanceAssociations" => {
                maintenance::describe_effective_instance_associations(&state, &input, ctx)
            }
            "DescribeAssociationExecutions" => {
                maintenance::describe_association_executions(&state, &input, ctx)
            }
            "DescribeAssociationExecutionTargets" => {
                maintenance::describe_association_execution_targets(&state, &input, ctx)
            }
            "ListAssociationVersions" => {
                maintenance::list_association_versions(&state, &input, ctx)
            }

            // Service settings
            "UpdateServiceSetting" => maintenance::update_service_setting(&state, &input, ctx),
            "ResetServiceSetting" => maintenance::reset_service_setting(&state, &input, ctx),

            // Automation extras
            "DescribeAutomationStepExecutions" => {
                maintenance::describe_automation_step_executions(&state, &input, ctx)
            }
            "SendAutomationSignal" => maintenance::send_automation_signal(&state, &input, ctx),

            // Commands extras
            "CancelCommand" => maintenance::cancel_command(&state, &input, ctx),
            "ListCommandInvocations" => maintenance::list_command_invocations(&state, &input, ctx),

            // Parameter / document extras
            "UnlabelParameterVersion" => {
                maintenance::unlabel_parameter_version(&state, &input, ctx)
            }
            "DeleteInventory" => maintenance::delete_inventory(&state, &input, ctx),
            "UpdateResourceDataSync" => maintenance::update_resource_data_sync(&state, &input, ctx),
            "GetConnectionStatus" => maintenance::get_connection_status(&state, &input, ctx),
            "GetCalendarState" => maintenance::get_calendar_state(&state, &input, ctx),
            "UpdateDocumentDefaultVersion" => {
                maintenance::update_document_default_version(&state, &input, ctx)
            }
            "ListDocumentVersions" => maintenance::list_document_versions(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
