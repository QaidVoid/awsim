use std::path::Path;
use std::sync::Arc;

use awsim_core::{
    AccountRegionStore, AwsError, BlobInventory, Protocol, RequestContext, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use crate::SqliteStore;
use crate::operations::{filters, log_events, log_groups, log_streams};
use crate::state::LogsState;

/// The CloudWatch Logs service handler. Log events live in
/// `sqlite_store` (one DB per process); group/stream metadata
/// stays in `LogsState` per (account, region).
pub struct CloudWatchLogsService {
    store: AccountRegionStore<LogsState>,
    sqlite_store: Arc<SqliteStore>,
    /// Holds the per-process tempdir when running without
    /// `--data-dir` so the `.db` files are removed on graceful
    /// shutdown via Drop.
    _tempdir: Option<tempfile::TempDir>,
}

impl CloudWatchLogsService {
    pub const GROUPS: &'static [&'static str] = &[];

    /// Ephemeral in-process store. Useful for tests and `awsim` runs
    /// without `--data-dir` — files live in a `TempDir` cleaned up
    /// on shutdown.
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("awsim-cwl-")
            .tempdir()
            .expect("creating ephemeral CWL tempdir should not fail");
        let path = dir.path().join("cloudwatch-logs.db");
        let sqlite_store = Arc::new(
            SqliteStore::open(&path).expect("opening ephemeral CWL sqlite store should not fail"),
        );
        Self {
            store: AccountRegionStore::new(),
            sqlite_store,
            _tempdir: Some(dir),
        }
    }

    /// Persistent store rooted at `{dir}/cloudwatch-logs.db`. Created
    /// alongside DynamoDB's `dynamodb.db` so a single `--data-dir`
    /// captures every persistent service.
    pub fn with_data_dir(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir).unwrap_or_else(|e| {
            panic!(
                "creating CloudWatch Logs data dir {} failed: {e}",
                dir.display()
            )
        });
        let path = dir.join("cloudwatch-logs.db");
        let sqlite_store = Arc::new(SqliteStore::open(&path).unwrap_or_else(|e| {
            panic!(
                "opening persistent CWL sqlite store at {} failed: {e}",
                path.display()
            )
        }));
        Self {
            store: AccountRegionStore::new(),
            sqlite_store,
            _tempdir: None,
        }
    }

    /// Legacy shim — `--max-blob-bytes` used to size the body store
    /// that backed log events. Now that events live in SQLite the
    /// flag is a no-op for CloudWatch Logs; kept on the type so the
    /// `awsim` binary's wiring doesn't have to special-case it.
    pub fn with_max_blob_bytes(self, _bytes: u64) -> Self {
        self
    }

    pub fn store(&self) -> AccountRegionStore<LogsState> {
        self.store.clone()
    }

    /// Return the path to the sqlite tempdir (when this instance owns
    /// one) so the awsim binary can clean it up on `process::exit`.
    pub fn tempdir_path(&self) -> Option<&Path> {
        self._tempdir.as_ref().map(|d| d.path())
    }

    /// Internal Arc to the sqlite store — exposed so the awsim
    /// binary's `/_awsim/storage/sqlite` endpoint can surface row
    /// counts + file size.
    pub fn sqlite_store_handle(&self) -> Option<Arc<SqliteStore>> {
        Some(Arc::clone(&self.sqlite_store))
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<LogsState> {
        let state = self.store.get(&ctx.account_id, &ctx.region);
        state.set_sqlite(Arc::clone(&self.sqlite_store));
        state
    }
}

impl Default for CloudWatchLogsService {
    fn default() -> Self {
        Self::new()
    }
}

impl BlobInventory for CloudWatchLogsService {
    /// Log events used to live in a body store. Now they're rows in
    /// SQLite, so the orphan-blob inventory is empty for CWL.
    fn known_blobs(&self) -> Vec<(String, String, String)> {
        Vec::new()
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

        let state = self.get_state(ctx);

        match operation {
            // Log Groups
            "CreateLogGroup" => log_groups::create_log_group(&state, &input, ctx),
            "DeleteLogGroup" => log_groups::delete_log_group(&state, &input, ctx),
            "DescribeLogGroups" => log_groups::describe_log_groups(&state, &input, ctx),
            "PutRetentionPolicy" => log_groups::put_retention_policy(&state, &input, ctx),
            "DeleteRetentionPolicy" => log_groups::delete_retention_policy(&state, &input, ctx),
            "AssociateKmsKey" => log_groups::associate_kms_key(&state, &input, ctx),
            "DisassociateKmsKey" => log_groups::disassociate_kms_key(&state, &input, ctx),
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

            // Resource-based tagging (newer API names)
            "TagResource" => filters::tag_resource(&state, &input, ctx),
            "UntagResource" => filters::untag_resource(&state, &input, ctx),
            "ListTagsForResource" => filters::list_tags_for_resource(&state, &input, ctx),

            // Subscription Filters
            "PutSubscriptionFilter" => filters::put_subscription_filter(&state, &input, ctx),
            "DescribeSubscriptionFilters" => {
                filters::describe_subscription_filters(&state, &input, ctx)
            }
            "DeleteSubscriptionFilter" => filters::delete_subscription_filter(&state, &input, ctx),

            // Metric Filters
            "PutMetricFilter" => filters::put_metric_filter(&state, &input, ctx),
            "DescribeMetricFilters" => filters::describe_metric_filters(&state, &input, ctx),
            "DeleteMetricFilter" => filters::delete_metric_filter(&state, &input, ctx),

            // Query Definitions
            "PutQueryDefinition" => filters::put_query_definition(&state, &input, ctx),
            "DescribeQueryDefinitions" => filters::describe_query_definitions(&state, &input, ctx),
            "DeleteQueryDefinition" => filters::delete_query_definition(&state, &input, ctx),

            // Insights Queries
            "StartQuery" => filters::start_query(&state, &input, ctx),
            "GetQueryResults" => filters::get_query_results(&state, &input, ctx),
            "StopQuery" => filters::stop_query(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        self.store.snapshot_to_bytes()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        // Group/stream metadata comes from JSON; events themselves
        // live in the SQLite file alongside the rest of awsim's
        // persistent state, so there's no second-pass replay anymore.
        self.store.restore_from_bytes(data)
    }
}
