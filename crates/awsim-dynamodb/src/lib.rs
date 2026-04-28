mod expressions;
mod keys;
mod operations;
mod sqlite_store;
mod state;

pub use sqlite_store::{MAX_GSI_SLOTS, SqliteStore};

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::{debug, warn};

use state::{DynamoState, DynamoStateSnapshot, Table};

/// The AWSim DynamoDB service handler.
///
/// Holds two stores during the in-memory → SQLite transition:
///   * `store` — the legacy in-memory `DashMap` per (account, region).
///     Reads still go here; writes are mirrored to SQLite (stage 2 dual-write).
///   * `sqlite` — the persistent backing store that Query/Scan/etc. will
///     migrate to in subsequent stages. When AWSim is started without
///     `--data-dir` we open it on a per-process temp file so behaviour
///     is consistent in tests and ephemeral runs.
pub struct DynamoDbService {
    store: AccountRegionStore<DynamoState>,
    sqlite: Arc<SqliteStore>,
}

impl DynamoDbService {
    /// Ephemeral in-process store. Useful for tests and `awsim` runs
    /// that don't pass `--data-dir` (the SQLite file goes under
    /// `std::env::temp_dir()` and is cleaned up by the OS).
    pub fn new() -> Self {
        let id = uuid::Uuid::new_v4();
        let path = std::env::temp_dir().join(format!("awsim-ddb-{id}.db"));
        let sqlite = SqliteStore::open(path)
            .expect("opening ephemeral DynamoDB sqlite store should not fail");
        Self {
            store: AccountRegionStore::new(),
            sqlite: Arc::new(sqlite),
        }
    }

    /// Persistent store rooted at `{dir}/dynamodb.db`. Mirrors the
    /// `with_data_dir` convention used by the other AWSim services so
    /// the `awsim` binary can wire it the same way.
    pub fn with_data_dir(dir: impl AsRef<Path>) -> Self {
        let path = dir.as_ref().join("dynamodb.db");
        let sqlite = SqliteStore::open(path)
            .expect("opening persistent DynamoDB sqlite store should not fail");
        Self {
            store: AccountRegionStore::new(),
            sqlite: Arc::new(sqlite),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<DynamoState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for DynamoDbService {
    fn default() -> Self {
        Self::new()
    }
}

/// Run a sync DynamoDB op (which may touch SQLite) on tokio's blocking
/// pool so we don't stall worker threads on rusqlite IO. Cheap when no
/// IO actually happens — the blocking pool reuses threads.
async fn run_blocking<F>(f: F) -> Result<Value, AwsError>
where
    F: FnOnce() -> Result<Value, AwsError> + Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(res) => res,
        Err(join_err) => {
            warn!(error = %join_err, "DynamoDB blocking task panicked");
            Err(AwsError::internal(format!(
                "DynamoDB worker join error: {join_err}"
            )))
        }
    }
}

#[async_trait]
impl ServiceHandler for DynamoDbService {
    fn service_name(&self) -> &str {
        "dynamodb"
    }

