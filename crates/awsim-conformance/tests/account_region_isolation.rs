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
