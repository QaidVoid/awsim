use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{databases, named_queries, query_executions, workgroups};
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

            // Query executions
            "StartQueryExecution" => query_executions::start_query_execution(&state, &input, ctx),
            "GetQueryExecution" => query_executions::get_query_execution(&state, &input, ctx),
            "GetQueryResults" => query_executions::get_query_results(&state, &input, ctx),
            "ListQueryExecutions" => query_executions::list_query_executions(&state, &input, ctx),
            "StopQueryExecution" => query_executions::stop_query_execution(&state, &input, ctx),

            // Named queries
            "CreateNamedQuery" => named_queries::create_named_query(&state, &input, ctx),
            "GetNamedQuery" => named_queries::get_named_query(&state, &input, ctx),
            "ListNamedQueries" => named_queries::list_named_queries(&state, &input, ctx),
            "DeleteNamedQuery" => named_queries::delete_named_query(&state, &input, ctx),

            // Databases (stub)
            "ListDatabases" => databases::list_databases(&state, &input, ctx),
            "GetDatabase" => databases::get_database(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
