mod expressions;
mod operations;
mod state;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::DynamoState;

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

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
