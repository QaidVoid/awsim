mod error;
mod ids;
mod operations;
mod state;
mod template;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::CloudFormationState;

/// The AWSim CloudFormation service handler.
pub struct CloudFormationService {
    store: AccountRegionStore<CloudFormationState>,
}

impl CloudFormationService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<CloudFormationState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for CloudFormationService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for CloudFormationService {
    fn service_name(&self) -> &str {
        "cloudformation"
    }

    fn signing_name(&self) -> &str {
        "cloudformation"
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
        debug!(operation, "CloudFormation request");
        let state = self.get_state(ctx);

        match operation {
            // Stacks
            "CreateStack" => operations::stacks::create_stack(&state, &input, ctx),
            "DeleteStack" => operations::stacks::delete_stack(&state, &input, ctx),
            "UpdateStack" => operations::stacks::update_stack(&state, &input, ctx),
            "DescribeStacks" => operations::stacks::describe_stacks(&state, &input),
            "DescribeStackEvents" => operations::stacks::describe_stack_events(&state, &input),
            "DescribeStackResources" => {
                operations::stacks::describe_stack_resources(&state, &input)
            }
            "DescribeStackResource" => operations::stacks::describe_stack_resource(&state, &input),
            "ListStacks" => operations::stacks::list_stacks(&state, &input),
            "ListStackResources" => operations::stacks::list_stack_resources(&state, &input),
            "GetTemplate" => operations::stacks::get_template(&state, &input),
            "GetTemplateSummary" => operations::stacks::get_template_summary(&state, &input),
            "ValidateTemplate" => operations::stacks::validate_template(&state, &input),
            "UpdateTerminationProtection" => {
                operations::stacks::update_termination_protection(&state, &input, ctx)
            }
            "SetStackPolicy" => operations::stacks::set_stack_policy(&state, &input),
            "GetStackPolicy" => operations::stacks::get_stack_policy(&state, &input),

            // Exports / Imports
            "ListExports" => operations::stacks::list_exports(&state, &input),
            "ListImports" => operations::stacks::list_imports(&state, &input),

            // Tagging
            "TagResource" => operations::stacks::tag_resource(&state, &input),
            "UntagResource" => operations::stacks::untag_resource(&state, &input),

            // Signals / Cost
            "SignalResource" => operations::stacks::signal_resource(&state, &input),
            "EstimateTemplateCost" => operations::stacks::estimate_template_cost(&state, &input),

            // Change Sets
            "CreateChangeSet" => operations::change_sets::create_change_set(&state, &input, ctx),
            "ExecuteChangeSet" => operations::change_sets::execute_change_set(&state, &input, ctx),
            "DeleteChangeSet" => operations::change_sets::delete_change_set(&state, &input),
            "DescribeChangeSet" => operations::change_sets::describe_change_set(&state, &input),
            "ListChangeSets" => operations::change_sets::list_change_sets(&state, &input),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    /// Roll back stacks whose `TimeoutInMinutes` deadline elapsed while a
    /// resource is still `CREATE_IN_PROGRESS` (e.g. a custom resource that
    /// never received its SignalResource). The action follows `OnFailure`:
    /// ROLLBACK -> ROLLBACK_COMPLETE (pending resources -> CREATE_FAILED),
    /// DELETE -> DELETE_COMPLETE, DO_NOTHING -> CREATE_FAILED. Idempotent:
    /// the deadline is cleared once acted on.
    async fn tick(&self) {
        let now = ids::now_unix();
        for (_, state) in self.store.iter_all() {
            for mut entry in state.stacks.iter_mut() {
                let stack = entry.value_mut();
                let Some(deadline) = stack.timeout_deadline_secs else {
                    continue;
                };
                if now < deadline {
                    continue;
                }
                let pending = stack
                    .resources
                    .iter()
                    .any(|r| r.resource_status == "CREATE_IN_PROGRESS");
                if !pending {
                    stack.timeout_deadline_secs = None;
                    continue;
                }
                let reason = "Stack creation timed out (TimeoutInMinutes exceeded).".to_string();
                match stack.on_failure.as_str() {
                    "DELETE" => stack.status = "DELETE_COMPLETE".to_string(),
                    "DO_NOTHING" => stack.status = "CREATE_FAILED".to_string(),
                    _ => {
                        for r in &mut stack.resources {
                            if r.resource_status == "CREATE_IN_PROGRESS" {
                                r.resource_status = "CREATE_FAILED".to_string();
                                r.resource_status_reason = Some("Timed out".to_string());
                            }
                        }
                        stack.status = "ROLLBACK_COMPLETE".to_string();
                    }
                }
                stack.status_reason = Some(reason.clone());
                stack.timeout_deadline_secs = None;
                let event = state::StackEvent {
                    event_id: ids::new_uuid(),
                    stack_id: stack.stack_id.clone(),
                    stack_name: stack.stack_name.clone(),
                    logical_resource_id: stack.stack_name.clone(),
                    physical_resource_id: Some(stack.stack_id.clone()),
                    resource_type: "AWS::CloudFormation::Stack".to_string(),
                    timestamp: ids::now_iso8601(),
                    resource_status: stack.status.clone(),
                    resource_status_reason: Some(reason),
                };
                stack.events.push(event);
            }
        }
    }
}

#[cfg(test)]
mod timeout_tests {
    use super::*;
    use serde_json::json;

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        fn noop_clone(_: *const ()) -> RawWaker {
            noop_raw_waker()
        }
        fn noop(_: *const ()) {}
        fn noop_raw_waker() -> RawWaker {
            static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(f);
        loop {
            if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    const CUSTOM_TEMPLATE: &str = r#"{"Resources":{"CR":{"Type":"AWS::CloudFormation::CustomResource","Properties":{"ServiceToken":"arn:aws:lambda:us-east-1:000000000000:function:p"}}}}"#;

    #[test]
    fn timeout_rolls_back_pending_stack_and_is_idempotent() {
        let svc = CloudFormationService::new();
        let ctx = RequestContext::new("cloudformation", "us-east-1");
        let state = svc.store.get(&ctx.account_id, &ctx.region);
        operations::stacks::create_stack(
            &state,
            &json!({ "StackName": "s", "TemplateBody": CUSTOM_TEMPLATE }),
            &ctx,
        )
        .unwrap();
        // Force the deadline into the past.
        state.stacks.get_mut("s").unwrap().timeout_deadline_secs = Some(0);

        block_on(svc.tick());
        {
            let stack = state.stacks.get("s").unwrap();
            assert_eq!(stack.status, "ROLLBACK_COMPLETE");
            assert_eq!(stack.resources[0].resource_status, "CREATE_FAILED");
            assert!(stack.timeout_deadline_secs.is_none());
        }
        // Second tick is a no-op.
        block_on(svc.tick());
        assert_eq!(state.stacks.get("s").unwrap().status, "ROLLBACK_COMPLETE");
    }

    #[test]
    fn timeout_on_failure_delete_marks_delete_complete() {
        let svc = CloudFormationService::new();
        let ctx = RequestContext::new("cloudformation", "us-east-1");
        let state = svc.store.get(&ctx.account_id, &ctx.region);
        operations::stacks::create_stack(
            &state,
            &json!({ "StackName": "s", "TemplateBody": CUSTOM_TEMPLATE, "OnFailure": "DELETE" }),
            &ctx,
        )
        .unwrap();
        state.stacks.get_mut("s").unwrap().timeout_deadline_secs = Some(0);
        block_on(svc.tick());
        assert_eq!(state.stacks.get("s").unwrap().status, "DELETE_COMPLETE");
    }
}
