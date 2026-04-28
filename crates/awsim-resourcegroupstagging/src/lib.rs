mod operations;
mod state;

pub use state::{TaggingState, TaggingStateSnapshot};

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::{Value, json};
use tracing::debug;

/// AWS Resource Groups Tagging API emulator.
///
/// Maintains a per-(account, region) ARN → tag map populated through
/// `TagResources` / `UntagResources`. Cross-service tag propagation is not
/// modelled — callers explicitly tag resources here for them to show up in
/// `GetResources` / `GetTagKeys` / `GetTagValues`.
pub struct ResourceGroupsTaggingService {
    store: AccountRegionStore<TaggingState>,
}

impl ResourceGroupsTaggingService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<TaggingState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for ResourceGroupsTaggingService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for ResourceGroupsTaggingService {
    fn service_name(&self) -> &str {
        "tagging"
    }

    fn signing_name(&self) -> &str {
        "tagging"
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
        debug!(operation, "ResourceGroupsTagging request");
        let state = self.get_state(ctx);

        match operation {
            "GetResources" => operations::resources::get_resources(&state, &input, ctx),
            "GetTagKeys" => operations::tag_keys::get_tag_keys(&state, &input, ctx),
            "GetTagValues" => operations::tag_values::get_tag_values(&state, &input, ctx),
            "TagResources" => operations::tagging::tag_resources(&state, &input, ctx),
            "UntagResources" => operations::tagging::untag_resources(&state, &input, ctx),

            // Compliance / report ops — not modelled. Return empty shapes that
            // satisfy the SDK without claiming the work was done.
            "DescribeReportCreation" => Ok(json!({
                "Status": "NONE",
                "S3Location": "",
                "ErrorMessage": "",
            })),
            "StartReportCreation" => Ok(json!({})),
            "GetComplianceSummary" => Ok(json!({
                "PaginationToken": "",
                "SummaryList": [],
            })),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let entries: Vec<(String, String, TaggingStateSnapshot)> = self
            .store
            .iter_all()
            .into_iter()
            .map(|((account, region), state)| (account, region, state.snapshot()))
            .collect();
        serde_json::to_vec(&entries).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let entries: Vec<(String, String, TaggingStateSnapshot)> =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;
        for (account, region, snap) in entries {
            self.store.get(&account, &region).restore(snap);
        }
        Ok(())
    }
}
