mod handler;
mod operations;
mod state;

pub use handler::EventBridgeService;

#[cfg(test)]
mod tests {
    use awsim_core::RequestContext;
    use serde_json::json;

    use super::handler::EventBridgeService;
    use awsim_core::ServiceHandler;

    fn ctx() -> RequestContext {
        RequestContext::new("events", "us-east-1")
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

    // -----------------------------------------------------------------------
    // Event Bus tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_event_bus() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        let result = block_on(svc.handle(
            "CreateEventBus",
            json!({ "Name": "my-bus" }),
            &ctx,
        ))
        .unwrap();
        let arn = result["EventBusArn"].as_str().unwrap();
        assert!(
            arn.starts_with("arn:aws:events:us-east-1:000000000000:event-bus/my-bus"),
            "arn={arn}"
        );
    }

    #[test]
    fn test_create_default_bus_rejected() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateEventBus",
            json!({ "Name": "default" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn test_create_duplicate_bus() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        block_on(svc.handle("CreateEventBus", json!({ "Name": "dup-bus" }), &ctx)).unwrap();
        let err = block_on(svc.handle(
            "CreateEventBus",
            json!({ "Name": "dup-bus" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ResourceAlreadyExistsException");
    }

    #[test]
    fn test_describe_default_event_bus() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        let result = block_on(svc.handle("DescribeEventBus", json!({}), &ctx)).unwrap();
        assert_eq!(result["Name"].as_str().unwrap(), "default");
        assert!(result["Arn"].as_str().unwrap().ends_with("event-bus/default"));
    }

    #[test]
    fn test_list_event_buses_includes_default() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        let result = block_on(svc.handle("ListEventBuses", json!({}), &ctx)).unwrap();
        let buses = result["EventBuses"].as_array().unwrap();
        assert!(buses.iter().any(|b| b["Name"].as_str() == Some("default")));
    }

    #[test]
    fn test_delete_event_bus() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        block_on(svc.handle("CreateEventBus", json!({ "Name": "del-bus" }), &ctx)).unwrap();
        block_on(svc.handle("DeleteEventBus", json!({ "Name": "del-bus" }), &ctx)).unwrap();
        let err = block_on(svc.handle(
            "DescribeEventBus",
            json!({ "Name": "del-bus" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn test_delete_default_bus_rejected() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "DeleteEventBus",
            json!({ "Name": "default" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    // -----------------------------------------------------------------------
    // Rule tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_put_rule_event_pattern() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        let result = block_on(svc.handle(
            "PutRule",
            json!({
                "Name": "my-rule",
                "EventPattern": r#"{"source":["myapp"]}"#,
                "State": "ENABLED",
            }),
            &ctx,
        ))
        .unwrap();
        let arn = result["RuleArn"].as_str().unwrap();
        assert!(arn.contains("rule/default/my-rule"), "arn={arn}");
    }

    #[test]
    fn test_put_rule_schedule() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        let result = block_on(svc.handle(
            "PutRule",
            json!({
                "Name": "sched-rule",
                "ScheduleExpression": "rate(5 minutes)",
                "State": "ENABLED",
            }),
            &ctx,
        ))
        .unwrap();
        assert!(result["RuleArn"].as_str().is_some());
    }

    #[test]
    fn test_put_rule_missing_pattern_and_schedule() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "PutRule",
            json!({ "Name": "bad-rule", "State": "ENABLED" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn test_describe_rule() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "PutRule",
            json!({
                "Name": "desc-rule",
                "EventPattern": r#"{"source":["myapp"]}"#,
                "State": "ENABLED",
                "Description": "A test rule",
            }),
            &ctx,
        ))
        .unwrap();

        let result = block_on(svc.handle(
            "DescribeRule",
            json!({ "Name": "desc-rule" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["Name"].as_str().unwrap(), "desc-rule");
        assert_eq!(result["Description"].as_str().unwrap(), "A test rule");
        assert_eq!(result["State"].as_str().unwrap(), "ENABLED");
    }

    #[test]
    fn test_list_rules() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "PutRule",
            json!({ "Name": "rule-a", "EventPattern": r#"{"source":["x"]}"#, "State": "ENABLED" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "PutRule",
            json!({ "Name": "rule-b", "EventPattern": r#"{"source":["y"]}"#, "State": "ENABLED" }),
            &ctx,
        ))
        .unwrap();
        let result = block_on(svc.handle("ListRules", json!({}), &ctx)).unwrap();
        assert_eq!(result["Rules"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_list_rules_name_prefix() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "PutRule",
            json!({ "Name": "app-rule-1", "EventPattern": r#"{"source":["x"]}"#, "State": "ENABLED" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "PutRule",
            json!({ "Name": "other-rule", "EventPattern": r#"{"source":["y"]}"#, "State": "ENABLED" }),
            &ctx,
        ))
        .unwrap();
        let result = block_on(svc.handle(
            "ListRules",
            json!({ "NamePrefix": "app-" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["Rules"].as_array().unwrap().len(), 1);
        assert_eq!(
            result["Rules"][0]["Name"].as_str().unwrap(),
            "app-rule-1"
        );
    }

    #[test]
    fn test_enable_disable_rule() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "PutRule",
            json!({ "Name": "toggle-rule", "EventPattern": r#"{"source":["x"]}"#, "State": "ENABLED" }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "DisableRule",
            json!({ "Name": "toggle-rule" }),
            &ctx,
        ))
        .unwrap();

        let desc = block_on(svc.handle(
            "DescribeRule",
            json!({ "Name": "toggle-rule" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(desc["State"].as_str().unwrap(), "DISABLED");

        block_on(svc.handle(
            "EnableRule",
            json!({ "Name": "toggle-rule" }),
            &ctx,
        ))
        .unwrap();

        let desc2 = block_on(svc.handle(
            "DescribeRule",
            json!({ "Name": "toggle-rule" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(desc2["State"].as_str().unwrap(), "ENABLED");
    }

    #[test]
    fn test_delete_rule() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "PutRule",
            json!({ "Name": "del-rule", "EventPattern": r#"{"source":["x"]}"#, "State": "ENABLED" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "DeleteRule",
            json!({ "Name": "del-rule" }),
            &ctx,
        ))
        .unwrap();
        let err = block_on(svc.handle(
            "DescribeRule",
            json!({ "Name": "del-rule" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    // -----------------------------------------------------------------------
    // Target tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_put_and_list_targets() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "PutRule",
            json!({ "Name": "tgt-rule", "EventPattern": r#"{"source":["x"]}"#, "State": "ENABLED" }),
            &ctx,
        ))
        .unwrap();

        let result = block_on(svc.handle(
            "PutTargets",
            json!({
                "Rule": "tgt-rule",
                "Targets": [
                    { "Id": "t1", "Arn": "arn:aws:sqs:us-east-1:000000000000:my-queue" },
                    { "Id": "t2", "Arn": "arn:aws:lambda:us-east-1:000000000000:function:my-fn" },
                ],
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["FailedEntryCount"].as_u64().unwrap(), 0);

        let list = block_on(svc.handle(
            "ListTargetsByRule",
            json!({ "Rule": "tgt-rule" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(list["Targets"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_remove_targets() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "PutRule",
            json!({ "Name": "rm-rule", "EventPattern": r#"{"source":["x"]}"#, "State": "ENABLED" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "PutTargets",
            json!({
                "Rule": "rm-rule",
                "Targets": [
                    { "Id": "t1", "Arn": "arn:aws:sqs:us-east-1:000000000000:q1" },
                    { "Id": "t2", "Arn": "arn:aws:sqs:us-east-1:000000000000:q2" },
                ],
            }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "RemoveTargets",
            json!({ "Rule": "rm-rule", "Ids": ["t1"] }),
            &ctx,
        ))
        .unwrap();

        let list = block_on(svc.handle(
            "ListTargetsByRule",
            json!({ "Rule": "rm-rule" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(list["Targets"].as_array().unwrap().len(), 1);
        assert_eq!(list["Targets"][0]["Id"].as_str().unwrap(), "t2");
    }

    // -----------------------------------------------------------------------
    // PutEvents tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_put_events_basic() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        let result = block_on(svc.handle(
            "PutEvents",
            json!({
                "Entries": [
                    {
                        "Source": "myapp",
                        "DetailType": "OrderCreated",
                        "Detail": r#"{"orderId":"123"}"#,
                    }
                ],
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["FailedEntryCount"].as_u64().unwrap(), 0);
        let entries = result["Entries"].as_array().unwrap();
        assert!(entries[0]["EventId"].as_str().is_some());
    }

    #[test]
    fn test_put_events_missing_source() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        let result = block_on(svc.handle(
            "PutEvents",
            json!({
                "Entries": [
                    { "DetailType": "OrderCreated", "Detail": r#"{}"# }
                ],
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["FailedEntryCount"].as_u64().unwrap(), 1);
    }

    #[test]
    fn test_put_events_matches_rule() {
        let svc = EventBridgeService::new();
        let ctx = ctx();

        // Create a rule with a matching pattern
        block_on(svc.handle(
            "PutRule",
            json!({
                "Name": "order-rule",
                "EventPattern": r#"{"source":["myapp"],"detail-type":["OrderCreated"]}"#,
                "State": "ENABLED",
            }),
            &ctx,
        ))
        .unwrap();

        // PutEvents should succeed (matching is currently log-only)
        let result = block_on(svc.handle(
            "PutEvents",
            json!({
                "Entries": [{
                    "Source": "myapp",
                    "DetailType": "OrderCreated",
                    "Detail": r#"{"orderId":"42"}"#,
                }],
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["FailedEntryCount"].as_u64().unwrap(), 0);
        assert!(result["Entries"][0]["EventId"].as_str().is_some());
    }

    #[test]
    fn test_put_events_no_match_disabled_rule() {
        let svc = EventBridgeService::new();
        let ctx = ctx();

        block_on(svc.handle(
            "PutRule",
            json!({
                "Name": "disabled-rule",
                "EventPattern": r#"{"source":["myapp"]}"#,
                "State": "DISABLED",
            }),
            &ctx,
        ))
        .unwrap();

        // Event should succeed even though rule is disabled
        let result = block_on(svc.handle(
            "PutEvents",
            json!({
                "Entries": [{
                    "Source": "myapp",
                    "DetailType": "Anything",
                    "Detail": r#"{}"#,
                }],
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["FailedEntryCount"].as_u64().unwrap(), 0);
    }

    // -----------------------------------------------------------------------
    // Tags tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_tag_and_list_tags_for_event_bus() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateEventBus",
            json!({ "Name": "tagged-bus" }),
            &ctx,
        ))
        .unwrap();

        let arn = format!(
            "arn:aws:events:us-east-1:000000000000:event-bus/tagged-bus"
        );

        block_on(svc.handle(
            "TagResource",
            json!({
                "ResourceARN": arn,
                "Tags": [
                    { "Key": "env", "Value": "test" },
                    { "Key": "team", "Value": "platform" },
                ],
            }),
            &ctx,
        ))
        .unwrap();

        let tags_result = block_on(svc.handle(
            "ListTagsForResource",
            json!({ "ResourceARN": arn }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(tags_result["Tags"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_untag_resource() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateEventBus",
            json!({ "Name": "untag-bus" }),
            &ctx,
        ))
        .unwrap();

        let arn = "arn:aws:events:us-east-1:000000000000:event-bus/untag-bus";

        block_on(svc.handle(
            "TagResource",
            json!({
                "ResourceARN": arn,
                "Tags": [{ "Key": "remove-me", "Value": "yes" }],
            }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "UntagResource",
            json!({ "ResourceARN": arn, "TagKeys": ["remove-me"] }),
            &ctx,
        ))
        .unwrap();

        let tags_result = block_on(svc.handle(
            "ListTagsForResource",
            json!({ "ResourceARN": arn }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(tags_result["Tags"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_unknown_operation() {
        let svc = EventBridgeService::new();
        let ctx = ctx();
        let err = block_on(svc.handle("NonExistentOp", json!({}), &ctx)).unwrap_err();
        assert_eq!(err.code, "UnknownOperationException");
    }
}
