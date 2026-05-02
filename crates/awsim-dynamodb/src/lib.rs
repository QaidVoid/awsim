#![deny(warnings)]

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
    /// Holds the per-process `TempDir` for the no-data-dir case so
    /// `dynamodb.db` + `.db-wal` + `.db-shm` are deleted when the
    /// service drops. `None` when persistent storage is in use — the
    /// user owns that directory.
    _tempdir: Option<tempfile::TempDir>,
}

impl DynamoDbService {
    /// Ephemeral in-process store. Useful for tests and `awsim` runs
    /// that don't pass `--data-dir` — files live in a `TempDir` that
    /// the OS cleans up on graceful shutdown via the Drop impl.
    pub fn new() -> Self {
        // Best-effort cleanup of leaked legacy temp files from prior
        // crashes / SIGKILLs / pre-tempdir versions of awsim. The old
        // layout was a single `awsim-ddb-{uuid}.db` (+ .db-wal / .db-shm)
        // directly in $TMPDIR; the new layout is a self-cleaning
        // tempdir, so anything matching the old pattern is safe to
        // remove here.
        sweep_legacy_temp_files();

        let dir = tempfile::Builder::new()
            .prefix("awsim-ddb-")
            .tempdir()
            .expect("creating ephemeral DynamoDB tempdir should not fail");
        let path = dir.path().join("dynamodb.db");
        let sqlite = SqliteStore::open(&path)
            .expect("opening ephemeral DynamoDB sqlite store should not fail");
        Self {
            store: AccountRegionStore::new(),
            sqlite: Arc::new(sqlite),
            _tempdir: Some(dir),
        }
    }

