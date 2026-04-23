use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{commands, documents, parameters, tags};
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

            // OpsCenter
            "CreateOpsItem" => documents::create_ops_item(&state, &input, ctx),
            "GetOpsItem" => documents::get_ops_item(&state, &input, ctx),
            "UpdateOpsItem" => documents::update_ops_item(&state, &input, ctx),
            "DescribeOpsItems" => documents::describe_ops_items(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
