//! SNS speaks the AWS query protocol, which spells lists as `<member>`
//! elements and maps as `<entry><key/><value/></entry>` pairs.
//!
//! Returning a bare list or a plain object produced XML that botocore
//! refuses to parse, so `list-topics` came back as `[{}]` and
//! `get-topic-attributes` raised a parse error inside the SDK. The admin
//! UI reads the same two shapes, so its SNS pages were blank as well.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_sns::SnsService;
use serde_json::{Value, json};

fn ctx() -> RequestContext {
    RequestContext::new("sns", "us-east-1")
}

fn members(response: &Value, field: &str) -> Vec<Value> {
    response[field]["member"]
        .as_array()
        .unwrap_or_else(|| panic!("{field} should be a member list: {response}"))
        .clone()
}

fn attribute(response: &Value, name: &str) -> Option<String> {
    response["Attributes"]["entry"]
        .as_array()?
        .iter()
        .find(|e| e["key"] == name)
        .map(|e| e["value"].as_str().unwrap_or_default().to_string())
}

#[tokio::test]
async fn list_topics_returns_member_wrapped_rows() {
    let svc = SnsService::new();
    svc.handle("CreateTopic", json!({ "Name": "shape-a" }), &ctx())
        .await
        .expect("CreateTopic");

    let out = svc
        .handle("ListTopics", json!({}), &ctx())
        .await
        .expect("ListTopics");
    let rows = members(&out, "Topics");
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0]["TopicArn"]
            .as_str()
            .is_some_and(|a| a.ends_with("shape-a")),
        "a member should carry the topic ARN: {rows:?}"
    );
}

#[tokio::test]
async fn topic_attributes_are_key_value_entries() {
    let svc = SnsService::new();
    let created = svc
        .handle(
            "CreateTopic",
            json!({ "Name": "shape-attrs", "Attributes": { "DisplayName": "Orders" } }),
            &ctx(),
        )
        .await
        .expect("CreateTopic");
    let arn = created["TopicArn"].as_str().expect("TopicArn");

    let out = svc
        .handle("GetTopicAttributes", json!({ "TopicArn": arn }), &ctx())
        .await
        .expect("GetTopicAttributes");
    assert_eq!(attribute(&out, "DisplayName").as_deref(), Some("Orders"));
    assert_eq!(attribute(&out, "TopicArn").as_deref(), Some(arn));
}

/// The CLI sends attributes as `Attributes.entry.N.key`, which the query
/// parser must fold back into a map before the handler reads it.
#[tokio::test]
async fn create_topic_keeps_attributes_and_tags() {
    let svc = SnsService::new();
    let created = svc
        .handle(
            "CreateTopic",
            json!({
                "Name": "shape-tags",
                "Attributes": { "DisplayName": "Orders" },
                "Tags": [{ "Key": "env", "Value": "dev" }],
            }),
            &ctx(),
        )
        .await
        .expect("CreateTopic");
    let arn = created["TopicArn"].as_str().expect("TopicArn");

    let tags = svc
        .handle("ListTagsForResource", json!({ "ResourceArn": arn }), &ctx())
        .await
        .expect("ListTagsForResource");
    let rows = members(&tags, "Tags");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["Key"], "env");
    assert_eq!(rows[0]["Value"], "dev");
}

#[tokio::test]
async fn subscriptions_are_member_wrapped() {
    let svc = SnsService::new();
    let created = svc
        .handle("CreateTopic", json!({ "Name": "shape-subs" }), &ctx())
        .await
        .expect("CreateTopic");
    let arn = created["TopicArn"].as_str().expect("TopicArn").to_string();

    svc.handle(
        "Subscribe",
        json!({ "TopicArn": arn, "Protocol": "email", "Endpoint": "a@b.c" }),
        &ctx(),
    )
    .await
    .expect("Subscribe");

    for op in ["ListSubscriptions", "ListSubscriptionsByTopic"] {
        let out = svc
            .handle(op, json!({ "TopicArn": arn }), &ctx())
            .await
            .unwrap_or_else(|e| panic!("{op}: {e:?}"));
        assert_eq!(members(&out, "Subscriptions").len(), 1, "{op}");
    }
}