    fn signing_name(&self) -> &str {
        "dynamodb"
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
        debug!(operation, "DynamoDB request");
        let state = self.get_state(ctx);

        match operation {
            // Table management
            "CreateTable" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || {
                    operations::table::create_table(&state, &sqlite, &input, &ctx)
                })
                .await
            }
            "DeleteTable" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || {
                    operations::table::delete_table(&state, &sqlite, &input, &ctx)
                })
                .await
            }
            "DescribeTable" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || {
                    operations::table::describe_table(&state, &sqlite, &input, &ctx)
                })
                .await
            }
            "ListTables" => operations::table::list_tables(&state, &input, ctx),
            "UpdateTable" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || {
                    operations::table::update_table(&state, &sqlite, &input, &ctx)
                })
                .await
            }

            // Endpoint discovery
            "DescribeEndpoints" => operations::table::describe_endpoints(&state, &input, ctx),

            // TTL
            "DescribeTimeToLive" => operations::table::describe_time_to_live(&state, &input, ctx),
            "UpdateTimeToLive" => operations::table::update_time_to_live(&state, &input, ctx),

            // Continuous Backups
            "DescribeContinuousBackups" => {
                operations::table::describe_continuous_backups(&state, &input, ctx)
            }
            "UpdateContinuousBackups" => {
                operations::backup::update_continuous_backups(&state, &input, ctx)
            }

            // Tagging
            "TagResource" => operations::table::tag_resource(&state, &input, ctx),
            "UntagResource" => operations::table::untag_resource(&state, &input, ctx),
            "ListTagsOfResource" => operations::table::list_tags_of_resource(&state, &input, ctx),

            // Item operations — dual-write to SQLite, so they go through
            // the blocking pool to avoid stalling tokio workers on rusqlite IO.
            "PutItem" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || operations::item::put_item(&state, &sqlite, &input, &ctx))
                    .await
            }
            "GetItem" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || operations::item::get_item(&state, &sqlite, &input, &ctx))
                    .await
            }
            "DeleteItem" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || {
                    operations::item::delete_item(&state, &sqlite, &input, &ctx)
                })
                .await
            }
            "UpdateItem" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || {
                    operations::item::update_item(&state, &sqlite, &input, &ctx)
                })
                .await
            }

            // Query & Scan — read items from SQLite, evaluate filters in Rust.
            "Query" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || operations::query::query(&state, &sqlite, &input, &ctx))
                    .await
            }
            "Scan" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || operations::query::scan(&state, &sqlite, &input, &ctx))
                    .await
            }

            // Batch operations
            "BatchGetItem" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || {
                    operations::batch::batch_get_item(&state, &sqlite, &input, &ctx)
                })
                .await
            }
            "BatchWriteItem" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || {
                    operations::batch::batch_write_item(&state, &sqlite, &input, &ctx)
                })
                .await
            }

            // Transactions — sqlite-backed (best-effort consistency for now;
            // stage 5 wraps writes in a single sqlite transaction).
            "TransactGetItems" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || {
                    operations::transact::transact_get_items(&state, &sqlite, &input, &ctx)
                })
                .await
            }
            "TransactWriteItems" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || {
                    operations::transact::transact_write_items(&state, &sqlite, &input, &ctx)
                })
                .await
            }

            // DynamoDB Streams (target prefix DynamoDBStreams_20120810)
            "DescribeStream" => operations::streams::describe_stream(&state, &input, ctx),
            "GetShardIterator" => operations::streams::get_shard_iterator(&state, &input, ctx),
            "GetRecords" => operations::streams::get_records(&state, &input, ctx),
            "ListStreams" => operations::streams::list_streams(&state, &input, ctx),

            // Limits
            "DescribeLimits" => operations::table::describe_limits(&state, &input, ctx),

            // Backup
            "CreateBackup" => operations::backup::create_backup(&state, &input, ctx),
            "DeleteBackup" => operations::backup::delete_backup(&state, &input, ctx),
            "DescribeBackup" => operations::backup::describe_backup(&state, &input, ctx),
            "ListBackups" => operations::backup::list_backups(&state, &input, ctx),
            "RestoreTableFromBackup" => {
                operations::backup::restore_table_from_backup(&state, &input, ctx)
            }
            "RestoreTableToPointInTime" => {
                operations::backup::restore_table_to_point_in_time(&state, &input, ctx)
            }

            // Global Tables
            "DescribeGlobalTable" => operations::table::describe_global_table(&state, &input, ctx),
            "ListGlobalTables" => operations::table::list_global_tables(&state, &input, ctx),

            // Exports
            "DescribeExport" => operations::table::describe_export(&state, &input, ctx),
            "ExportTableToPointInTime" => {
                operations::table::export_table_to_point_in_time(&state, &input, ctx)
            }
            "ListExports" => operations::table::list_exports(&state, &input, ctx),

            // Imports
            "DescribeImport" => operations::table::describe_import(&state, &input, ctx),
            "ImportTable" => operations::table::import_table(&state, &input, ctx),
            "ListImports" => operations::table::list_imports(&state, &input, ctx),

            // Contributor Insights
            "DescribeContributorInsights" => {
                operations::table::describe_contributor_insights(&state, &input, ctx)
            }
            "UpdateContributorInsights" => {
                operations::table::update_contributor_insights(&state, &input, ctx)
            }
            "ListContributorInsights" => {
                operations::table::list_contributor_insights(&state, &input, ctx)
            }

            // PartiQL — sqlite-backed.
            "ExecuteStatement" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || {
                    operations::partiql::execute_statement(&state, &sqlite, &input, &ctx)
                })
                .await
            }
            "BatchExecuteStatement" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || {
                    operations::partiql::batch_execute_statement(&state, &sqlite, &input, &ctx)
                })
                .await
            }
            "ExecuteTransaction" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || {
                    operations::partiql::execute_transaction(&state, &sqlite, &input, &ctx)
                })
                .await
            }

            // Kinesis Streaming Destination
            "EnableKinesisStreamingDestination" => {
                operations::kinesis_dest::enable_kinesis_streaming_destination(&state, &input, ctx)
            }
            "DisableKinesisStreamingDestination" => {
                operations::kinesis_dest::disable_kinesis_streaming_destination(&state, &input, ctx)
            }
            "DescribeKinesisStreamingDestination" => {
                operations::kinesis_dest::describe_kinesis_streaming_destination(
                    &state, &input, ctx,
                )
            }

            // Resource Policy
            "PutResourcePolicy" => {
                operations::resource_policy::put_resource_policy(&state, &input, ctx)
            }
            "GetResourcePolicy" => {
                operations::resource_policy::get_resource_policy(&state, &input, ctx)
            }
            "DeleteResourcePolicy" => {
                operations::resource_policy::delete_resource_policy(&state, &input, ctx)
            }

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn iam_action(&self, operation: &str) -> Option<String> {
        match operation {
            "CreateTable"
            | "DeleteTable"
            | "DescribeTable"
            | "ListTables"
            | "UpdateTable"
            | "DescribeEndpoints"
            | "DescribeTimeToLive"
            | "UpdateTimeToLive"
            | "DescribeContinuousBackups"
            | "UpdateContinuousBackups"
            | "TagResource"
            | "UntagResource"
            | "ListTagsOfResource"
            | "PutItem"
            | "GetItem"
            | "DeleteItem"
            | "UpdateItem"
            | "Query"
            | "Scan"
            | "BatchGetItem"
            | "BatchWriteItem"
            | "TransactGetItems"
            | "TransactWriteItems"
            | "DescribeStream"
            | "GetShardIterator"
            | "GetRecords"
            | "ListStreams"
            | "DescribeLimits"
            | "CreateBackup"
            | "DeleteBackup"
            | "DescribeBackup"
            | "ListBackups"
            | "RestoreTableFromBackup"
            | "RestoreTableToPointInTime"
            | "DescribeGlobalTable"
            | "ListGlobalTables"
            | "DescribeExport"
            | "ExportTableToPointInTime"
            | "ListExports"
            | "DescribeImport"
            | "ImportTable"
            | "ListImports"
            | "DescribeContributorInsights"
            | "UpdateContributorInsights"
            | "ListContributorInsights"
            | "ExecuteStatement"
            | "BatchExecuteStatement"
            | "ExecuteTransaction"
            | "EnableKinesisStreamingDestination"
            | "DisableKinesisStreamingDestination"
            | "DescribeKinesisStreamingDestination"
            | "PutResourcePolicy"
            | "GetResourcePolicy"
            | "DeleteResourcePolicy" => Some(format!("dynamodb:{operation}")),
            _ => None,
        }
    }

    fn iam_resource(&self, operation: &str, input: &Value, ctx: &RequestContext) -> Option<String> {
        let prefix = format!("arn:aws:dynamodb:{}:{}", ctx.region, ctx.account_id);
        match operation {
            "ListTables"
            | "DescribeEndpoints"
            | "DescribeLimits"
            | "ListGlobalTables"
            | "ListExports"
            | "ListImports"
            | "ListContributorInsights"
            | "ListBackups"
            | "ListStreams" => Some("*".to_string()),
            "DescribeStream" | "GetShardIterator" | "GetRecords" => {
                if let Some(stream_arn) = input.get("StreamArn").and_then(|v| v.as_str()) {
                    Some(stream_arn.to_string())
                } else {
                    let table = input.get("TableName").and_then(|v| v.as_str())?;
                    Some(format!("{prefix}:table/{table}/stream/*"))
                }
            }
            "DescribeBackup" | "DeleteBackup" | "RestoreTableFromBackup" => {
                if let Some(arn) = input.get("BackupArn").and_then(|v| v.as_str()) {
                    Some(arn.to_string())
                } else {
                    let table = input.get("TableName").and_then(|v| v.as_str())?;
                    Some(format!("{prefix}:table/{table}/backup/*"))
                }
            }
            "DescribeExport" => input
                .get("ExportArn")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            "DescribeImport" => input
                .get("ImportArn")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            "TagResource" | "UntagResource" | "ListTagsOfResource" => input
                .get("ResourceArn")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            _ => {
                let table = input.get("TableName").and_then(|v| v.as_str())?;
                Some(format!("{prefix}:table/{table}"))
            }
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        // Items live in SQLite. This snapshot only persists the schema
        // pieces the AccountRegionStore can't reconstruct itself; the
        // serialised `Table` no longer has an `items` field at all so
        // the JSON stays cheap regardless of row count.
        let tables: Vec<Table> = self
            .store
            .iter_all()
            .into_iter()
            .flat_map(|(_, state)| {
                state
                    .tables
                    .iter()
                    .map(|e| {
                        let t = e.value();
                        Table {
                            name: t.name.clone(),
                            arn: t.arn.clone(),
                            key_schema: t.key_schema.clone(),
                            attribute_definitions: t.attribute_definitions.clone(),
                            billing_mode: t.billing_mode.clone(),
                            status: t.status.clone(),
                            created_at: t.created_at,
                            gsi: t.gsi.clone(),
                            lsi: t.lsi.clone(),
                            stream_enabled: t.stream_enabled,
                            stream_arn: t.stream_arn.clone(),
                            stream_view_type: t.stream_view_type.clone(),
                            stream_records: t.stream_records.clone(),
                            stream_sequence: t.stream_sequence,
                            ttl: t.ttl.clone(),
                            tags: t.tags.clone(),
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        serde_json::to_vec(&DynamoStateSnapshot { tables }).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        // Deserialize through the legacy shape so older snapshots — which
        // still carry items inside Table — round-trip cleanly. The
        // `serde(default)` on `legacy_items` means current snapshots
        // (without an `items` field) parse just as well, with the field
        // defaulting to an empty BTreeMap.
        let snapshot: state::LegacyDynamoStateSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;

        for legacy in snapshot.tables {
            // DynamoDB ARN: arn:aws:dynamodb:{region}:{account}:table/{name}
            let (table, legacy_items) = legacy.into_parts();
            let parts: Vec<&str> = table.arn.splitn(6, ':').collect();
            let (account, region) = if parts.len() == 6 {
                (parts[4].to_string(), parts[3].to_string())
            } else {
                ("000000000000".to_string(), "us-east-1".to_string())
            };

            for (_composite, item) in legacy_items {
                if let Some(keys) = keys::extract_item_keys(&table, &item) {
                    let attrs = keys::item_to_storage_value(&item);
                    self.sqlite
                        .put_item(
                            &account,
                            &region,
                            &table.name,
                            &keys.pk,
                            &keys.sk,
                            &attrs,
                            &keys.gsi,
                        )
                        .map_err(|e| format!("DynamoDB legacy item migrate failed: {e}"))?;
                }
            }

            // Mirror the schema row so a fresh process without a snapshot
            // can still bootstrap from SQLite alone.
            if let Ok(schema_value) = serde_json::to_value(&table) {
                let _ = self
                    .sqlite
                    .put_table_schema(&account, &region, &table.name, &schema_value);
            }

            let state = self.store.get(&account, &region);
            state.tables.insert(table.name.clone(), table);
        }

        Ok(())
    }
}
