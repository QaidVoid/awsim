mod error;
mod ids;
mod operations;
mod state;
mod template;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::CloudFormationState;

/// The AWSim CloudFormation service handler.
pub struct CloudFormationService {
    store: AccountRegionStore<CloudFormationState>,
}

impl CloudFormationService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<CloudFormationState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for CloudFormationService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for CloudFormationService {
    fn service_name(&self) -> &str {
        "cloudformation"
    }

    fn signing_name(&self) -> &str {
        "cloudformation"
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
        debug!(operation, "CloudFormation request");
        let state = self.get_state(ctx);

        match operation {
            // Stacks
            "CreateStack" => operations::stacks::create_stack(&state, &input, ctx),
            "DeleteStack" => operations::stacks::delete_stack(&state, &input, ctx),
            "UpdateStack" => operations::stacks::update_stack(&state, &input, ctx),
            "DescribeStacks" => operations::stacks::describe_stacks(&state, &input),
            "DescribeStackEvents" => operations::stacks::describe_stack_events(&state, &input),
            "DescribeStackResources" => {
                operations::stacks::describe_stack_resources(&state, &input)
            }
            "DescribeStackResource" => operations::stacks::describe_stack_resource(&state, &input),
            "ListStacks" => operations::stacks::list_stacks(&state, &input),
            "ListStackResources" => operations::stacks::list_stack_resources(&state, &input),
            "GetTemplate" => operations::stacks::get_template(&state, &input),
            "GetTemplateSummary" => operations::stacks::get_template_summary(&state, &input),
            "ValidateTemplate" => operations::stacks::validate_template(&state, &input),
            "UpdateTerminationProtection" => {
                operations::stacks::update_termination_protection(&state, &input, ctx)
            }
            "SetStackPolicy" => operations::stacks::set_stack_policy(&state, &input),
            "GetStackPolicy" => operations::stacks::get_stack_policy(&state, &input),

            // Exports / Imports
            "ListExports" => operations::stacks::list_exports(&state, &input),
            "ListImports" => operations::stacks::list_imports(&state, &input),

            // Tagging
            "TagResource" => operations::stacks::tag_resource(&state, &input),
            "UntagResource" => operations::stacks::untag_resource(&state, &input),

            // Signals / Cost
            "SignalResource" => operations::stacks::signal_resource(&state, &input),
            "EstimateTemplateCost" => operations::stacks::estimate_template_cost(&state, &input),

            // Change Sets
            "CreateChangeSet" => operations::change_sets::create_change_set(&state, &input, ctx),
            "ExecuteChangeSet" => operations::change_sets::execute_change_set(&state, &input, ctx),
            "DeleteChangeSet" => operations::change_sets::delete_change_set(&state, &input),
            "DescribeChangeSet" => operations::change_sets::describe_change_set(&state, &input),
            "ListChangeSets" => operations::change_sets::list_change_sets(&state, &input),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
