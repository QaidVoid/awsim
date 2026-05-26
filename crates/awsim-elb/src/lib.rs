mod error;
mod ids;
mod operations;
mod state;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::ElbState;

/// The AWSim ELB v2 service handler (ALB/NLB).
pub struct ElbService {
    store: AccountRegionStore<ElbState>,
}

impl ElbService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<ElbState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for ElbService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for ElbService {
    fn service_name(&self) -> &str {
        "elasticloadbalancing"
    }

    fn signing_name(&self) -> &str {
        "elasticloadbalancing"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsQuery
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "ELB request");
        let state = self.get_state(ctx);

        match operation {
            // Load Balancers
            "CreateLoadBalancer" => {
                operations::load_balancers::create_load_balancer(&state, &input, ctx)
            }
            "DeleteLoadBalancer" => {
                operations::load_balancers::delete_load_balancer(&state, &input)
            }
            "DescribeLoadBalancers" => {
                operations::load_balancers::describe_load_balancers(&state, &input)
            }
            "DescribeLoadBalancerAttributes" => {
                operations::load_balancers::describe_load_balancer_attributes(&state, &input)
            }
            "ModifyLoadBalancerAttributes" => {
                operations::load_balancers::modify_load_balancer_attributes(&state, &input)
            }
            "SetSecurityGroups" => operations::load_balancers::set_security_groups(&state, &input),
            "SetSubnets" => operations::load_balancers::set_subnets(&state, &input),

            // Target Groups
            "CreateTargetGroup" => {
                operations::target_groups::create_target_group(&state, &input, ctx)
            }
            "DeleteTargetGroup" => operations::target_groups::delete_target_group(&state, &input),
            "DescribeTargetGroups" => {
                operations::target_groups::describe_target_groups(&state, &input)
            }
            "RegisterTargets" => operations::target_groups::register_targets(&state, &input),
            "DeregisterTargets" => operations::target_groups::deregister_targets(&state, &input),
            "DescribeTargetHealth" => {
                operations::target_groups::describe_target_health(&state, &input)
            }
            "DescribeTargetGroupAttributes" => {
                operations::target_groups::describe_target_group_attributes(&state, &input)
            }
            "ModifyTargetGroupAttributes" => {
                operations::target_groups::modify_target_group_attributes(&state, &input)
            }

            // Listeners
            "CreateListener" => operations::listeners::create_listener(&state, &input, ctx),
            "DeleteListener" => operations::listeners::delete_listener(&state, &input),
            "DescribeListeners" => operations::listeners::describe_listeners(&state, &input),
            "ModifyListener" => operations::listeners::modify_listener(&state, &input),
            "DescribeListenerCertificates" => {
                operations::listeners::describe_listener_certificates(&state, &input)
            }
            "AddListenerCertificates" => {
                operations::listeners::add_listener_certificates(&state, &input)
            }
            "RemoveListenerCertificates" => {
                operations::listeners::remove_listener_certificates(&state, &input)
            }

            // Rules
            "CreateRule" => operations::rules::create_rule(&state, &input, ctx),
            "DeleteRule" => operations::rules::delete_rule(&state, &input),
            "DescribeRules" => operations::rules::describe_rules(&state, &input),
            "ModifyRule" => operations::rules::modify_rule(&state, &input),
            "SetRulePriorities" => operations::rules::set_rule_priorities(&state, &input),

            // Tags
            "AddTags" => operations::tags::add_tags(&state, &input),
            "RemoveTags" => operations::tags::remove_tags(&state, &input),
            "DescribeTags" => operations::tags::describe_tags(&state, &input),

            // Metadata
            "DescribeAccountLimits" => {
                operations::metadata::describe_account_limits(&state, &input)
            }
            "DescribeSSLPolicies" => operations::metadata::describe_ssl_policies(&state, &input),
            "DescribeLoadBalancerPolicies" => {
                operations::metadata::describe_load_balancer_policies(&state, &input)
            }

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
