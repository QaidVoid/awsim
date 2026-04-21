mod operations;
mod state;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::CloudWatchState;

/// The AWSim CloudWatch Metrics service handler.
///
/// Supports the `AwsQuery` protocol (Action= form-encoded requests),
/// signing name `monitoring`.
pub struct CloudWatchMetricsService {
    store: AccountRegionStore<CloudWatchState>,
}

impl CloudWatchMetricsService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<CloudWatchState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for CloudWatchMetricsService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for CloudWatchMetricsService {
    fn service_name(&self) -> &str {
        "monitoring"
    }

    fn signing_name(&self) -> &str {
        "monitoring"
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
        debug!(operation = %operation, "CloudWatch Metrics operation");
        let state = self.get_state(ctx);

        match operation {
            // Metrics
            "PutMetricData" => operations::metrics::put_metric_data(&state, &input, ctx),
            "ListMetrics" => operations::metrics::list_metrics(&state, &input, ctx),
            "GetMetricStatistics" => {
                operations::metrics::get_metric_statistics(&state, &input, ctx)
            }
            "GetMetricData" => operations::metrics::get_metric_data(&state, &input, ctx),

            // Alarms
            "PutMetricAlarm" => operations::alarms::put_metric_alarm(&state, &input, ctx),
            "DescribeAlarms" => operations::alarms::describe_alarms(&state, &input, ctx),
            "DeleteAlarms" => operations::alarms::delete_alarms(&state, &input, ctx),
            "SetAlarmState" => operations::alarms::set_alarm_state(&state, &input, ctx),

            // Dashboards
            "PutDashboard" => operations::dashboards::put_dashboard(&state, &input, ctx),
            "GetDashboard" => operations::dashboards::get_dashboard(&state, &input, ctx),
            "ListDashboards" => operations::dashboards::list_dashboards(&state, &input, ctx),
            "DeleteDashboards" => operations::dashboards::delete_dashboards(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
