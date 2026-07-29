use std::sync::Arc;

use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, S3ObjectReader, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use crate::operations::{activities, executions, state_machines, tags, task_callbacks};
use crate::state::StepFunctionsState;

/// The Step Functions service handler.
pub struct StepFunctionsService {
    store: AccountRegionStore<StepFunctionsState>,
    /// In-process S3 reader used by Distributed Map `ItemReader`.
    s3_reader: Option<Arc<dyn S3ObjectReader>>,
}

impl StepFunctionsService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
            s3_reader: None,
        }
    }

    /// Wire the in-process S3 reader so Distributed Map can read CSV
    /// inventories from the embedded S3.
    pub fn with_s3_reader(mut self, reader: Arc<dyn S3ObjectReader>) -> Self {
        self.s3_reader = Some(reader);
        self
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

            // Executions. Install the S3 reader context for the synchronous
            // interpreter (Distributed Map ItemReader) and clear it after.
            "StartExecution" => {
                crate::asl::set_s3_context(self.s3_reader.clone(), &ctx.account_id, &ctx.region);
                let result = executions::start_execution(&state, &input, ctx);
                crate::asl::clear_s3_context();
                result
            }
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

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut buckets = Vec::new();
        for ((account_id, region), state) in self.store.iter_all() {
            buckets.push(SfnBucketSnapshot {
                account_id,
                region,
                state_machines: state
                    .state_machines
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect(),
                executions: state
                    .executions
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect(),
                activities: state
                    .activities
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect(),
                pending_tokens: state
                    .pending_tokens
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect(),
            });
        }
        serde_json::to_vec(&SfnSnapshot { buckets }).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snapshot: SfnSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        for bucket in snapshot.buckets {
            let state = self.store.get(&bucket.account_id, &bucket.region);
            state.state_machines.clear();
            state.executions.clear();
            state.activities.clear();
            state.pending_tokens.clear();
            for (k, v) in bucket.state_machines {
                state.state_machines.insert(k, v);
            }
            for (k, v) in bucket.executions {
                state.executions.insert(k, v);
            }
            for (k, v) in bucket.activities {
                state.activities.insert(k, v);
            }
            for (k, v) in bucket.pending_tokens {
                state.pending_tokens.insert(k, v);
            }
        }
        Ok(())
    }
}

/// One (account, region) pair's Step Functions state on disk.
///
/// `pending_tokens` is included so a `.waitForTaskToken` execution
/// suspended at shutdown can still be answered after a restart, rather
/// than being stranded with no way to resume.
#[derive(serde::Serialize, serde::Deserialize)]
struct SfnBucketSnapshot {
    account_id: String,
    region: String,
    state_machines: Vec<(String, crate::state::StateMachine)>,
    executions: Vec<(String, crate::state::Execution)>,
    activities: Vec<(String, crate::state::Activity)>,
    pending_tokens: Vec<(String, crate::state::PendingTask)>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SfnSnapshot {
    buckets: Vec<SfnBucketSnapshot>,
}
