//! Amazon MQ names every request and response field in camelCase.
//!
//! AWSim used the PascalCase model names, so an SDK's `CreateBroker`
//! was rejected with "BrokerName is required" and every describe came
//! back empty. This pins the wire spelling on the paths an SDK touches
//! first.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_mq::MqService;
use serde_json::{Value, json};

fn ctx() -> RequestContext {
    RequestContext::new("mq", "us-east-1")
}

fn create_input(name: &str) -> Value {
    json!({
        "brokerName": name,
        "engineType": "RABBITMQ",
        "engineVersion": "3.13",
        "hostInstanceType": "mq.t3.micro",
        "deploymentMode": "SINGLE_INSTANCE",
        "publiclyAccessible": true,
        "users": [{ "username": "admin", "password": "passwordpassword" }],
    })
}

async fn broker(svc: &MqService, name: &str) -> String {
    let created = svc
        .handle("CreateBroker", create_input(name), &ctx())
        .await
        .expect("CreateBroker");
    created["brokerId"]
        .as_str()
        .unwrap_or_else(|| panic!("CreateBroker should answer with brokerId: {created}"))
        .to_string()
}

#[tokio::test]
async fn create_broker_reads_camel_case_input() {
    let svc = MqService::new();
    let id = broker(&svc, "wire-create").await;
    assert!(id.starts_with("b-"), "{id}");
}

/// The PascalCase spelling is the SDK model name, not the wire name, so
/// it must not satisfy the required-field check.
#[tokio::test]
async fn pascal_case_input_is_not_accepted() {
    let svc = MqService::new();
    let err = svc
        .handle(
            "CreateBroker",
            json!({
                "BrokerName": "wire-pascal",
                "EngineType": "RABBITMQ",
                "EngineVersion": "3.13",
                "HostInstanceType": "mq.t3.micro",
            }),
            &ctx(),
        )
        .await
        .expect_err("PascalCase is not the MQ wire spelling");
    assert_eq!(err.code, "BadRequestException", "{err:?}");
}

#[tokio::test]
async fn describe_broker_answers_in_camel_case() {
    let svc = MqService::new();
    let id = broker(&svc, "wire-describe").await;

    let out = svc
        .handle("DescribeBroker", json!({ "brokerId": id.clone() }), &ctx())
        .await
        .expect("DescribeBroker");

    for field in [
        "brokerId",
        "brokerArn",
        "brokerName",
        "brokerState",
        "engineType",
        "hostInstanceType",
        "publiclyAccessible",
        "users",
    ] {
        assert!(
            out.get(field).is_some(),
            "DescribeBroker should carry `{field}`: {out}"
        );
    }
    assert!(
        out.get("BrokerId").is_none(),
        "PascalCase must not leak onto the wire: {out}"
    );
    assert_eq!(out["users"][0]["username"], "admin");
}

#[tokio::test]
async fn list_brokers_answers_under_broker_summaries() {
    let svc = MqService::new();
    broker(&svc, "wire-list").await;

    let out = svc
        .handle("ListBrokers", json!({}), &ctx())
        .await
        .expect("ListBrokers");
    let rows = out["brokerSummaries"]
        .as_array()
        .unwrap_or_else(|| panic!("ListBrokers should answer brokerSummaries: {out}"));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["brokerName"], "wire-list");
}

/// `logs` on DescribeBroker is the derived summary, which is what makes
/// the log group names visible to a caller.
#[tokio::test]
async fn logs_is_the_derived_summary() {
    let svc = MqService::new();
    let mut input = create_input("wire-logs");
    input["engineType"] = json!("ACTIVEMQ");
    input["engineVersion"] = json!("5.18");
    input["logs"] = json!({ "general": true, "audit": true });
    let created = svc
        .handle("CreateBroker", input, &ctx())
        .await
        .expect("CreateBroker");
    let id = created["brokerId"].as_str().expect("brokerId").to_string();

    let out = svc
        .handle("DescribeBroker", json!({ "brokerId": id.clone() }), &ctx())
        .await
        .expect("DescribeBroker");
    assert_eq!(out["logs"]["audit"], json!(true));
    assert_eq!(
        out["logs"]["auditLogGroup"],
        json!(format!("/aws/amazonmq/{id}/audit"))
    );
    assert!(
        out.get("LogsSummary").is_none(),
        "the summary is the `logs` member itself: {out}"
    );
}
