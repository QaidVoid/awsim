use std::path::Path;
use std::sync::Arc;

use awsim_core::{
    AccountRegionStore, AwsError, BodyStore, Protocol, RequestContext, ServiceHandler,
};
use serde_json::Value;
use tracing::{debug, warn};

use crate::operations::{filters, log_events, log_groups, log_streams};
use crate::state::{LogEvent, LogsState};

/// The CloudWatch Logs service handler.
pub struct CloudWatchLogsService {
    store: AccountRegionStore<LogsState>,
    body_store: Option<Arc<BodyStore>>,
}

impl CloudWatchLogsService {
    pub const GROUPS: &'static [&'static str] = &["cloudwatch-logs"];

    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
            body_store: None,
        }
    }

    pub fn with_data_dir(dir: impl AsRef<Path>) -> Self {
        Self {
            store: AccountRegionStore::new(),
            body_store: Some(Arc::new(BodyStore::new(dir.as_ref().to_path_buf()))),
        }
    }

    pub fn with_max_blob_bytes(mut self, bytes: u64) -> Self {
        if let Some(bs) = self.body_store.take() {
            let root = bs.root().to_path_buf();
            self.body_store = Some(Arc::new(BodyStore::new(root).with_max_size(bytes)));
        }
        self
    }

    pub fn store(&self) -> AccountRegionStore<LogsState> {
        self.store.clone()
    }

    pub fn body_store(&self) -> Option<&Arc<BodyStore>> {
        self.body_store.as_ref()
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<LogsState> {
        let state = self.store.get(&ctx.account_id, &ctx.region);
        if let Some(bs) = &self.body_store {
            state.set_body_store(Arc::clone(bs));
        }
        state
    }

    fn rebind_and_replay(&self) {
        let Some(bs) = &self.body_store else {
            return;
        };
        for (_, state) in self.store.iter_all() {
            state.set_body_store(Arc::clone(bs));
            for group_entry in state.log_groups.iter() {
                let group_name = group_entry.key().clone();
                let group = group_entry.value();
                for stream_entry in group.streams.iter() {
                    let stream_name = stream_entry.key().clone();
                    let stream = stream_entry.value();
                    let events = match bs.read_blob("cloudwatch-logs", &group_name, &stream_name) {
                        Ok(b) => b,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                        Err(e) => {
                            warn!(
                                log_group = %group_name,
                                log_stream = %stream_name,
                                error = %e,
                                "Failed to read persisted log stream during restore"
                            );
                            continue;
                        }
                    };
                    let text = match std::str::from_utf8(&events) {
                        Ok(s) => s,
                        Err(e) => {
                            warn!(
                                log_group = %group_name,
                                log_stream = %stream_name,
                                error = %e,
                                "Persisted log stream is not valid utf-8"
                            );
                            continue;
                        }
                    };
                    let mut restored: Vec<LogEvent> = Vec::new();
                    for (lineno, line) in text.lines().enumerate() {
                        if line.trim().is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<Value>(line) {
                            Ok(v) => {
                                let ts = v.get("ts").and_then(|x| x.as_u64()).unwrap_or(0);
                                let msg = v
                                    .get("msg")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let ing = v.get("ing").and_then(|x| x.as_u64()).unwrap_or(0);
                                restored.push(LogEvent {
                                    timestamp: ts,
                                    message: msg,
                                    ingestion_time: ing,
                                });
                            }
                            Err(e) => {
                                warn!(
                                    log_group = %group_name,
                                    log_stream = %stream_name,
                                    line = lineno,
                                    error = %e,
                                    "Skipping malformed JSONL line during restore"
                                );
                            }
                        }
                    }
                    restored.sort_by_key(|e| e.timestamp);
                    *stream.events.write().unwrap() = restored;
                }
            }
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

        let state = self.get_state(ctx);

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
        self.store.restore_from_bytes(data)?;
        self.rebind_and_replay();
        Ok(())
    }
}
