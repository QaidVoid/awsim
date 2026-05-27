//! Per-service (account, region) isolation conformance.
//!
//! AWS partitions resources by `(account_id, region)`. Two tenants
//! sharing one AWSim instance must not see each other's resources, and
//! a request issued under a different region than the resource was
//! created in must not surface that resource either.
//!
//! Each test creates a resource in `(111111111111, us-east-1)`, then
//! tries to read the same name from
//! `(222222222222, us-east-1)` and `(111111111111, eu-west-1)`. Both
//! reads must miss; a hit means the service is leaking state across
//! its `AccountRegionStore` boundary.
//!
//! Services with intentionally global state (IAM, Route 53,
//! Organizations) are exempted: AWS treats them as per-account-only,
//! and AWSim mirrors that by ignoring `ctx.region` on those stores.

use awsim_core::{RequestContext, ServiceHandler};
use serde_json::json;

const ACCT_A: &str = "111111111111";
const ACCT_B: &str = "222222222222";

fn ctx(service: &str, account: &str, region: &str) -> RequestContext {
    RequestContext::new_with_account(service, region, account)
}

/// Run a service through the create / verify-other-account / verify-other-region
/// triad. Asserts the resource is visible only from the original
/// `(ACCT_A, us-east-1)` slot.
async fn assert_isolated<S: ServiceHandler>(
    svc: &S,
    create_op: &str,
    create_input: serde_json::Value,
    describe_op: &str,
    describe_input: serde_json::Value,
) {
    svc.handle(
        create_op,
        create_input,
        &ctx(svc.service_name(), ACCT_A, "us-east-1"),
    )
    .await
    .expect("create in seed account must succeed");

    // Same name, different account: must NOT find it.
    let cross_account = svc
        .handle(
            describe_op,
            describe_input.clone(),
            &ctx(svc.service_name(), ACCT_B, "us-east-1"),
        )
        .await;
    assert!(
        cross_account.is_err(),
        "cross-account read leaked the resource: {cross_account:?}"
    );

    // Same name, different region: must NOT find it.
    let cross_region = svc
        .handle(
            describe_op,
            describe_input,
            &ctx(svc.service_name(), ACCT_A, "eu-west-1"),
        )
        .await;
    assert!(
        cross_region.is_err(),
        "cross-region read leaked the resource: {cross_region:?}"
    );
}

#[tokio::test]
async fn sqs_is_account_region_scoped() {
    let svc = awsim_sqs::SqsService::new();
    assert_isolated(
        &svc,
        "CreateQueue",
        json!({"QueueName": "tenant-queue"}),
        "GetQueueUrl",
        json!({"QueueName": "tenant-queue"}),
    )
    .await;
}

#[tokio::test]
async fn dynamodb_is_account_region_scoped() {
    let svc = awsim_dynamodb::DynamoDbService::new();
    assert_isolated(
        &svc,
        "CreateTable",
        json!({
            "TableName": "tenant-table",
            "AttributeDefinitions": [{"AttributeName": "PK", "AttributeType": "S"}],
            "KeySchema": [{"AttributeName": "PK", "KeyType": "HASH"}],
            "BillingMode": "PAY_PER_REQUEST",
        }),
        "DescribeTable",
        json!({"TableName": "tenant-table"}),
    )
    .await;
}

#[tokio::test]
async fn secretsmanager_is_account_region_scoped() {
    let svc = awsim_secretsmanager::SecretsManagerService::new();
    assert_isolated(
        &svc,
        "CreateSecret",
        json!({"Name": "tenant-secret", "SecretString": "shh"}),
        "DescribeSecret",
        json!({"SecretId": "tenant-secret"}),
    )
    .await;
}

#[tokio::test]
async fn sns_is_account_region_scoped() {
    let svc = awsim_sns::SnsService::new();
    // SNS GetTopicAttributes is keyed by ARN. The same topic name in
    // two accounts/regions produces different ARNs, so a hit in the
    // wrong slot would only happen if the underlying store was
    // shared. Cross both axes use the seed account's ARN to keep the
    // describe step independent of the local ctx.
    svc.handle(
        "CreateTopic",
        json!({"Name": "tenant-topic"}),
        &ctx(svc.service_name(), ACCT_A, "us-east-1"),
    )
    .await
    .unwrap();
    let seed_arn = format!("arn:aws:sns:us-east-1:{ACCT_A}:tenant-topic");
    let cross_account = svc
        .handle(
            "GetTopicAttributes",
            json!({"TopicArn": seed_arn}),
            &ctx(svc.service_name(), ACCT_B, "us-east-1"),
        )
        .await;
    assert!(
        cross_account.is_err(),
        "SNS leaked topic across accounts: {cross_account:?}"
    );
    let cross_region = svc
        .handle(
            "GetTopicAttributes",
            json!({"TopicArn": seed_arn}),
            &ctx(svc.service_name(), ACCT_A, "eu-west-1"),
        )
        .await;
    assert!(
        cross_region.is_err(),
        "SNS leaked topic across regions: {cross_region:?}"
    );
}

