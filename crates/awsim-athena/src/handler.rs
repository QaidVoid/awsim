use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{
    data_catalogs, databases, extras, named_queries, prepared_statements, query_executions,
    table_metadata, workgroups,
};
use crate::state::AthenaState;

/// The Athena service handler.
pub struct AthenaService {
    store: AccountRegionStore<AthenaState>,
}

impl AthenaService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for AthenaService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ServiceHandler for AthenaService {
    fn service_name(&self) -> &str {
        "athena"
    }

    fn signing_name(&self) -> &str {
        "athena"
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
        debug!(operation = %operation, "Athena operation");

        let state = self.store.get(&ctx.account_id, &ctx.region);

        // Ensure built-in `primary` workgroup exists on first access.
        {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string();
            state.ensure_primary_workgroup(&now);
        }

        match operation {
            // Workgroups
            "CreateWorkGroup" => workgroups::create_workgroup(&state, &input, ctx),
            "DeleteWorkGroup" => workgroups::delete_workgroup(&state, &input, ctx),
            "GetWorkGroup" => workgroups::get_workgroup(&state, &input, ctx),
            "ListWorkGroups" => workgroups::list_workgroups(&state, &input, ctx),
            "UpdateWorkGroup" => workgroups::update_workgroup(&state, &input, ctx),

            // Query executions
            "StartQueryExecution" => query_executions::start_query_execution(&state, &input, ctx),
            "GetQueryExecution" => query_executions::get_query_execution(&state, &input, ctx),
            "GetQueryResults" => query_executions::get_query_results(&state, &input, ctx),
            "ListQueryExecutions" => query_executions::list_query_executions(&state, &input, ctx),
            "StopQueryExecution" => query_executions::stop_query_execution(&state, &input, ctx),
            "BatchGetQueryExecution" => {
                query_executions::batch_get_query_execution(&state, &input, ctx)
            }

            // Named queries
            "CreateNamedQuery" => named_queries::create_named_query(&state, &input, ctx),
            "GetNamedQuery" => named_queries::get_named_query(&state, &input, ctx),
            "ListNamedQueries" => named_queries::list_named_queries(&state, &input, ctx),
            "DeleteNamedQuery" => named_queries::delete_named_query(&state, &input, ctx),
            "BatchGetNamedQuery" => named_queries::batch_get_named_query(&state, &input, ctx),

            // Databases (stub)
            "ListDatabases" => databases::list_databases(&state, &input, ctx),
            "GetDatabase" => databases::get_database(&state, &input, ctx),

            // Data Catalogs
            "ListDataCatalogs" => data_catalogs::list_data_catalogs(&state, &input, ctx),
            "GetDataCatalog" => data_catalogs::get_data_catalog(&state, &input, ctx),
            "CreateDataCatalog" => data_catalogs::create_data_catalog(&state, &input, ctx),
            "DeleteDataCatalog" => data_catalogs::delete_data_catalog(&state, &input, ctx),

            // Prepared Statements
            "CreatePreparedStatement" => {
                prepared_statements::create_prepared_statement(&state, &input, ctx)
            }
            "GetPreparedStatement" => {
                prepared_statements::get_prepared_statement(&state, &input, ctx)
            }
            "ListPreparedStatements" => {
                prepared_statements::list_prepared_statements(&state, &input, ctx)
            }
            "DeletePreparedStatement" => {
                prepared_statements::delete_prepared_statement(&state, &input, ctx)
            }

            // Table Metadata
            "GetTableMetadata" => table_metadata::get_table_metadata(&state, &input, ctx),
            "ListTableMetadata" => table_metadata::list_table_metadata(&state, &input, ctx),

            // Tags + Misc
            "TagResource" => extras::tag_resource(&state, &input, ctx),
            "UntagResource" => extras::untag_resource(&state, &input, ctx),
            "ListTagsForResource" => extras::list_tags_for_resource(&state, &input, ctx),
            "ListEngineVersions" => extras::list_engine_versions(&state, &input, ctx),
            "ListApplicationDPUSizes" => extras::list_application_dpu_sizes(&state, &input, ctx),
            "GetQueryRuntimeStatistics" => {
                extras::get_query_runtime_statistics(&state, &input, ctx)
            }
            "UpdateDataCatalog" => extras::update_data_catalog(&state, &input, ctx),
            "BatchGetPreparedStatement" => {
                extras::batch_get_prepared_statement(&state, &input, ctx)
            }

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
