mod operations;
mod state;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::CloudTrailState;

pub struct CloudTrailService {
    store: AccountRegionStore<CloudTrailState>,
}

impl CloudTrailService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for CloudTrailService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for CloudTrailService {
    fn service_name(&self) -> &str {
        "cloudtrail"
    }

    fn signing_name(&self) -> &str {
        "cloudtrail"
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
        debug!(operation = %operation, "CloudTrail operation");
        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            "CreateTrail" => operations::trails::create_trail(&state, &input, ctx),
            "DescribeTrails" => operations::trails::describe_trails(&state, &input, ctx),
            "DeleteTrail" => operations::trails::delete_trail(&state, &input, ctx),
            "UpdateTrail" => operations::trails::update_trail(&state, &input, ctx),
            "StartLogging" => operations::trails::start_logging(&state, &input, ctx),
            "StopLogging" => operations::trails::stop_logging(&state, &input, ctx),
            "GetTrailStatus" => operations::trails::get_trail_status(&state, &input, ctx),
            "GetEventSelectors" => operations::selectors::get_event_selectors(&state, &input, ctx),
            "PutEventSelectors" => operations::selectors::put_event_selectors(&state, &input, ctx),
            "ListTrails" => operations::trails::list_trails(&state, &input, ctx),
            "LookupEvents" => operations::trails::lookup_events(&state, &input, ctx),
            "PutInsightSelectors" => {
                operations::selectors::put_insight_selectors(&state, &input, ctx)
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
