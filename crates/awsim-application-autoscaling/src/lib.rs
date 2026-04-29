//! Application Auto Scaling emulator.
//!
//! Stores scalable targets, policies, and scheduled actions per namespace
//! (`ecs`, `lambda`, `dynamodb`, etc.). The emulator never executes scaling
//! decisions — `DescribeScalingActivities` always returns an empty list.

mod operations;
pub mod state;

pub use state::AppAutoScalingState;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

pub struct AppAutoScalingService {
    store: AccountRegionStore<AppAutoScalingState>,
}

impl AppAutoScalingService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<AppAutoScalingState> {
        self.store.clone()
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<AppAutoScalingState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for AppAutoScalingService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for AppAutoScalingService {
    fn service_name(&self) -> &str {
        "application-autoscaling"
    }

    fn signing_name(&self) -> &str {
        "application-autoscaling"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![]
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "AppAutoScaling request");
        let state = self.get_state(ctx);
        match operation {
            "RegisterScalableTarget" => operations::register_scalable_target(&state, &input, ctx),
            "DeregisterScalableTarget" => {
                operations::deregister_scalable_target(&state, &input, ctx)
            }
            "DescribeScalableTargets" => operations::describe_scalable_targets(&state, &input, ctx),
            "PutScalingPolicy" => operations::put_scaling_policy(&state, &input, ctx),
            "DeleteScalingPolicy" => operations::delete_scaling_policy(&state, &input, ctx),
            "DescribeScalingPolicies" => operations::describe_scaling_policies(&state, &input, ctx),
            "PutScheduledAction" => operations::put_scheduled_action(&state, &input, ctx),
            "DeleteScheduledAction" => operations::delete_scheduled_action(&state, &input, ctx),
            "DescribeScheduledActions" => {
                operations::describe_scheduled_actions(&state, &input, ctx)
            }
            "DescribeScalingActivities" => {
                operations::describe_scaling_activities(&state, &input, ctx)
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all = state::AppAutoScalingSnapshot {
            targets: vec![],
            policies: vec![],
            scheduled_actions: vec![],
        };
        for (_, st) in self.store.iter_all() {
            let s = st.to_snapshot();
            all.targets.extend(s.targets);
            all.policies.extend(s.policies);
            all.scheduled_actions.extend(s.scheduled_actions);
        }
        serde_json::to_vec(&all).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: state::AppAutoScalingSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;
        let st = self.store.get("000000000000", "us-east-1");
        st.restore_from_snapshot(snap);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> RequestContext {
        RequestContext::new("application-autoscaling", "us-east-1")
    }

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

    #[test]
    fn register_target_attach_policy_describe_lifecycle() {
        let svc = AppAutoScalingService::new();
        let ctx = ctx();

        block_on(svc.handle(
            "RegisterScalableTarget",
            json!({
                "ServiceNamespace": "ecs",
                "ResourceId": "service/cluster-1/web",
                "ScalableDimension": "ecs:service:DesiredCount",
                "MinCapacity": 1,
                "MaxCapacity": 10
            }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "PutScalingPolicy",
            json!({
                "PolicyName": "cpu-target",
                "ServiceNamespace": "ecs",
                "ResourceId": "service/cluster-1/web",
                "ScalableDimension": "ecs:service:DesiredCount",
                "PolicyType": "TargetTrackingScaling",
                "TargetTrackingScalingPolicyConfiguration": {
                    "TargetValue": 50.0,
                    "PredefinedMetricSpecification": {"PredefinedMetricType": "ECSServiceAverageCPUUtilization"}
                }
            }),
            &ctx,
        ))
        .unwrap();

        let described = block_on(svc.handle(
            "DescribeScalableTargets",
            json!({ "ServiceNamespace": "ecs" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(described["ScalableTargets"].as_array().unwrap().len(), 1);

        let policies = block_on(svc.handle(
            "DescribeScalingPolicies",
            json!({ "ServiceNamespace": "ecs" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(policies["ScalingPolicies"].as_array().unwrap().len(), 1);

        // Deregister cascades to policies
        block_on(svc.handle(
            "DeregisterScalableTarget",
            json!({
                "ServiceNamespace": "ecs",
                "ResourceId": "service/cluster-1/web",
                "ScalableDimension": "ecs:service:DesiredCount"
            }),
            &ctx,
        ))
        .unwrap();

        let after = block_on(svc.handle(
            "DescribeScalingPolicies",
            json!({ "ServiceNamespace": "ecs" }),
            &ctx,
        ))
        .unwrap();
        assert!(after["ScalingPolicies"].as_array().unwrap().is_empty());
    }

    #[test]
    fn cannot_attach_policy_without_target() {
        let svc = AppAutoScalingService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "PutScalingPolicy",
            json!({
                "PolicyName": "p",
                "ServiceNamespace": "lambda",
                "ResourceId": "function:f:provisioned",
                "ScalableDimension": "lambda:function:ProvisionedConcurrency",
                "PolicyType": "TargetTrackingScaling"
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ObjectNotFoundException");
    }
}
