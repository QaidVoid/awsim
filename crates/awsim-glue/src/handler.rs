use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{crawlers, databases, jobs, tables};
use crate::state::GlueState;

/// The Glue service handler.
pub struct GlueService {
    store: AccountRegionStore<GlueState>,
}

impl GlueService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for GlueService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ServiceHandler for GlueService {
    fn service_name(&self) -> &str {
        "glue"
    }

    fn signing_name(&self) -> &str {
        "glue"
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
        debug!(operation = %operation, "Glue operation");

        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            // Databases
            "CreateDatabase" => databases::create_database(&state, &input, ctx),
            "GetDatabase" => databases::get_database(&state, &input, ctx),
            "GetDatabases" => databases::get_databases(&state, &input, ctx),
            "DeleteDatabase" => databases::delete_database(&state, &input, ctx),
            "UpdateDatabase" => databases::update_database(&state, &input, ctx),

            // Tables
            "CreateTable" => tables::create_table(&state, &input, ctx),
            "GetTable" => tables::get_table(&state, &input, ctx),
            "GetTables" => tables::get_tables(&state, &input, ctx),
            "DeleteTable" => tables::delete_table(&state, &input, ctx),
            "UpdateTable" => tables::update_table(&state, &input, ctx),

            // Crawlers
            "CreateCrawler" => crawlers::create_crawler(&state, &input, ctx),
            "GetCrawler" => crawlers::get_crawler(&state, &input, ctx),
            "GetCrawlers" => crawlers::get_crawlers(&state, &input, ctx),
            "DeleteCrawler" => crawlers::delete_crawler(&state, &input, ctx),
            "StartCrawler" => crawlers::start_crawler(&state, &input, ctx),
            "StopCrawler" => crawlers::stop_crawler(&state, &input, ctx),

            // Jobs
            "CreateJob" => jobs::create_job(&state, &input, ctx),
            "GetJob" => jobs::get_job(&state, &input, ctx),
            "GetJobs" => jobs::get_jobs(&state, &input, ctx),
            "DeleteJob" => jobs::delete_job(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
