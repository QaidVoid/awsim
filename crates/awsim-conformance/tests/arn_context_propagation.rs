//! ARN region/account propagation conformance.
//!
//! Every regional service must reflect the request's `(account_id,
//! region)` into the ARNs it returns. A regression where a service
//! hard-codes `us-east-1` or `000000000000` would surface as a
//! cross-region or cross-account dispatch bug under multi-tenant
//! workloads, so this suite pins the contract: hit each service with
//! `(account=123456789012, region=eu-west-1)` and verify every ARN in
//! the response carries those segments.
//!
//! Global services (IAM, Route 53, STS at the global endpoint,
//! Organizations) are intentionally not covered here — they omit the
//! region segment by design, and AWS treats them as per-account-only.

use awsim_core::{RequestContext, ServiceHandler};
use serde_json::{Value, json};

const ACCT: &str = "123456789012";
const REGION: &str = "eu-west-1";

fn ctx(service: &str) -> RequestContext {
    RequestContext::new_with_account(service, REGION, ACCT)
}

/// Recursively walk a JSON value and collect every string that looks
/// like an ARN (starts with `arn:`).
fn collect_arns(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) if s.starts_with("arn:") => out.push(s.clone()),
        Value::Array(arr) => {
            for v in arr {
                collect_arns(v, out);
            }
        }
        Value::Object(map) => {
            for v in map.values() {
                collect_arns(v, out);
            }
        }
        _ => {}
    }
}

