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
        let tag_list = tags["Tags"].as_array().unwrap();
        assert_eq!(tag_list.len(), 2);
        assert!(
            tag_list
                .iter()
                .all(|t| t.get("Key").is_some() && t.get("Value").is_some())
        );

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
        let tag_list2 = tags2["Tags"].as_array().unwrap();
        assert_eq!(tag_list2.len(), 1);
        assert_eq!(tag_list2[0]["Key"].as_str(), Some("team"));
        assert_eq!(tag_list2[0]["Value"].as_str(), Some("infra"));
    }

    #[test]
    fn test_unknown_operation() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        let err = block_on(svc.handle("Bogus", json!({}), &ctx)).unwrap_err();
        assert_eq!(err.code, "UnknownOperationException");
    }

    #[test]
    fn create_schedule_defaults_timezone_to_utc() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "tz-default",
                "ScheduleExpression": "rate(5 minutes)",
                "Target": {
                    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                },
                "FlexibleTimeWindow": { "Mode": "OFF" },
            }),
            &ctx,
        ))
        .unwrap();
        let desc =
            block_on(svc.handle("GetSchedule", json!({ "Name": "tz-default" }), &ctx)).unwrap();
        assert_eq!(desc["ScheduleExpressionTimezone"], json!("UTC"));
    }

    #[test]
    fn create_schedule_accepts_iana_timezone() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "tz-iana",
                "ScheduleExpression": "cron(0 9 * * ? *)",
                "ScheduleExpressionTimezone": "America/New_York",
                "Target": {
                    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                },
                "FlexibleTimeWindow": { "Mode": "OFF" },
            }),
            &ctx,
        ))
        .unwrap();
        let desc = block_on(svc.handle("GetSchedule", json!({ "Name": "tz-iana" }), &ctx)).unwrap();
        assert_eq!(
            desc["ScheduleExpressionTimezone"],
            json!("America/New_York")
        );
    }

    #[test]
    fn create_schedule_rejects_malformed_timezone() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        for bad in ["nope", "lower/case", "America/", "/New_York", "1Foo/Bar"] {
            let err = block_on(svc.handle(
                "CreateSchedule",
                json!({
                    "Name": format!("tz-bad-{}", bad.replace('/', "-")),
                    "ScheduleExpression": "rate(1 minute)",
                    "ScheduleExpressionTimezone": bad,
                    "Target": {
                        "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                        "RoleArn": "arn:aws:iam::000000000000:role/r",
                    },
                    "FlexibleTimeWindow": { "Mode": "OFF" },
                }),
                &ctx,
            ))
            .unwrap_err();
            assert_eq!(err.code, "ValidationException", "input `{bad}`");
        }
    }

    #[test]
    fn tag_resource_rejects_reserved_aws_prefix() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        let created = block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "tagged",
                "ScheduleExpression": "rate(5 minutes)",
                "Target": {
                    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                },
                "FlexibleTimeWindow": { "Mode": "OFF" },
            }),
            &ctx,
        ))
        .unwrap();
        let arn = created["ScheduleArn"].as_str().unwrap().to_string();
        let err = block_on(svc.handle(
            "TagResource",
            json!({
                "ResourceArn": arn,
                "Tags": { "aws:reserved": "no" },
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn tag_resource_rejects_too_many_tags() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        let created = block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "tagged-too-many",
                "ScheduleExpression": "rate(5 minutes)",
                "Target": {
                    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                },
                "FlexibleTimeWindow": { "Mode": "OFF" },
            }),
            &ctx,
        ))
        .unwrap();
        let arn = created["ScheduleArn"].as_str().unwrap().to_string();
        // 51 tags blows past the AWS-documented 50-tag cap.
        let mut tags = serde_json::Map::new();
        for i in 0..51 {
            tags.insert(format!("k{i}"), json!(format!("v{i}")));
        }
        let err = block_on(svc.handle(
            "TagResource",
            json!({ "ResourceArn": arn, "Tags": tags }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn start_date_and_end_date_round_trip() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "bounded",
                "ScheduleExpression": "rate(5 minutes)",
                "StartDate": "2026-06-01T00:00:00",
                "EndDate": "2026-06-30T00:00:00",
                "Target": {
                    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                },
                "FlexibleTimeWindow": { "Mode": "OFF" },
            }),
            &ctx,
        ))
        .unwrap();
        let desc = block_on(svc.handle("GetSchedule", json!({ "Name": "bounded" }), &ctx)).unwrap();
        assert_eq!(desc["StartDate"], json!("2026-06-01T00:00:00"));
        assert_eq!(desc["EndDate"], json!("2026-06-30T00:00:00"));
    }

    #[test]
    fn start_date_must_precede_end_date() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "inverted",
                "ScheduleExpression": "rate(5 minutes)",
                "StartDate": "2026-06-30T00:00:00",
                "EndDate": "2026-06-01T00:00:00",
                "Target": {
                    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                },
                "FlexibleTimeWindow": { "Mode": "OFF" },
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn start_date_rejects_malformed_timestamp() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        for bad in ["not-a-date", "2026-13-01T00:00:00", "2026-01-01 00:00:00"] {
            let err = block_on(svc.handle(
                "CreateSchedule",
                json!({
                    "Name": format!("bad-{}", bad.replace([' ', ':'], "_")),
                    "ScheduleExpression": "rate(5 minutes)",
                    "StartDate": bad,
                    "Target": {
                        "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                        "RoleArn": "arn:aws:iam::000000000000:role/r",
                    },
                    "FlexibleTimeWindow": { "Mode": "OFF" },
                }),
                &ctx,
            ))
            .unwrap_err();
            assert_eq!(err.code, "ValidationException", "input `{bad}`");
        }
    }

    #[test]
    fn retry_policy_accepts_documented_bounds() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "with-retry",
                "ScheduleExpression": "rate(5 minutes)",
                "Target": {
                    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                    "RetryPolicy": {
                        "MaximumEventAgeInSeconds": 3600,
                        "MaximumRetryAttempts": 5,
                    },
                    "DeadLetterConfig": {
                        "Arn": "arn:aws:sqs:us-east-1:000000000000:dlq",
                    },
                },
                "FlexibleTimeWindow": { "Mode": "OFF" },
            }),
            &ctx,
        ))
        .unwrap();
    }

    #[test]
    fn retry_policy_rejects_out_of_range_age() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        for bad in [0i64, 59, 86_401, 100_000] {
            let err = block_on(svc.handle(
                "CreateSchedule",
                json!({
                    "Name": format!("retry-age-{bad}"),
                    "ScheduleExpression": "rate(5 minutes)",
                    "Target": {
                        "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                        "RoleArn": "arn:aws:iam::000000000000:role/r",
                        "RetryPolicy": { "MaximumEventAgeInSeconds": bad },
                    },
                    "FlexibleTimeWindow": { "Mode": "OFF" },
                }),
                &ctx,
            ))
            .unwrap_err();
            assert_eq!(err.code, "ValidationException", "input {bad}");
        }
    }

    #[test]
    fn retry_policy_rejects_out_of_range_attempts() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        for bad in [-1i64, 186, 1000] {
            let err = block_on(svc.handle(
                "CreateSchedule",
                json!({
                    "Name": format!("retry-att-{}", bad.unsigned_abs()),
                    "ScheduleExpression": "rate(5 minutes)",
                    "Target": {
                        "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                        "RoleArn": "arn:aws:iam::000000000000:role/r",
                        "RetryPolicy": { "MaximumRetryAttempts": bad },
                    },
                    "FlexibleTimeWindow": { "Mode": "OFF" },
                }),
                &ctx,
            ))
            .unwrap_err();
            assert_eq!(err.code, "ValidationException", "input {bad}");
        }
    }

    #[test]
    fn dead_letter_config_rejects_non_sqs_arn() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "bad-dlq",
                "ScheduleExpression": "rate(5 minutes)",
                "Target": {
                    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                    "DeadLetterConfig": {
                        "Arn": "arn:aws:sns:us-east-1:000000000000:topic"
                    },
                },
                "FlexibleTimeWindow": { "Mode": "OFF" },
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn kms_key_arn_persisted_when_well_formed() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "encrypted",
                "ScheduleExpression": "rate(5 minutes)",
                "KmsKeyArn": "arn:aws:kms:us-east-1:000000000000:key/abc-123",
                "Target": {
                    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                },
                "FlexibleTimeWindow": { "Mode": "OFF" },
            }),
            &ctx,
        ))
        .unwrap();
        let desc =
            block_on(svc.handle("GetSchedule", json!({ "Name": "encrypted" }), &ctx)).unwrap();
        assert_eq!(
            desc["KmsKeyArn"],
            json!("arn:aws:kms:us-east-1:000000000000:key/abc-123")
        );
    }

    #[test]
    fn kms_key_arn_rejects_malformed_shapes() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        for bad in [
            "not-an-arn",
            // wrong service
            "arn:aws:s3:us-east-1:000000000000:key/abc",
            // missing region
            "arn:aws:kms::000000000000:key/abc",
            // wrong resource type
            "arn:aws:kms:us-east-1:000000000000:alias/foo",
            // missing key id
            "arn:aws:kms:us-east-1:000000000000:key/",
        ] {
            let err = block_on(svc.handle(
                "CreateSchedule",
                json!({
                    "Name": format!("k-{}", &bad[..5.min(bad.len())]),
                    "ScheduleExpression": "rate(5 minutes)",
                    "KmsKeyArn": bad,
                    "Target": {
                        "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                        "RoleArn": "arn:aws:iam::000000000000:role/r",
                    },
                    "FlexibleTimeWindow": { "Mode": "OFF" },
                }),
                &ctx,
            ))
            .unwrap_err();
            assert_eq!(err.code, "ValidationException", "input `{bad}`");
        }
    }

    #[test]
    fn list_schedules_paginates_with_next_token() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        for i in 0..3 {
            block_on(svc.handle(
                "CreateSchedule",
                json!({
                    "Name": format!("p-{i}"),
                    "ScheduleExpression": "rate(1 hour)",
                    "Target": {
                        "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                        "RoleArn": "arn:aws:iam::000000000000:role/r",
                    },
                    "FlexibleTimeWindow": { "Mode": "OFF" },
                }),
                &ctx,
            ))
            .unwrap();
        }
        let page1 =
            block_on(svc.handle("ListSchedules", json!({ "MaxResults": 1 }), &ctx)).unwrap();
        assert_eq!(page1["Schedules"].as_array().unwrap().len(), 1);
        let token = page1["NextToken"].as_str().unwrap().to_string();
        let page2 = block_on(svc.handle(
            "ListSchedules",
            json!({ "MaxResults": 5, "NextToken": token }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(page2["Schedules"].as_array().unwrap().len(), 2);
        assert!(page2.get("NextToken").is_none());
    }

    #[test]
    fn list_schedule_groups_paginates_with_next_token() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        for i in 0..3 {
            block_on(svc.handle(
                "CreateScheduleGroup",
                json!({ "Name": format!("g-{i}") }),
                &ctx,
            ))
            .unwrap();
        }
        let page1 =
            block_on(svc.handle("ListScheduleGroups", json!({ "MaxResults": 2 }), &ctx)).unwrap();
        assert_eq!(page1["ScheduleGroups"].as_array().unwrap().len(), 2);
        assert!(page1["NextToken"].as_str().is_some());
    }

    #[test]
    fn create_schedule_rejects_malformed_name() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        let too_long = "a".repeat(65);
        for bad in [
            "",
            "with space",
            "with/slash",
            "with*star",
            too_long.as_str(),
        ] {
            let err = block_on(svc.handle(
                "CreateSchedule",
                json!({
                    "Name": bad,
                    "ScheduleExpression": "rate(1 hour)",
                    "Target": {
                        "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                        "RoleArn": "arn:aws:iam::000000000000:role/r",
                    },
                    "FlexibleTimeWindow": { "Mode": "OFF" },
                }),
                &ctx,
            ))
            .unwrap_err();
            assert_eq!(err.code, "ValidationException", "input `{bad}`");
        }
    }

    #[test]
    fn create_schedule_group_rejects_malformed_name() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        let err =
            block_on(svc.handle("CreateScheduleGroup", json!({ "Name": "bad group!" }), &ctx))
                .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn create_schedule_accepts_dotted_name() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        // Allowed: alphanumerics + `-_.`
        block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "deploy.daily-cron_v1",
                "ScheduleExpression": "rate(1 hour)",
                "Target": {
                    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                },
                "FlexibleTimeWindow": { "Mode": "OFF" },
            }),
            &ctx,
        ))
        .unwrap();
    }

    #[test]
    fn at_expression_with_delete_action_round_trips() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "one-shot",
                "ScheduleExpression": "at(2030-01-15T09:30:00)",
                "ActionAfterCompletion": "DELETE",
                "Target": {
                    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                },
                "FlexibleTimeWindow": { "Mode": "OFF" },
            }),
            &ctx,
        ))
        .unwrap();
        let desc =
            block_on(svc.handle("GetSchedule", json!({ "Name": "one-shot" }), &ctx)).unwrap();
        assert_eq!(desc["ActionAfterCompletion"], json!("DELETE"));
        assert_eq!(desc["ScheduleExpression"], json!("at(2030-01-15T09:30:00)"));
    }

    #[test]
    fn delete_action_rejected_for_recurring_expression() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "bad-delete",
                "ScheduleExpression": "rate(5 minutes)",
                "ActionAfterCompletion": "DELETE",
                "Target": {
                    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                },
                "FlexibleTimeWindow": { "Mode": "OFF" },
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("at(..."), "{}", err.message);
    }

    #[test]
    fn schedule_expression_rejects_malformed_forms() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        for bad in [
            "garbage",
            "at(not-a-date)",
            "at(2030-13-01T00:00:00)", // month 13
            "at(2030-01-32T00:00:00)", // day 32
            "at(2030-01-01T25:00:00)", // hour 25
            "rate(0 minutes)",
            "rate(5 fortnights)",
            "cron(only 4 fields here)",
        ] {
            let err = block_on(svc.handle(
                "CreateSchedule",
                json!({
                    "Name": format!("bad-expr-{}", &bad[..5.min(bad.len())]),
                    "ScheduleExpression": bad,
                    "Target": {
                        "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                        "RoleArn": "arn:aws:iam::000000000000:role/r",
                    },
                    "FlexibleTimeWindow": { "Mode": "OFF" },
                }),
                &ctx,
            ))
            .unwrap_err();
            assert_eq!(err.code, "ValidationException", "input `{bad}`");
        }
    }

    #[test]
    fn flexible_time_window_accepts_flexible_mode_with_window() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "ftw-flex",
                "ScheduleExpression": "rate(1 hour)",
                "Target": {
                    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                },
                "FlexibleTimeWindow": { "Mode": "FLEXIBLE", "MaximumWindowInMinutes": 15 },
            }),
            &ctx,
        ))
        .unwrap();
    }

    #[test]
    fn flexible_time_window_requires_window_minutes_when_flexible() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "ftw-missing",
                "ScheduleExpression": "rate(1 hour)",
                "Target": {
                    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                },
                "FlexibleTimeWindow": { "Mode": "FLEXIBLE" },
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("MaximumWindowInMinutes"));
    }

    #[test]
    fn flexible_time_window_rejects_minutes_with_off_mode() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "ftw-off-with-mins",
                "ScheduleExpression": "rate(1 hour)",
                "Target": {
                    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                },
                "FlexibleTimeWindow": { "Mode": "OFF", "MaximumWindowInMinutes": 10 },
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn flexible_time_window_rejects_window_outside_documented_range() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        for bad in [0, 1441] {
            let err = block_on(svc.handle(
                "CreateSchedule",
                json!({
                    "Name": format!("ftw-{bad}"),
                    "ScheduleExpression": "rate(1 hour)",
                    "Target": {
                        "Arn": "arn:aws:lambda:us-east-1:000000000000:function:f",
                        "RoleArn": "arn:aws:iam::000000000000:role/r",
                    },
                    "FlexibleTimeWindow": { "Mode": "FLEXIBLE", "MaximumWindowInMinutes": bad },
                }),
                &ctx,
            ))
            .unwrap_err();
            assert_eq!(err.code, "ValidationException", "input {bad}");
        }
    }

    #[test]
    fn universal_target_arn_accepts_documented_service_action() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "sqs-send",
                "ScheduleExpression": "rate(1 minute)",
                "Target": {
                    "Arn": "arn:aws:scheduler:::aws-sdk:sqs:sendMessage",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                },
                "FlexibleTimeWindow": { "Mode": "OFF" },
            }),
            &ctx,
        ))
        .unwrap();
    }

    #[test]
    fn target_arn_rejects_malformed_universal() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        for bad in [
            // missing aws-sdk prefix
            "arn:aws:scheduler:::custom:sqs:sendMessage",
            // empty service
            "arn:aws:scheduler:::aws-sdk::sendMessage",
            // service has uppercase
            "arn:aws:scheduler:::aws-sdk:SQS:sendMessage",
            // empty action
            "arn:aws:scheduler:::aws-sdk:sqs:",
            // action starts with digit
            "arn:aws:scheduler:::aws-sdk:sqs:1send",
        ] {
            let err = block_on(svc.handle(
                "CreateSchedule",
                json!({
                    "Name": format!("bad-{}", &bad.split(':').next_back().unwrap_or("x")),
                    "ScheduleExpression": "rate(1 minute)",
                    "Target": { "Arn": bad, "RoleArn": "arn:aws:iam::000000000000:role/r" },
                    "FlexibleTimeWindow": { "Mode": "OFF" },
                }),
                &ctx,
            ))
            .unwrap_err();
            assert_eq!(err.code, "ValidationException", "input `{bad}`");
        }
    }

    #[test]
    fn target_arn_rejects_missing_arn_prefix() {
        let svc = SchedulerService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateSchedule",
            json!({
                "Name": "no-arn",
                "ScheduleExpression": "rate(1 minute)",
                "Target": {
                    "Arn": "lambda:function:foo",
                    "RoleArn": "arn:aws:iam::000000000000:role/r",
                },
                "FlexibleTimeWindow": { "Mode": "OFF" },
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
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
