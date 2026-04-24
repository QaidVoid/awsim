use crate::chk;
use crate::runner::common::*;

pub async fn test_eventbridge(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_eventbridge::Client::new(&config);
    let mut results = Vec::new();

    // CreateEventBus
    let bus_r = client
        .create_event_bus()
        .name("conformance-bus")
        .send()
        .await;
    let bus_arn = bus_r
        .as_ref()
        .ok()
        .and_then(|r| r.event_bus_arn.clone())
        .unwrap_or_else(|| {
            "arn:aws:events:us-east-1:000000000000:event-bus/conformance-bus".to_string()
        });
    results.push(chk!("CreateEventBus", bus_r, verbose));

    // ListEventBuses
    results.push(chk!(
        "ListEventBuses",
        client.list_event_buses().send().await,
        verbose
    ));

    // DescribeEventBus
    results.push(chk!(
        "DescribeEventBus",
        client
            .describe_event_bus()
            .name("conformance-bus")
            .send()
            .await,
        verbose
    ));

    // PutRule
    let rule_r = client
        .put_rule()
        .name("conformance-rule")
        .event_bus_name("conformance-bus")
        .schedule_expression("rate(5 minutes)")
        .state(aws_sdk_eventbridge::types::RuleState::Enabled)
        .send()
        .await;
    results.push(chk!("PutRule", rule_r, verbose));

    // ListRules
    results.push(chk!(
        "ListRules",
        client
            .list_rules()
            .event_bus_name("conformance-bus")
            .send()
            .await,
        verbose
    ));

    // DescribeRule
    results.push(chk!(
        "DescribeRule",
        client
            .describe_rule()
            .name("conformance-rule")
            .event_bus_name("conformance-bus")
            .send()
            .await,
        verbose
    ));

    // PutTargets
    results.push(chk!(
        "PutTargets",
        client
            .put_targets()
            .rule("conformance-rule")
            .event_bus_name("conformance-bus")
            .targets(
                aws_sdk_eventbridge::types::Target::builder()
                    .id("conformance-target")
                    .arn("arn:aws:lambda:us-east-1:000000000000:function:conformance-fn")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTargetsByRule
    results.push(chk!(
        "ListTargetsByRule",
        client
            .list_targets_by_rule()
            .rule("conformance-rule")
            .event_bus_name("conformance-bus")
            .send()
            .await,
        verbose
    ));

    // PutEvents
    results.push(chk!(
        "PutEvents",
        client
            .put_events()
            .entries(
                aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
                    .source("conformance.test")
                    .detail_type("ConformanceEvent")
                    .detail(r#"{"key":"value"}"#)
                    .event_bus_name("conformance-bus")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // CreateArchive
    results.push(chk!(
        "CreateArchive",
        client
            .create_archive()
            .archive_name("conformance-archive")
            .event_source_arn(&bus_arn)
            .send()
            .await,
        verbose
    ));

    // ListArchives
    results.push(chk!(
        "ListArchives",
        client.list_archives().send().await,
        verbose
    ));

    // DescribeArchive
    results.push(chk!(
        "DescribeArchive",
        client
            .describe_archive()
            .archive_name("conformance-archive")
            .send()
            .await,
        verbose
    ));

    // TagResource (EventBridge) — tag the event bus ARN
    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource_arn(&bus_arn)
            .tags(
                aws_sdk_eventbridge::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTagsForResource (EventBridge)
    results.push(chk!(
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_arn(&bus_arn)
            .send()
            .await,
        verbose
    ));

    // UntagResource (EventBridge)
    results.push(chk!(
        "UntagResource",
        client
            .untag_resource()
            .resource_arn(&bus_arn)
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // RemoveTargets
    results.push(chk!(
        "RemoveTargets",
        client
            .remove_targets()
            .rule("conformance-rule")
            .event_bus_name("conformance-bus")
            .ids("conformance-target")
            .send()
            .await,
        verbose
    ));

    // DeleteArchive
    results.push(chk!(
        "DeleteArchive",
        client
            .delete_archive()
            .archive_name("conformance-archive")
            .send()
            .await,
        verbose
    ));

    // DeleteRule
    results.push(chk!(
        "DeleteRule",
        client
            .delete_rule()
            .name("conformance-rule")
            .event_bus_name("conformance-bus")
            .send()
            .await,
        verbose
    ));

    // DeleteEventBus
    results.push(chk!(
        "DeleteEventBus",
        client
            .delete_event_bus()
            .name("conformance-bus")
            .send()
            .await,
        verbose
    ));

    results
}
