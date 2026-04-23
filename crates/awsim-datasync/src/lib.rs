mod operations;
mod state;

pub use state::DataSyncState;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

pub struct DataSyncService {
    store: AccountRegionStore<DataSyncState>,
}

impl DataSyncService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for DataSyncService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for DataSyncService {
    fn service_name(&self) -> &str {
        "datasync"
    }

    fn signing_name(&self) -> &str {
        "datasync"
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
        debug!(operation, "DataSync request");
        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            "CreateLocationS3" => operations::locations::create_location_s3(&state, &input, ctx),
            "CreateLocationNfs" => operations::locations::create_location_nfs(&state, &input, ctx),
            "CreateLocationSmb" => operations::locations::create_location_smb(&state, &input, ctx),
            "CreateLocationEfs" => operations::locations::create_location_efs(&state, &input, ctx),
            "DescribeLocationS3" => operations::locations::describe_location_s3(&state, &input, ctx),
            "ListLocations" => operations::locations::list_locations(&state, &input, ctx),
            "DeleteLocation" => operations::locations::delete_location(&state, &input, ctx),
            "CreateTask" => operations::tasks::create_task(&state, &input, ctx),
            "DescribeTask" => operations::tasks::describe_task(&state, &input, ctx),
            "ListTasks" => operations::tasks::list_tasks(&state, &input, ctx),
            "UpdateTask" => operations::tasks::update_task(&state, &input, ctx),
            "DeleteTask" => operations::tasks::delete_task(&state, &input, ctx),
            "StartTaskExecution" => operations::executions::start_task_execution(&state, &input, ctx),
            "DescribeTaskExecution" => operations::executions::describe_task_execution(&state, &input, ctx),
            "ListTaskExecutions" => operations::executions::list_task_executions(&state, &input, ctx),
            "CancelTaskExecution" => operations::executions::cancel_task_execution(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