    /// Persistent store rooted at `{dir}/dynamodb.db`. Mirrors the
    /// `with_data_dir` convention used by the other AWSim services so
    /// the `awsim` binary can wire it the same way.
    ///
    /// SQLite needs the parent directory to exist before `open()`, so we
    /// create it here. Other services that lazily write files (S3 body
    /// store, lambda code) get away without this because their first
    /// write does the create — sqlite can't.
    pub fn with_data_dir(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir)
            .unwrap_or_else(|e| panic!("creating DynamoDB data dir {} failed: {e}", dir.display()));
        let path = dir.join("dynamodb.db");
        let sqlite = SqliteStore::open(&path).unwrap_or_else(|e| {
            panic!(
                "opening persistent DynamoDB sqlite store at {} failed: {e}",
                path.display()
            )
        });
        Self {
            store: AccountRegionStore::new(),
            sqlite: Arc::new(sqlite),
            _tempdir: None,
        }
    }

    /// Reclaim disk space after heavy DELETE / UPDATE churn — exposed
    /// so the awsim binary can wire it to a CLI / admin endpoint.
    pub fn vacuum(&self) -> Result<(), AwsError> {
        self.sqlite.vacuum()
    }

    /// Spawn a background tokio task that periodically scans every
    /// (account, region, table) with TTL enabled and deletes items
    /// whose TTL attribute has passed. Mirrors the AWS contract that
    /// expired items are eventually removed (within ~48 hours on
    /// AWS); we run every `interval_secs` (default 60) which is far
    /// more aggressive but cheap on a local emulator.
    ///
    /// Returns immediately. The task lives until the process exits.
    pub fn spawn_ttl_sweeper(&self, interval_secs: u64) {
        let store = self.store.clone();
        let sqlite = Arc::clone(&self.sqlite);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            // Skip the immediate first tick so we don't sweep before
            // the user has had a chance to insert anything.
            tick.tick().await;
            loop {
                tick.tick().await;
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                // Snapshot the (account, region, state) set so we can
                // iterate without holding any DashMap locks across the
                // sqlite calls.
                let regions = store.iter_all();
                for ((account, region), state) in regions {
                    // Collect ttl-enabled tables to avoid holding the
                    // tables DashMap iterator across awaits / sqlite
                    // calls.
                    let targets: Vec<(String, String)> = state
                        .tables
                        .iter()
                        .filter_map(|e| {
                            let t = e.value();
                            if t.ttl.enabled && !t.ttl.attribute_name.is_empty() {
                                Some((t.name.clone(), t.ttl.attribute_name.clone()))
                            } else {
                                None
                            }
                        })
                        .collect();

                    for (table_name, attr) in targets {
                        let sqlite = Arc::clone(&sqlite);
                        let account = account.clone();
                        let region = region.clone();
                        // sqlite calls block — run them on the
                        // blocking pool so we don't stall the runtime.
                        let res = tokio::task::spawn_blocking(move || {
                            sqlite.delete_expired_items(
                                &account,
                                &region,
                                &table_name,
                                &attr,
                                now_secs,
                            )
                        })
                        .await;
                        match res {
                            Ok(Ok(removed)) if removed > 0 => {
                                tracing::info!(removed, "DynamoDB TTL sweep removed expired items");
                            }
                            Ok(Ok(_)) => {}
                            Ok(Err(e)) => {
                                tracing::warn!(error = %e.message, "DynamoDB TTL sweep failed");
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "DynamoDB TTL sweep join error");
                            }
                        }
                    }
                }
            }
        });
    }

    /// If this instance owns an ephemeral tempdir (no `--data-dir`
    /// case), return its path so the shutdown handler can remove it
    /// before calling `process::exit`. Drop wouldn't run otherwise.
    pub fn tempdir_path(&self) -> Option<&Path> {
        self._tempdir.as_ref().map(|d| d.path())
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<DynamoState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }

    /// Bulk-seed `tables` tables, each with `items_per_table` items,
    /// directly into state + SQLite — bypasses the SigV4 / gateway path
    /// so a 1k-table × 100-item seed completes in well under a second.
    /// Each table gets a single `id` (String) hash key. The `id_prefix`
    /// is used as the basename for the generated table names so seed
    /// data is easy to spot / clean up later.
    pub fn seed(&self, input: SeedDatasetInput) -> SeedDatasetOutput {
        use serde_json::json;
        use uuid::Uuid;
        let state = self.store.get(&input.account, &input.region);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        let mut tables_created = 0u64;
        let mut items_created = 0u64;
        let mut errors: Vec<String> = Vec::new();
        let empty_gsi: [(Option<String>, Option<String>); MAX_GSI_SLOTS] = Default::default();

        for t in 0..input.tables {
            let table_name = format!("{}-{}-{}", input.id_prefix, t, Uuid::new_v4().simple());
            let table = state::Table {
                name: table_name.clone(),
                arn: format!(
                    "arn:aws:dynamodb:{}:{}:table/{table_name}",
                    input.region, input.account
                ),
                key_schema: vec![state::KeySchemaElement {
                    attribute_name: "id".to_string(),
                    key_type: "HASH".to_string(),
                }],
                attribute_definitions: vec![state::AttributeDefinition {
                    attribute_name: "id".to_string(),
                    attribute_type: "S".to_string(),
                }],
                billing_mode: "PAY_PER_REQUEST".to_string(),
                status: "ACTIVE".to_string(),
                created_at: now,
                gsi: Vec::new(),
                lsi: Vec::new(),
                stream_enabled: false,
                stream_arn: None,
                stream_view_type: None,
                stream_records: Vec::new(),
                stream_sequence: 0,
                ttl: state::TtlSpecification::default(),
                tags: std::collections::HashMap::new(),
                deletion_protection_enabled: false,
                sse: state::SseSpecification::default(),
                read_capacity_units: 0,
                write_capacity_units: 0,
            };
            state.tables.insert(table_name.clone(), table);
            tables_created += 1;

            for i in 0..input.items_per_table {
                let pk = Uuid::new_v4().to_string();
                let attrs = json!({
                    "id":     { "S": pk },
                    "name":   { "S": format!("seed-name-{i}") },
                    "email":  { "S": format!("seed-{i}@example.test") },
                    "score":  { "N": (i % 1000).to_string() },
                    "active": { "BOOL": (i % 2 == 0) }
                });
                if let Err(e) = self.sqlite.put_item(
                    &input.account,
                    &input.region,
                    &table_name,
                    &pk,
                    "",
                    &attrs,
                    &empty_gsi,
                ) {
                    errors.push(format!("{table_name}/{pk}: {}", e.message));
                    if errors.len() > 10 {
                        break;
                    }
                    continue;
                }
                items_created += 1;
            }
        }

        SeedDatasetOutput {
            tables_created,
            items_created,
            errors,
        }
    }
}

/// Input shape for `DynamoDbService::seed`.
pub struct SeedDatasetInput {
    pub account: String,
    pub region: String,
    pub tables: u64,
    pub items_per_table: u64,
    pub id_prefix: String,
}

