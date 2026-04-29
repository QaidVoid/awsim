mod operations;
pub mod sqlite_store;
mod state;

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

pub use sqlite_store::{MetricDatumRow, SqliteStore};
use state::CloudWatchState;

/// The AWSim CloudWatch Metrics service handler.
///
/// Supports the `AwsQuery` protocol (Action= form-encoded requests),
/// signing name `monitoring`. Datapoints live in `sqlite_store`;
/// alarms + dashboards stay in DashMap on `CloudWatchState`.
pub struct CloudWatchMetricsService {
    store: AccountRegionStore<CloudWatchState>,
    sqlite_store: Arc<SqliteStore>,
    /// Owns the per-process tempdir when running without
    /// `--data-dir` so the `.db` file is removed on graceful exit.
    _tempdir: Option<tempfile::TempDir>,
}

impl CloudWatchMetricsService {
    /// Ephemeral in-process store. Files live in a `TempDir` cleaned
    /// up by the awsim shutdown handler.
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("awsim-cwm-")
            .tempdir()
            .expect("creating ephemeral CWM tempdir should not fail");
        let path = dir.path().join("cloudwatch-metrics.db");
        let sqlite_store = Arc::new(
            SqliteStore::open(&path).expect("opening ephemeral CWM sqlite store should not fail"),
        );
        Self {
            store: AccountRegionStore::new(),
            sqlite_store,
            _tempdir: Some(dir),
        }
    }

    /// Persistent store at `{dir}/cloudwatch-metrics.db`.
    pub fn with_data_dir(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir).unwrap_or_else(|e| {
            panic!(
                "creating CloudWatch Metrics data dir {} failed: {e}",
                dir.display()
            )
        });
        let path = dir.join("cloudwatch-metrics.db");
        let sqlite_store = Arc::new(SqliteStore::open(&path).unwrap_or_else(|e| {
            panic!(
                "opening persistent CWM sqlite store at {} failed: {e}",
                path.display()
            )
        }));
        Self {
            store: AccountRegionStore::new(),
            sqlite_store,
            _tempdir: None,
        }
    }

    /// Path to the underlying tempdir (when one is owned), so the
    /// awsim binary can clean it up before `process::exit`.
    pub fn tempdir_path(&self) -> Option<&Path> {
        self._tempdir.as_ref().map(|d| d.path())
    }

    /// Internal Arc to the sqlite store — exposed so the awsim
    /// binary's `/_awsim/storage/sqlite` endpoint can report stats.
    pub fn sqlite_store_handle(&self) -> Option<Arc<SqliteStore>> {
        Some(Arc::clone(&self.sqlite_store))
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<CloudWatchState> {
        let state = self.store.get(&ctx.account_id, &ctx.region);
        state.set_sqlite(Arc::clone(&self.sqlite_store));
        state
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
