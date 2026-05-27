mod operations;
mod state;

pub use state::SchedulerState;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use state::SchedulerStateSnapshot;

/// The EventBridge Scheduler service handler.
pub struct SchedulerService {
    store: AccountRegionStore<SchedulerState>,
}

impl SchedulerService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<SchedulerState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for SchedulerService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for SchedulerService {
    fn service_name(&self) -> &str {
        "scheduler"
    }

    fn signing_name(&self) -> &str {
        "scheduler"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "POST",
                path_pattern: "/schedules/{Name}",
                operation: "CreateSchedule",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/schedules/{Name}",
                operation: "GetSchedule",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/schedules",
                operation: "ListSchedules",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/schedules/{Name}",
                operation: "DeleteSchedule",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/schedules/{Name}",
                operation: "UpdateSchedule",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/schedule-groups/{Name}",
                operation: "CreateScheduleGroup",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/schedule-groups/{Name}",
                operation: "GetScheduleGroup",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/schedule-groups",
                operation: "ListScheduleGroups",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/schedule-groups/{Name}",
                operation: "DeleteScheduleGroup",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/tags/{ResourceArn}",
                operation: "TagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/tags/{ResourceArn}",
                operation: "UntagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/tags/{ResourceArn}",
                operation: "ListTagsForResource",
                required_query_param: None,
            },
        ]
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "EventBridge Scheduler request");
        let state = self.get_state(ctx);

        match operation {
            // Schedule operations
            "CreateSchedule" => operations::schedules::create_schedule(&state, &input, ctx),
            "GetSchedule" => operations::schedules::get_schedule(&state, &input, ctx),
            "ListSchedules" => operations::schedules::list_schedules(&state, &input, ctx),
            "DeleteSchedule" => operations::schedules::delete_schedule(&state, &input, ctx),
            "UpdateSchedule" => operations::schedules::update_schedule(&state, &input, ctx),

            // Schedule Group operations
            "CreateScheduleGroup" => operations::groups::create_schedule_group(&state, &input, ctx),
            "GetScheduleGroup" => operations::groups::get_schedule_group(&state, &input, ctx),
            "ListScheduleGroups" => operations::groups::list_schedule_groups(&state, &input, ctx),
            "DeleteScheduleGroup" => operations::groups::delete_schedule_group(&state, &input, ctx),

            // Tagging
            "TagResource" => operations::tags::tag_resource(&state, &input, ctx),
            "UntagResource" => operations::tags::untag_resource(&state, &input, ctx),
            "ListTagsForResource" => operations::tags::list_tags_for_resource(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut snap = SchedulerStateSnapshot {
            schedules: vec![],
            schedule_groups: vec![],
        };

        for (_, state) in self.store.iter_all() {
            let s = state.to_snapshot();
            snap.schedules.extend(s.schedules);
            snap.schedule_groups.extend(s.schedule_groups);
        }

        serde_json::to_vec(&snap).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snapshot: SchedulerStateSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;

        let state = self.store.get("000000000000", "us-east-1");
        state.restore_from_snapshot(snapshot);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use awsim_core::{RequestContext, ServiceHandler};
    use serde_json::json;

    use super::SchedulerService;

    fn ctx() -> RequestContext {
        RequestContext::new("scheduler", "us-east-1")
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
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => {}
            }
        }
    }

    #[test]
    fn test_tag_resource_and_list() {
        let svc = SchedulerService::new();
        let ctx = ctx();

        // Create a schedule first so we have an ARN to tag
        let created = block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "my-schedule",
                "ScheduleExpression": "rate(1 hour)",
                "Target": { "Arn": "arn:aws:lambda:us-east-1:123:function:fn", "RoleArn": "arn:aws:iam::123:role/r" },
                "FlexibleTimeWindow": { "Mode": "OFF" },
            }),
            &ctx,
        ))
        .unwrap();
        let schedule_arn = created["ScheduleArn"].as_str().unwrap().to_string();

        // Tag it
        block_on(svc.handle(
            "TagResource",
            json!({
                "ResourceArn": schedule_arn,
                "Tags": { "env": "prod", "team": "infra" }
            }),
            &ctx,
        ))
        .unwrap();

        let tags = block_on(svc.handle(
            "ListTagsForResource",
            json!({ "ResourceArn": schedule_arn }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(tags["Tags"].as_object().unwrap().len(), 2);

        // Untag one
        block_on(svc.handle(
            "UntagResource",
            json!({ "ResourceArn": schedule_arn, "TagKeys": ["env"] }),
            &ctx,
        ))
        .unwrap();

        let tags2 = block_on(svc.handle(
            "ListTagsForResource",
            json!({ "ResourceArn": schedule_arn }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(tags2["Tags"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn test_unknown_operation() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        let err = block_on(svc.handle("Bogus", json!({}), &ctx)).unwrap_err();
        assert_eq!(err.code, "UnknownOperationException");
    }

    #[test]
    fn client_token_replay_returns_cached_schedule_arn() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        let input = json!({
            "Name": "every-5m",
            "ScheduleExpression": "rate(5 minutes)",
            "Target": {
                "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                "RoleArn": "arn:aws:iam::000000000000:role/scheduler",
            },
            "FlexibleTimeWindow": { "Mode": "OFF" },
            "ClientToken": "ct-abc",
        });
        let first = block_on(svc.handle("CreateSchedule", input.clone(), &ctx)).unwrap();
        let arn = first["ScheduleArn"].as_str().unwrap().to_string();
        // Same token + same body must return the cached payload.
        let second = block_on(svc.handle("CreateSchedule", input, &ctx)).unwrap();
        assert_eq!(second["ScheduleArn"], json!(arn));
        let listed = block_on(svc.handle("ListSchedules", json!({}), &ctx)).unwrap();
        assert_eq!(listed["Schedules"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn client_token_mismatch_raises_idempotency_exception() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "first",
                "ScheduleExpression": "rate(5 minutes)",
                "Target": { "Arn": "arn:aws:lambda::000000000000:function:a", "RoleArn": "arn:aws:iam::000000000000:role/r" },
                "FlexibleTimeWindow": { "Mode": "OFF" },
                "ClientToken": "ct-xyz",
            }),
            &ctx,
        ))
        .unwrap();
        let err = block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "different",
                "ScheduleExpression": "rate(10 minutes)",
                "Target": { "Arn": "arn:aws:lambda::000000000000:function:b", "RoleArn": "arn:aws:iam::000000000000:role/r" },
                "FlexibleTimeWindow": { "Mode": "OFF" },
                "ClientToken": "ct-xyz",
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "IdempotencyParameterMismatchException");
    }
}
