use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{log_events, log_groups, log_streams};
use crate::state::LogsState;

/// The CloudWatch Logs service handler.
pub struct CloudWatchLogsService {
    store: AccountRegionStore<LogsState>,
}

impl CloudWatchLogsService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for CloudWatchLogsService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ServiceHandler for CloudWatchLogsService {
    fn service_name(&self) -> &str {
        "logs"
    }

    fn signing_name(&self) -> &str {
        "logs"
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
        debug!(operation = %operation, "CloudWatch Logs operation");

        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            // Log Groups
            "CreateLogGroup" => log_groups::create_log_group(&state, &input, ctx),
            "DeleteLogGroup" => log_groups::delete_log_group(&state, &input, ctx),
            "DescribeLogGroups" => log_groups::describe_log_groups(&state, &input, ctx),
            "PutRetentionPolicy" => log_groups::put_retention_policy(&state, &input, ctx),
            "DeleteRetentionPolicy" => log_groups::delete_retention_policy(&state, &input, ctx),
            "TagLogGroup" => log_groups::tag_log_group(&state, &input, ctx),
            "UntagLogGroup" => log_groups::untag_log_group(&state, &input, ctx),
            "ListTagsLogGroup" => log_groups::list_tags_log_group(&state, &input, ctx),

            // Log Streams
            "CreateLogStream" => log_streams::create_log_stream(&state, &input, ctx),
            "DeleteLogStream" => log_streams::delete_log_stream(&state, &input, ctx),
            "DescribeLogStreams" => log_streams::describe_log_streams(&state, &input, ctx),

            // Log Events
            "PutLogEvents" => log_events::put_log_events(&state, &input, ctx),
            "GetLogEvents" => log_events::get_log_events(&state, &input, ctx),
            "FilterLogEvents" => log_events::filter_log_events(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
