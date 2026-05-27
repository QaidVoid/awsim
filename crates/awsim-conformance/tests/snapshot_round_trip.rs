//! Per-service snapshot + restore round-trip conformance.
//!
//! Each test creates a resource through the public handler, snapshots
//! the service state, restores into a fresh instance, and asserts the
//! restored instance reports the same resource. Regressions where a
//! service forgets to wire a new collection into its serializable
//! snapshot show up here as a missing resource on the restored side.
//!
//! Add a new service entry whenever a service grows snapshot
//! coverage; the pattern is intentionally one test per service so a
//! failure points at the exact crate.

use awsim_core::{RequestContext, ServiceHandler};
use serde_json::json;

fn ctx(service: &str) -> RequestContext {
    RequestContext::new(service, "us-east-1")
}

async fn round_trip<S: ServiceHandler>(
    seed: &S,
    target: &S,
    seed_call: (&str, serde_json::Value),
    verify_call: (&str, serde_json::Value),
    expect_key: &str,
) {
    let c = ctx(seed.service_name());
    seed.handle(seed_call.0, seed_call.1, &c).await.unwrap();
    let bytes = seed.snapshot().expect("service must support snapshot");
    target.restore(&bytes).expect("restore must succeed");
    let resp = target
        .handle(verify_call.0, verify_call.1, &c)
        .await
        .unwrap();
    assert!(
        resp.get(expect_key).is_some(),
        "expected restored response to contain `{expect_key}`, got {resp}"
    );
}

#[tokio::test]
async fn sqs_round_trip_preserves_queues() {
    let seed = awsim_sqs::SqsService::new();
    let target = awsim_sqs::SqsService::new();
    round_trip(
        &seed,
        &target,
        ("CreateQueue", json!({"QueueName": "restored-q"})),
        ("GetQueueUrl", json!({"QueueName": "restored-q"})),
        "QueueUrl",
    )
    .await;
}

#[tokio::test]
async fn iam_round_trip_preserves_users() {
    use awsim_iam::IamService;
    let seed = IamService::new();
    let target = IamService::new();
    round_trip(
        &seed,
        &target,
        ("CreateUser", json!({"UserName": "alice"})),
        ("GetUser", json!({"UserName": "alice"})),
        "User",
    )
    .await;
}

#[tokio::test]
async fn dynamodb_round_trip_preserves_tables() {
    let seed = awsim_dynamodb::DynamoDbService::new();
    let target = awsim_dynamodb::DynamoDbService::new();
    round_trip(
        &seed,
        &target,
        (
            "CreateTable",
            json!({
                "TableName": "restored-table",
                "AttributeDefinitions": [{"AttributeName": "PK", "AttributeType": "S"}],
                "KeySchema": [{"AttributeName": "PK", "KeyType": "HASH"}],
                "BillingMode": "PAY_PER_REQUEST",
            }),
        ),
        ("DescribeTable", json!({"TableName": "restored-table"})),
        "Table",
    )
    .await;
}

#[tokio::test]
async fn sns_round_trip_preserves_topics() {
    let seed = awsim_sns::SnsService::new();
    let target = awsim_sns::SnsService::new();
    round_trip(
        &seed,
        &target,
        ("CreateTopic", json!({"Name": "restored-topic"})),
        (
            "GetTopicAttributes",
            json!({"TopicArn": "arn:aws:sns:us-east-1:000000000000:restored-topic"}),
        ),
        "Attributes",
    )
    .await;
}

#[tokio::test]
async fn cloudwatch_logs_round_trip_preserves_log_groups() {
    let seed = awsim_cloudwatch_logs::CloudWatchLogsService::new();
    let target = awsim_cloudwatch_logs::CloudWatchLogsService::new();
    let c = ctx(seed.service_name());
    seed.handle("CreateLogGroup", json!({"logGroupName": "restored-lg"}), &c)
        .await
        .unwrap();
    let bytes = seed.snapshot().expect("CloudWatch Logs supports snapshot");
    target.restore(&bytes).expect("restore must succeed");
    let resp = target
        .handle(
            "DescribeLogGroups",
            json!({"logGroupNamePrefix": "restored-lg"}),
            &c,
        )
        .await
        .unwrap();
    let groups = resp["logGroups"]
        .as_array()
        .expect("logGroups array present");
    assert!(
        groups
            .iter()
            .any(|g| g["logGroupName"].as_str() == Some("restored-lg")),
        "restored log group missing: {resp}"
    );
}

#[tokio::test]
async fn acm_round_trip_preserves_certificates() {
    let seed = awsim_acm::AcmService::new();
    let target = awsim_acm::AcmService::new();
    let c = ctx(seed.service_name());
    let issued = seed
        .handle(
            "RequestCertificate",
            json!({"DomainName": "example.com"}),
            &c,
        )
        .await
        .unwrap();
    let arn = issued["CertificateArn"].as_str().unwrap().to_string();
    let bytes = seed.snapshot().expect("ACM supports snapshot");
    target.restore(&bytes).expect("restore must succeed");
    let resp = target
        .handle(
            "DescribeCertificate",
            json!({"CertificateArn": arn.clone()}),
            &c,
        )
        .await
        .unwrap();
    assert_eq!(
        resp["Certificate"]["CertificateArn"].as_str(),
        Some(arn.as_str())
    );
}

#[tokio::test]
async fn lambda_round_trip_preserves_functions() {
    let seed = awsim_lambda::LambdaService::new();
    let target = awsim_lambda::LambdaService::new();
    let c = ctx(seed.service_name());
    seed.handle(
        "CreateFunction",
        json!({
            "FunctionName": "restored-fn",
            "Role": "arn:aws:iam::000000000000:role/lambda",
            "Runtime": "provided.al2",
            "Handler": "index.handler",
            "Code": {"ZipFile": "AAAA"},
        }),
        &c,
    )
    .await
    .unwrap();
    let bytes = seed.snapshot().expect("Lambda supports snapshot");
    target.restore(&bytes).expect("restore must succeed");
    let resp = target
        .handle("GetFunction", json!({"FunctionName": "restored-fn"}), &c)
        .await
        .unwrap();
    assert_eq!(
        resp["Configuration"]["FunctionName"].as_str(),
        Some("restored-fn")
    );
}
