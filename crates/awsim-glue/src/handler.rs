use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{connections, crawlers, databases, extras, jobs, tables, tags};
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
            "SearchTables" => tables::search_tables(&state, &input, ctx),

            // Partitions
            "GetPartitions" => tables::get_partitions(&state, &input, ctx),
            "CreatePartition" => tables::create_partition(&state, &input, ctx),
            "DeletePartition" => tables::delete_partition(&state, &input, ctx),
            "BatchCreatePartition" => tables::batch_create_partition(&state, &input, ctx),
            "BatchDeletePartition" => tables::batch_delete_partition(&state, &input, ctx),

            // Crawlers
            "CreateCrawler" => crawlers::create_crawler(&state, &input, ctx),
            "GetCrawler" => crawlers::get_crawler(&state, &input, ctx),
            "GetCrawlers" => crawlers::get_crawlers(&state, &input, ctx),
            "DeleteCrawler" => crawlers::delete_crawler(&state, &input, ctx),
            "StartCrawler" => crawlers::start_crawler(&state, &input, ctx),
            "StopCrawler" => crawlers::stop_crawler(&state, &input, ctx),
            "UpdateCrawler" => crawlers::update_crawler(&state, &input, ctx),
            "GetCrawlerMetrics" => crawlers::get_crawler_metrics(&state, &input, ctx),
            "GetClassifier" => crawlers::get_classifier(&state, &input, ctx),
            "GetClassifiers" => crawlers::get_classifiers(&state, &input, ctx),

            // Jobs
            "CreateJob" => jobs::create_job(&state, &input, ctx),
            "GetJob" => jobs::get_job(&state, &input, ctx),
            "GetJobs" => jobs::get_jobs(&state, &input, ctx),
            "DeleteJob" => jobs::delete_job(&state, &input, ctx),
            "BatchGetJobs" => jobs::batch_get_jobs(&state, &input, ctx),

            // Job Runs
            "StartJobRun" => jobs::start_job_run(&state, &input, ctx),
            "GetJobRun" => jobs::get_job_run(&state, &input, ctx),
            "GetJobRuns" => jobs::get_job_runs(&state, &input, ctx),
            "BatchStopJobRun" => jobs::batch_stop_job_run(&state, &input, ctx),

            // Connections
            "CreateConnection" => connections::create_connection(&state, &input, ctx),
            "GetConnections" => connections::get_connections(&state, &input, ctx),
            "DeleteConnection" => connections::delete_connection(&state, &input, ctx),

            // Tags
            "GetTags" => tags::get_tags(&state, &input, ctx),
            "TagResource" => tags::tag_resource(&state, &input, ctx),
            "UntagResource" => tags::untag_resource(&state, &input, ctx),

            // Extras: Tables/Partitions/Jobs/Connections additions
            "BatchDeleteTable" => extras::batch_delete_table(&state, &input, ctx),
            "BatchGetTables" => extras::batch_get_tables(&state, &input, ctx),
            "GetPartition" => extras::get_partition(&state, &input, ctx),
            "BatchGetPartition" => extras::batch_get_partition(&state, &input, ctx),
            "UpdateJob" => extras::update_job(&state, &input, ctx),
            "GetConnection" => extras::get_connection(&state, &input, ctx),
            "UpdateConnection" => extras::update_connection(&state, &input, ctx),

            // Triggers
            "CreateTrigger" => extras::create_trigger(&state, &input, ctx),
            "GetTrigger" => extras::get_trigger(&state, &input, ctx),
            "GetTriggers" => extras::get_triggers(&state, &input, ctx),
            "DeleteTrigger" => extras::delete_trigger(&state, &input, ctx),

            // Workflows
            "CreateWorkflow" => extras::create_workflow(&state, &input, ctx),
            "GetWorkflow" => extras::get_workflow(&state, &input, ctx),
            "ListWorkflows" => extras::list_workflows(&state, &input, ctx),
            "DeleteWorkflow" => extras::delete_workflow(&state, &input, ctx),

            // Table Versions
            "GetTableVersion" => extras::get_table_version(&state, &input, ctx),
            "GetTableVersions" => extras::get_table_versions(&state, &input, ctx),
            "DeleteTableVersion" => extras::delete_table_version(&state, &input, ctx),
            "BatchDeleteTableVersion" => extras::batch_delete_table_version(&state, &input, ctx),

            // Scripts & Catalog
            "CreateScript" => extras::create_script(&state, &input, ctx),
            "GetCatalogImportStatus" => extras::get_catalog_import_status(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
