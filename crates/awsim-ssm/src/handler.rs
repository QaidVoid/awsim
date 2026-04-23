use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{
    automation, commands, documents, maintenance, parameters, patch_baselines, sessions, tags,
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
            "StopAutomationExecution" => {
                automation::stop_automation_execution(&state, &input, ctx)
            }

            // Sessions
            "StartSession" => sessions::start_session(&state, &input, ctx),
            "DescribeSessions" => sessions::describe_sessions(&state, &input, ctx),
            "TerminateSession" => sessions::terminate_session(&state, &input, ctx),
            "ResumeSession" => sessions::resume_session(&state, &input, ctx),

            // Resource Data Sync
            "CreateResourceDataSync" => {
                maintenance::create_resource_data_sync(&state, &input, ctx)
            }
            "ListResourceDataSync" => maintenance::list_resource_data_sync(&state, &input, ctx),
            "DeleteResourceDataSync" => {
                maintenance::delete_resource_data_sync(&state, &input, ctx)
            }

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

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