/// Result shape returned by `DynamoDbService::seed`.
pub struct SeedDatasetOutput {
    pub tables_created: u64,
    pub items_created: u64,
    pub errors: Vec<String>,
}

impl Default for DynamoDbService {
    fn default() -> Self {
        Self::new()
    }
}

/// Remove any leftover `awsim-ddb-{uuid}.db[-wal|-shm]?` files in
/// the system temp directory. These came from older awsim builds
/// (pre-tempdir) that didn't clean up on shutdown — once the
/// process owning them is gone, the files are pure garbage.
///
/// Best-effort: failure to read $TMPDIR or unlink any individual
/// file is logged at debug level but never blocks startup.
fn sweep_legacy_temp_files() {
    let tmp = std::env::temp_dir();
    let entries = match std::fs::read_dir(&tmp) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut removed = 0u64;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        // Old pattern: `awsim-ddb-{uuid}.db` plus optional `-wal` / `-shm`.
        // The new tempdir-based pattern is `awsim-ddb-{random}/...` —
        // a directory, not a regular file — so this filter doesn't
        // accidentally delete a live tempdir.
        if !name_str.starts_with("awsim-ddb-") {
            continue;
        }
        if !(name_str.ends_with(".db")
            || name_str.ends_with(".db-wal")
            || name_str.ends_with(".db-shm"))
        {
            continue;
        }
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_file() {
            continue;
        }
        if std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        debug!(removed, "cleaned up legacy DynamoDB tmp files");
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
                run_blocking(move || operations::table::create_table(&state, &sqlite, &input, &ctx))
                    .await
            }
            "DeleteTable" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || operations::table::delete_table(&state, &sqlite, &input, &ctx))
                    .await
            }
            // awsim-only — no AWS equivalent. Clears items, keeps schema.
            "TruncateTable" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || {
                    operations::table::truncate_table(&state, &sqlite, &input, &ctx)
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
                run_blocking(move || operations::table::update_table(&state, &sqlite, &input, &ctx))
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
                run_blocking(move || operations::item::delete_item(&state, &sqlite, &input, &ctx))
                    .await
            }
            "UpdateItem" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || operations::item::update_item(&state, &sqlite, &input, &ctx))
                    .await
            }

            // Query & Scan — read items from SQLite, evaluate filters in Rust.
            "Query" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || operations::query::query(&state, &sqlite, &input, &ctx)).await
            }
            "Scan" => {
                let state = state.clone();
                let sqlite = self.sqlite.clone();
                let input = input.clone();
                let ctx = ctx.clone();
                run_blocking(move || operations::query::scan(&state, &sqlite, &input, &ctx)).await
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
            "CreateBackup" => operations::backup::create_backup(&state, &self.sqlite, &input, ctx),
            "DeleteBackup" => operations::backup::delete_backup(&state, &input, ctx),
            "DescribeBackup" => operations::backup::describe_backup(&state, &input, ctx),
            "ListBackups" => operations::backup::list_backups(&state, &input, ctx),
            "RestoreTableFromBackup" => {
                operations::backup::restore_table_from_backup(&state, &self.sqlite, &input, ctx)
            }
            "RestoreTableToPointInTime" => {
                operations::backup::restore_table_to_point_in_time(&state, &input, ctx)
            }

            // Global Tables
            "CreateGlobalTable" => operations::table::create_global_table(&state, &input, ctx),
            "UpdateGlobalTable" => operations::table::update_global_table(&state, &input, ctx),
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
            | "TruncateTable"
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
            | "CreateGlobalTable"
            | "UpdateGlobalTable"
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
                            deletion_protection_enabled: t.deletion_protection_enabled,
                            sse: t.sse.clone(),
                            read_capacity_units: t.read_capacity_units,
                            write_capacity_units: t.write_capacity_units,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        serde_json::to_vec(&DynamoStateSnapshot { tables }).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snapshot: DynamoStateSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;

        for table in snapshot.tables {
            // DynamoDB ARN: arn:aws:dynamodb:{region}:{account}:table/{name}
            let parts: Vec<&str> = table.arn.splitn(6, ':').collect();
            let (account, region) = if parts.len() == 6 {
                (parts[4].to_string(), parts[3].to_string())
            } else {
                ("000000000000".to_string(), "us-east-1".to_string())
            };

            // Mirror the schema row so a fresh process can still bootstrap
            // from SQLite alone if the snapshot file goes missing.
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
