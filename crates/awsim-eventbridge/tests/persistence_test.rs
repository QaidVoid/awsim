//! EventBridge buses, rules and targets must survive a restart. A rule
//! that vanishes takes its targets with it, so a restored setup would
//! silently stop routing events.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_eventbridge::EventBridgeService;
use serde_json::json;

fn ctx() -> RequestContext {
    RequestContext::new("events", "us-east-1")
}

fn round_trip(svc: &EventBridgeService) -> EventBridgeService {
    let bytes = svc.snapshot().expect("snapshot");
    let restored = EventBridgeService::new();
    restored.restore(&bytes).expect("restore");
    restored
}

#[tokio::test]
async fn custom_bus_survives_round_trip() {
    let svc = EventBridgeService::new();
    svc.handle("CreateEventBus", json!({ "Name": "custom" }), &ctx())
        .await
        .expect("CreateEventBus");

    let restored = round_trip(&svc);
    let listed = restored
        .handle("ListEventBuses", json!({}), &ctx())
        .await
        .expect("ListEventBuses");
    let names: Vec<String> = listed["EventBuses"]
        .as_array()
        .expect("EventBuses")
        .iter()
        .map(|b| b["Name"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(names.iter().any(|n| n == "custom"), "got {names:?}");
}

#[tokio::test]
async fn rule_survives_round_trip() {
    let svc = EventBridgeService::new();
    svc.handle(
        "PutRule",
        json!({
            "Name": "my-rule",
            "EventPattern": r#"{"source":["app"]}"#,
            "State": "ENABLED",
        }),
        &ctx(),
    )
    .await
    .expect("PutRule");

    let restored = round_trip(&svc);
    let described = restored
        .handle("DescribeRule", json!({ "Name": "my-rule" }), &ctx())
        .await
        .expect("rule should exist after restore");
    assert_eq!(described["Name"], "my-rule");
    assert_eq!(described["State"], "ENABLED");
}

/// Targets live inside the rule, so losing them is silent: the rule is
/// still there and simply routes nowhere.
#[tokio::test]
async fn rule_targets_survive_round_trip() {
    let svc = EventBridgeService::new();
    svc.handle(
        "PutRule",
        json!({ "Name": "with-targets", "EventPattern": r#"{"source":["app"]}"# }),
        &ctx(),
    )
    .await
    .expect("PutRule");
    svc.handle(
        "PutTargets",
        json!({
            "Rule": "with-targets",
            "Targets": [{
                "Id": "t1",
                "Arn": "arn:aws:sqs:us-east-1:000000000000:queue",
            }],
        }),
        &ctx(),
    )
    .await
    .expect("PutTargets");

    let restored = round_trip(&svc);
    let listed = restored
        .handle(
            "ListTargetsByRule",
            json!({ "Rule": "with-targets" }),
            &ctx(),
        )
        .await
        .expect("ListTargetsByRule");
    let count = listed["Targets"].as_array().map(|a| a.len()).unwrap_or(0);
    assert_eq!(count, 1, "target lost across restore: {listed:?}");
}

#[tokio::test]
async fn restore_is_implemented() {
    let svc = EventBridgeService::new();
    let bytes = svc.snapshot().expect("snapshot");
    assert!(svc.restore(&bytes).is_ok(), "restore should be implemented");
}
