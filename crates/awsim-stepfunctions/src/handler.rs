use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{activities, executions, state_machines, tags, task_callbacks};
use crate::state::StepFunctionsState;

/// The Step Functions service handler.
pub struct StepFunctionsService {
    store: AccountRegionStore<StepFunctionsState>,
}

impl StepFunctionsService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for StepFunctionsService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ServiceHandler for StepFunctionsService {
    fn service_name(&self) -> &str {
        "states"
    }

    fn signing_name(&self) -> &str {
        "states"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_0
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation = %operation, "Step Functions operation");

        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            // State machines
            "CreateStateMachine" => state_machines::create_state_machine(&state, &input, ctx),
            "DeleteStateMachine" => state_machines::delete_state_machine(&state, &input, ctx),
            "DescribeStateMachine" => state_machines::describe_state_machine(&state, &input, ctx),
            "ListStateMachines" => state_machines::list_state_machines(&state, &input, ctx),
            "UpdateStateMachine" => state_machines::update_state_machine(&state, &input, ctx),

            // Executions
            "StartExecution" => executions::start_execution(&state, &input, ctx),
            "StopExecution" => executions::stop_execution(&state, &input, ctx),
            "DescribeExecution" => executions::describe_execution(&state, &input, ctx),
            "ListExecutions" => executions::list_executions(&state, &input, ctx),
            "GetExecutionHistory" => executions::get_execution_history(&state, &input, ctx),
            "DescribeStateMachineForExecution" => {
                task_callbacks::describe_state_machine_for_execution(&state, &input, ctx)
            }

            // Tags
            "TagResource" => tags::tag_resource(&state, &input, ctx),
            "UntagResource" => tags::untag_resource(&state, &input, ctx),
            "ListTagsForResource" => tags::list_tags_for_resource(&state, &input, ctx),

            // Activities
            "CreateActivity" => activities::create_activity(&state, &input, ctx),
            "DeleteActivity" => activities::delete_activity(&state, &input, ctx),
            "DescribeActivity" => activities::describe_activity(&state, &input, ctx),
            "ListActivities" => activities::list_activities(&state, &input, ctx),

            // Task token callbacks
            "SendTaskSuccess" => task_callbacks::send_task_success(&state, &input, ctx),
            "SendTaskFailure" => task_callbacks::send_task_failure(&state, &input, ctx),
            "SendTaskHeartbeat" => task_callbacks::send_task_heartbeat(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
