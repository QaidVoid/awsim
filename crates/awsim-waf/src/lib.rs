mod operations;
mod state;

pub use state::WafState;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::WafStateSnapshot;

/// The WAF v2 service handler.
pub struct WafService {
    store: AccountRegionStore<WafState>,
}

impl WafService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<WafState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for WafService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for WafService {
    fn service_name(&self) -> &str {
        "wafv2"
    }

    fn signing_name(&self) -> &str {
        "wafv2"
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
        debug!(operation, "WAF v2 request");
        let state = self.get_state(ctx);

        match operation {
            // WebACL operations
            "CreateWebACL" => operations::web_acls::create_web_acl(&state, &input, ctx),
            "GetWebACL" => operations::web_acls::get_web_acl(&state, &input, ctx),
            "ListWebACLs" => operations::web_acls::list_web_acls(&state, &input, ctx),
            "DeleteWebACL" => operations::web_acls::delete_web_acl(&state, &input, ctx),
            "UpdateWebACL" => operations::web_acls::update_web_acl(&state, &input, ctx),

            // IP Set operations
            "CreateIPSet" => operations::ip_sets::create_ip_set(&state, &input, ctx),
            "GetIPSet" => operations::ip_sets::get_ip_set(&state, &input, ctx),
            "ListIPSets" => operations::ip_sets::list_ip_sets(&state, &input, ctx),
            "DeleteIPSet" => operations::ip_sets::delete_ip_set(&state, &input, ctx),
            "UpdateIPSet" => operations::ip_sets::update_ip_set(&state, &input, ctx),

            // Rule Group operations
            "CreateRuleGroup" => operations::rule_groups::create_rule_group(&state, &input, ctx),
            "GetRuleGroup" => operations::rule_groups::get_rule_group(&state, &input, ctx),
            "ListRuleGroups" => operations::rule_groups::list_rule_groups(&state, &input, ctx),
            "DeleteRuleGroup" => operations::rule_groups::delete_rule_group(&state, &input, ctx),
            "UpdateRuleGroup" => operations::rule_groups::update_rule_group(&state, &input, ctx),
            "CheckCapacity" => operations::rule_groups::check_capacity(&state, &input, ctx),
            "ListAvailableManagedRuleGroups" => {
                operations::rule_groups::list_available_managed_rule_groups(&state, &input, ctx)
            }

            // Logging Configuration operations
            "PutLoggingConfiguration" => {
                operations::logging::put_logging_configuration(&state, &input, ctx)
            }
            "GetLoggingConfiguration" => {
                operations::logging::get_logging_configuration(&state, &input, ctx)
            }
            "DeleteLoggingConfiguration" => {
                operations::logging::delete_logging_configuration(&state, &input, ctx)
            }
            "ListLoggingConfigurations" => {
                operations::logging::list_logging_configurations(&state, &input, ctx)
            }

            // WebACL Association operations
            "AssociateWebACL" => operations::associations::associate_web_acl(&state, &input, ctx),
            "DisassociateWebACL" => {
                operations::associations::disassociate_web_acl(&state, &input, ctx)
            }
            "GetWebACLForResource" => {
                operations::associations::get_web_acl_for_resource(&state, &input, ctx)
            }
            "ListResourcesForWebACL" => {
                operations::associations::list_resources_for_web_acl(&state, &input, ctx)
            }

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut snap = WafStateSnapshot {
            web_acls: vec![],
            ip_sets: vec![],
            rule_groups: vec![],
            logging_configs: vec![],
            web_acl_associations: vec![],
        };

        for (_, state) in self.store.iter_all() {
            let s = state.to_snapshot();
            snap.web_acls.extend(s.web_acls);
            snap.ip_sets.extend(s.ip_sets);
            snap.rule_groups.extend(s.rule_groups);
            snap.logging_configs.extend(s.logging_configs);
            snap.web_acl_associations.extend(s.web_acl_associations);
        }

        serde_json::to_vec(&snap).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snapshot: WafStateSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;

        // WAF ARNs contain region+account — use defaults for simplicity
        let state = self.store.get("000000000000", "us-east-1");
        state.restore_from_snapshot(snapshot);

        Ok(())
    }
}