#[tokio::test]
async fn kms_is_account_region_scoped() {
    let svc = awsim_kms::KmsService::new();
    // CreateKey returns a synthetic KeyId; describing it from a
    // different (account, region) must miss.
    let created = svc
        .handle(
            "CreateKey",
            json!({}),
            &ctx(svc.service_name(), ACCT_A, "us-east-1"),
        )
        .await
        .unwrap();
    let key_id = created["KeyMetadata"]["KeyId"]
        .as_str()
        .unwrap()
        .to_string();
    let cross_account = svc
        .handle(
            "DescribeKey",
            json!({"KeyId": key_id.clone()}),
            &ctx(svc.service_name(), ACCT_B, "us-east-1"),
        )
        .await;
    assert!(
        cross_account.is_err(),
        "KMS leaked key across accounts: {cross_account:?}"
    );
    let cross_region = svc
        .handle(
            "DescribeKey",
            json!({"KeyId": key_id}),
            &ctx(svc.service_name(), ACCT_A, "eu-west-1"),
        )
        .await;
    assert!(
        cross_region.is_err(),
        "KMS leaked key across regions: {cross_region:?}"
    );
}

#[tokio::test]
async fn eventbridge_is_account_region_scoped() {
    let svc = awsim_eventbridge::EventBridgeService::new();
    assert_isolated(
        &svc,
        "CreateEventBus",
        json!({"Name": "tenant-bus"}),
        "DescribeEventBus",
        json!({"Name": "tenant-bus"}),
    )
    .await;
}

#[tokio::test]
async fn kinesis_is_account_region_scoped() {
    let svc = awsim_kinesis::KinesisService::new();
    assert_isolated(
        &svc,
        "CreateStream",
        json!({"StreamName": "tenant-stream", "ShardCount": 1}),
        "DescribeStream",
        json!({"StreamName": "tenant-stream"}),
    )
    .await;
}

#[tokio::test]
async fn cloudwatch_logs_is_account_region_scoped() {
    let svc = awsim_cloudwatch_logs::CloudWatchLogsService::new();
    // CWLogs CreateLogGroup is success-only; lookups happen via
    // DescribeLogGroups with a prefix filter. The cross-account
    // / cross-region calls must return an empty list — a non-empty
    // hit means the store was shared.
    svc.handle(
        "CreateLogGroup",
        json!({"logGroupName": "tenant-lg"}),
        &ctx(svc.service_name(), ACCT_A, "us-east-1"),
    )
    .await
    .unwrap();
    let cross_account = svc
        .handle(
            "DescribeLogGroups",
            json!({"logGroupNamePrefix": "tenant-lg"}),
            &ctx(svc.service_name(), ACCT_B, "us-east-1"),
        )
        .await
        .unwrap();
    assert!(
        cross_account["logGroups"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(true),
        "CW Logs leaked group across accounts: {cross_account}"
    );
    let cross_region = svc
        .handle(
            "DescribeLogGroups",
            json!({"logGroupNamePrefix": "tenant-lg"}),
            &ctx(svc.service_name(), ACCT_A, "eu-west-1"),
        )
        .await
        .unwrap();
    assert!(
        cross_region["logGroups"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(true),
        "CW Logs leaked group across regions: {cross_region}"
    );
}

#[tokio::test]
async fn eks_is_account_region_scoped() {
    let svc = awsim_eks::EksService::new();
    assert_isolated(
        &svc,
        "CreateCluster",
        json!({
            "name": "tenant-cluster",
            "roleArn": "arn:aws:iam::111111111111:role/eks",
        }),
        "DescribeCluster",
        json!({"name": "tenant-cluster"}),
    )
    .await;
}

#[tokio::test]
async fn ssm_is_account_region_scoped() {
    let svc = awsim_ssm::SsmService::new();
    assert_isolated(
        &svc,
        "PutParameter",
        json!({"Name": "tenant-param", "Value": "v", "Type": "String"}),
        "GetParameter",
        json!({"Name": "tenant-param"}),
    )
    .await;
}
