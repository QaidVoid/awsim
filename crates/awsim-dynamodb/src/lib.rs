mod expressions;
mod operations;
mod state;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::{DynamoState, DynamoStateSnapshot, Table};

/// The AWSim DynamoDB service handler.
pub struct DynamoDbService {
    store: AccountRegionStore<DynamoState>,
}

impl DynamoDbService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
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
            "CreateTable" => operations::table::create_table(&state, &input, ctx),
            "DeleteTable" => operations::table::delete_table(&state, &input, ctx),
            "DescribeTable" => operations::table::describe_table(&state, &input, ctx),
            "ListTables" => operations::table::list_tables(&state, &input, ctx),
            "UpdateTable" => operations::table::update_table(&state, &input, ctx),

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

            // Item operations
            "PutItem" => operations::item::put_item(&state, &input, ctx),
            "GetItem" => operations::item::get_item(&state, &input, ctx),
            "DeleteItem" => operations::item::delete_item(&state, &input, ctx),
            "UpdateItem" => operations::item::update_item(&state, &input, ctx),

            // Query & Scan
            "Query" => operations::query::query(&state, &input, ctx),
            "Scan" => operations::query::scan(&state, &input, ctx),

            // Batch operations
            "BatchGetItem" => operations::batch::batch_get_item(&state, &input, ctx),
            "BatchWriteItem" => operations::batch::batch_write_item(&state, &input, ctx),

            // Transactions
            "TransactGetItems" => operations::transact::transact_get_items(&state, &input, ctx),
            "TransactWriteItems" => operations::transact::transact_write_items(&state, &input, ctx),

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

            // PartiQL
            "ExecuteStatement" => operations::partiql::execute_statement(&state, &input, ctx),
            "BatchExecuteStatement" => {
                operations::partiql::batch_execute_statement(&state, &input, ctx)
            }
            "ExecuteTransaction" => operations::partiql::execute_transaction(&state, &input, ctx),

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
                            created_at: t.created_at.clone(),
                            gsi: t.gsi.clone(),
                            lsi: t.lsi.clone(),
                            items: t.items.clone(),
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
        let snapshot: DynamoStateSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;

        for table in snapshot.tables {
            // Derive account+region from table ARN.
            // DynamoDB ARN: arn:aws:dynamodb:{region}:{account}:table/{name}
            let parts: Vec<&str> = table.arn.splitn(6, ':').collect();
            let (account, region) = if parts.len() == 6 {
                (parts[4].to_string(), parts[3].to_string())
            } else {
                ("000000000000".to_string(), "us-east-1".to_string())
            };

            let state = self.store.get(&account, &region);
            state.tables.insert(table.name.clone(), table);
        }

        Ok(())
    }
}
