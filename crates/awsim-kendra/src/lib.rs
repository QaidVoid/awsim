//! AWS Kendra intelligent search emulator for AWSim.
//!
//! Provides index management, document indexing, full-text query,
//! and passage retrieval with simple substring-based matching.

mod operations;
pub mod state;
mod util;

use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use state::KendraState;

/// Amazon Kendra service emulator.
pub struct KendraService {
    state: AccountRegionStore<KendraState>,
}

impl KendraService {
    pub fn new() -> Self {
        Self {
            state: AccountRegionStore::new(),
        }
    }
}

impl Default for KendraService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ServiceHandler for KendraService {
    fn service_name(&self) -> &str {
        "kendra"
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
        let state = self.state.get(&ctx.account_id, &ctx.region);

        match operation {
            // Index management
            "CreateIndex" => operations::indexes::create_index(&state, &input, ctx),
            "DescribeIndex" => operations::indexes::describe_index(&state, &input),
            "ListIndices" => operations::indexes::list_indices(&state),
            "DeleteIndex" => operations::indexes::delete_index(&state, &input),
            "UpdateIndex" => operations::indexes::update_index(&state, &input),

            // Data sources
            "CreateDataSource" => operations::indexes::create_data_source(&state, &input, ctx),
            "ListDataSources" => operations::indexes::list_data_sources(&state, &input),
            "DeleteDataSource" => operations::indexes::delete_data_source(&state, &input),

            // Documents
            "BatchPutDocument" => operations::documents::batch_put_document(&state, &input),
            "BatchDeleteDocument" => operations::documents::batch_delete_document(&state, &input),

            // Search
            "Query" => operations::query::query(&state, &input),
            "Retrieve" => operations::query::retrieve(&state, &input),
            "SubmitFeedback" => operations::query::submit_feedback(&state, &input),

            _ => Err(AwsError::not_implemented(operation)),
        }
    }
}