/// Assert that every collected ARN with a non-empty region segment
/// uses the expected region and account. ARN format:
/// `arn:partition:service:region:account:resource`.
fn assert_arns_pinned(value: &Value, label: &str) {
    let mut arns = Vec::new();
    collect_arns(value, &mut arns);
    assert!(
        !arns.is_empty(),
        "{label} returned no ARNs to validate (was the response empty?): {value}"
    );
    for arn in &arns {
        let parts: Vec<&str> = arn.splitn(6, ':').collect();
        assert!(
            parts.len() == 6,
            "{label} ARN `{arn}` is malformed: not enough segments"
        );
        let region = parts[3];
        let account = parts[4];
        // Some service ARNs intentionally omit either segment (S3
        // bucket ARNs leave both empty; IAM ARNs leave region empty).
        // Only verify segments that are present.
        if !region.is_empty() {
            assert_eq!(
                region, REGION,
                "{label}: ARN `{arn}` carries wrong region (expected {REGION})"
            );
        }
        if !account.is_empty() {
            assert_eq!(
                account, ACCT,
                "{label}: ARN `{arn}` carries wrong account (expected {ACCT})"
            );
        }
    }
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

#[tokio::test]
async fn sqs_create_queue_arn_uses_context() {
    let svc = awsim_sqs::SqsService::new();
    let out = svc
        .handle(
            "CreateQueue",
            json!({"QueueName": "tenant-q"}),
            &ctx(svc.service_name()),
        )
        .await
        .unwrap();
    // SQS returns a QueueUrl, not an ARN. Verify the URL embeds the
    // account, then fetch the ARN attribute and check that too.
    let queue_url = out["QueueUrl"].as_str().unwrap();
    assert!(queue_url.contains(ACCT), "QueueUrl: {queue_url}");

    let attrs = svc
        .handle(
            "GetQueueAttributes",
            json!({"QueueUrl": queue_url, "AttributeNames": ["QueueArn"]}),
            &ctx(svc.service_name()),
        )
        .await
        .unwrap();
    assert_arns_pinned(&attrs, "SQS GetQueueAttributes");
}

#[tokio::test]
async fn sns_create_topic_arn_uses_context() {
    let svc = awsim_sns::SnsService::new();
    let out = svc
        .handle(
            "CreateTopic",
            json!({"Name": "tenant-topic"}),
            &ctx(svc.service_name()),
        )
        .await
        .unwrap();
    assert_arns_pinned(&out, "SNS CreateTopic");
}

#[tokio::test]
async fn dynamodb_create_table_arn_uses_context() {
    let svc = awsim_dynamodb::DynamoDbService::new();
    let out = svc
        .handle(
            "CreateTable",
            json!({
                "TableName": "t",
                "AttributeDefinitions": [{"AttributeName": "PK", "AttributeType": "S"}],
                "KeySchema": [{"AttributeName": "PK", "KeyType": "HASH"}],
                "BillingMode": "PAY_PER_REQUEST",
            }),
            &ctx(svc.service_name()),
        )
        .await
        .unwrap();
    assert_arns_pinned(&out, "DynamoDB CreateTable");
}

#[tokio::test]
async fn secretsmanager_create_secret_arn_uses_context() {
    let svc = awsim_secretsmanager::SecretsManagerService::new();
    let out = svc
        .handle(
            "CreateSecret",
            json!({"Name": "tenant-secret", "SecretString": "shh"}),
            &ctx(svc.service_name()),
        )
        .await
        .unwrap();
    assert_arns_pinned(&out, "SecretsManager CreateSecret");
}

#[tokio::test]
async fn kms_create_key_arn_uses_context() {
    let svc = awsim_kms::KmsService::new();
    let out = svc
        .handle("CreateKey", json!({}), &ctx(svc.service_name()))
        .await
        .unwrap();
    assert_arns_pinned(&out, "KMS CreateKey");
}

#[tokio::test]
async fn eventbridge_default_bus_arn_uses_context() {
    let svc = awsim_eventbridge::EventBridgeService::new();
    let out = svc
        .handle("DescribeEventBus", json!({}), &ctx(svc.service_name()))
        .await
        .unwrap();
    assert_arns_pinned(&out, "EventBridge DescribeEventBus");
}

#[tokio::test]
async fn kinesis_create_stream_arn_uses_context() {
    let svc = awsim_kinesis::KinesisService::new();
    svc.handle(
        "CreateStream",
        json!({"StreamName": "tenant-stream", "ShardCount": 1}),
        &ctx(svc.service_name()),
    )
    .await
    .unwrap();
    let out = svc
        .handle(
            "DescribeStream",
            json!({"StreamName": "tenant-stream"}),
            &ctx(svc.service_name()),
        )
        .await
        .unwrap();
    assert_arns_pinned(&out, "Kinesis DescribeStream");
}

#[tokio::test]
async fn eks_create_cluster_arn_uses_context() {
    let svc = awsim_eks::EksService::new();
    let out = svc
        .handle(
            "CreateCluster",
            json!({
                "name": "tenant-cluster",
                "roleArn": format!("arn:aws:iam::{ACCT}:role/eks"),
            }),
            &ctx(svc.service_name()),
        )
        .await
        .unwrap();
    assert_arns_pinned(&out, "EKS CreateCluster");
}

#[tokio::test]
async fn cloudwatch_logs_create_log_group_arn_uses_context() {
    let svc = awsim_cloudwatch_logs::CloudWatchLogsService::new();
    svc.handle(
        "CreateLogGroup",
        json!({"logGroupName": "tenant-lg"}),
        &ctx(svc.service_name()),
    )
    .await
    .unwrap();
    let out = svc
        .handle(
            "DescribeLogGroups",
            json!({"logGroupNamePrefix": "tenant-lg"}),
            &ctx(svc.service_name()),
        )
        .await
        .unwrap();
    assert_arns_pinned(&out, "CloudWatch Logs DescribeLogGroups");
}

#[tokio::test]
async fn ssm_put_parameter_arn_uses_context() {
    let svc = awsim_ssm::SsmService::new();
    svc.handle(
        "PutParameter",
        json!({"Name": "tenant-param", "Value": "v", "Type": "String"}),
        &ctx(svc.service_name()),
    )
    .await
    .unwrap();
    let out = svc
        .handle(
            "GetParameter",
            json!({"Name": "tenant-param"}),
            &ctx(svc.service_name()),
        )
        .await
        .unwrap();
    assert_arns_pinned(&out, "SSM GetParameter");
}

const PARTITION: &str = "aws-cn";

/// Like [`ctx`] but with a non-default partition, to prove that a
/// configured `AWSIM_PARTITION` propagates into emitted ARNs.
fn ctx_part(service: &str, partition: &str) -> RequestContext {
    let mut c = RequestContext::new_with_account(service, REGION, ACCT);
    c.partition = partition.to_string();
    c
}

/// Assert every collected ARN carries the expected partition segment
/// (`arn:<partition>:...`).
fn assert_arns_partition(value: &Value, label: &str, partition: &str) {
    let mut arns = Vec::new();
    collect_arns(value, &mut arns);
    assert!(
        !arns.is_empty(),
        "{label} returned no ARNs to validate: {value}"
    );
    for arn in &arns {
        let parts: Vec<&str> = arn.splitn(6, ':').collect();
        assert!(
            parts.len() == 6,
            "{label} ARN `{arn}` is malformed: not enough segments"
        );
        assert_eq!(
            parts[1], partition,
            "{label}: ARN `{arn}` carries wrong partition (expected {partition})"
        );
    }
}

/// A non-default `AWSIM_PARTITION` (China `aws-cn` here) must surface in
/// every ARN a service emits, across the construction sites swept onto
/// the `arn::build*` helpers.
#[tokio::test]
async fn arns_honor_non_default_partition() {
    let sns = awsim_sns::SnsService::new();
    let out = sns
        .handle(
            "CreateTopic",
            json!({"Name": "p-topic"}),
            &ctx_part(sns.service_name(), PARTITION),
        )
        .await
        .unwrap();
    assert_arns_partition(&out, "SNS CreateTopic", PARTITION);

    let ddb = awsim_dynamodb::DynamoDbService::new();
    let out = ddb
        .handle(
            "CreateTable",
            json!({
                "TableName": "t",
                "AttributeDefinitions": [{"AttributeName": "PK", "AttributeType": "S"}],
                "KeySchema": [{"AttributeName": "PK", "KeyType": "HASH"}],
                "BillingMode": "PAY_PER_REQUEST",
            }),
            &ctx_part(ddb.service_name(), PARTITION),
        )
        .await
        .unwrap();
    assert_arns_partition(&out, "DynamoDB CreateTable", PARTITION);

    let kms = awsim_kms::KmsService::new();
    let out = kms
        .handle(
            "CreateKey",
            json!({}),
            &ctx_part(kms.service_name(), PARTITION),
        )
        .await
        .unwrap();
    assert_arns_partition(&out, "KMS CreateKey", PARTITION);

    let sm = awsim_secretsmanager::SecretsManagerService::new();
    let out = sm
        .handle(
            "CreateSecret",
            json!({"Name": "p-secret", "SecretString": "x"}),
            &ctx_part(sm.service_name(), PARTITION),
        )
        .await
        .unwrap();
    assert_arns_partition(&out, "SecretsManager CreateSecret", PARTITION);

    let ssm = awsim_ssm::SsmService::new();
    ssm.handle(
        "PutParameter",
        json!({"Name": "p-param", "Value": "v", "Type": "String"}),
        &ctx_part(ssm.service_name(), PARTITION),
    )
    .await
    .unwrap();
    let out = ssm
        .handle(
            "GetParameter",
            json!({"Name": "p-param"}),
            &ctx_part(ssm.service_name(), PARTITION),
        )
        .await
        .unwrap();
    assert_arns_partition(&out, "SSM GetParameter", PARTITION);
}

#[test]
fn unused_block_on_silenced() {
    // Suppress dead-code lint on the sync executor when no helper
    // happens to use it. Each #[tokio::test] above runs its own
    // runtime, so block_on is purely a convenience for future tests.
    let _ = block_on(async { 1 });
}
