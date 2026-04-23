use std::collections::HashSet;

use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_dynamodb::types::{
    AttributeDefinition, AttributeValue, BillingMode, KeySchemaElement, KeyType,
    ScalarAttributeType,
};
use aws_sdk_lambda::primitives::Blob;
use aws_sdk_lambda::types::{FunctionCode, Runtime};
use aws_sdk_ssm::types::ParameterType;
use aws_types::region::Region;

use crate::smithy::SmithyModel;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum OpResult {
    Pass(String),
    Fail(String, String),
    NotImplemented(String),
    Skipped(String),
}

impl OpResult {
    pub fn op_name(&self) -> &str {
        match self {
            OpResult::Pass(n) | OpResult::Fail(n, _) | OpResult::NotImplemented(n) | OpResult::Skipped(n) => n,
        }
    }

    pub fn is_pass(&self) -> bool {
        matches!(self, OpResult::Pass(_))
    }

    pub fn is_fail(&self) -> bool {
        matches!(self, OpResult::Fail(_, _))
    }

    #[allow(dead_code)]
    pub fn is_not_implemented(&self) -> bool {
        matches!(self, OpResult::NotImplemented(_))
    }
}

pub struct ServiceResult {
    pub service: String,
    pub total: usize,
    pub implemented: usize,
    pub passed: usize,
    pub failed: usize,
    pub results: Vec<OpResult>,
}

// ---------------------------------------------------------------------------
// Top-level dispatcher
// ---------------------------------------------------------------------------

/// Run conformance tests for a given service based on the model file name.
pub async fn test_service(
    endpoint: &str,
    service_name: &str,
    model: &SmithyModel,
    verbose: bool,
) -> ServiceResult {
    let op_results = match service_name {
        "sts" => test_sts(endpoint, verbose).await,
        "dynamodb" => test_dynamodb(endpoint, verbose).await,
        "s3" => test_s3(endpoint, verbose).await,
        "sqs" => test_sqs(endpoint, verbose).await,
        "sns" => test_sns(endpoint, verbose).await,
        "iam" => test_iam(endpoint, verbose).await,
        "kms" => test_kms(endpoint, verbose).await,
        "secretsmanager" => test_secretsmanager(endpoint, verbose).await,
        "ssm" => test_ssm(endpoint, verbose).await,
        "lambda" => test_lambda(endpoint, verbose).await,
        "kinesis" => test_kinesis(endpoint, verbose).await,
        "cognito-idp" => test_cognito_idp(endpoint, verbose).await,
        "cognito-identity" => test_cognito_identity(endpoint, verbose).await,
        "ecs" => test_ecs(endpoint, verbose).await,
        "ecr" => test_ecr(endpoint, verbose).await,
        _ => {
            // Unknown service — report nothing tested.
            return ServiceResult {
                service: service_name.to_string(),
                total: model.operations().len(),
                implemented: 0,
                passed: 0,
                failed: 0,
                results: Vec::new(),
            };
        }
    };

    let smithy_ops: HashSet<String> = model.operation_names();
    let tested_ops: HashSet<String> = op_results.iter().map(|r| r.op_name().to_string()).collect();

    // Operations that appear in the Smithy model are "known".
    let total = smithy_ops.len();

    // implemented = any operation we called that's in the Smithy model
    let implemented = tested_ops.intersection(&smithy_ops).count();

    let passed = op_results.iter().filter(|r| r.is_pass()).count();
    let failed = op_results.iter().filter(|r| r.is_fail()).count();

    if verbose {
        // Print untested Smithy operations.
        let mut missing: Vec<&String> = smithy_ops
            .iter()
            .filter(|op| !tested_ops.contains(*op))
            .collect();
        missing.sort();
        if !missing.is_empty() {
            println!(
                "  [{}] untested Smithy operations: {}",
                service_name,
                missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            );
        }
    }

    ServiceResult {
        service: service_name.to_string(),
        total,
        implemented,
        passed,
        failed,
        results: op_results,
    }
}

// ---------------------------------------------------------------------------
// AWS config helper
// ---------------------------------------------------------------------------

async fn make_config(endpoint: &str) -> aws_config::SdkConfig {
    let creds = Credentials::new("test", "test", None, None, "conformance");
    aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(creds)
        .endpoint_url(endpoint)
        .load()
        .await
}

/// Generic categoriser that works on any Result<(), String> where the error
/// string already contains the debug representation.
fn categorise(op: &str, result: Result<(), String>, verbose: bool) -> OpResult {
    match result {
        Ok(_) => {
            if verbose {
                println!("  PASS {op}");
            }
            OpResult::Pass(op.to_string())
        }
        Err(e) => {
            if e.contains("NotImplemented") || e.contains("UnknownOperationException") {
                if verbose {
                    println!("  SKIP {op}: not implemented");
                }
                OpResult::NotImplemented(op.to_string())
            } else if is_deserialization_error(&e) {
                if verbose {
                    println!("  FAIL {op}: {e}");
                }
                OpResult::Fail(op.to_string(), e)
            } else {
                // Service-level error (ResourceNotFound, etc.) — shape is correct.
                if verbose {
                    println!("  PASS {op} (service error: {})", truncate(&e, 120));
                }
                OpResult::Pass(op.to_string())
            }
        }
    }
}

fn is_deserialization_error(err: &str) -> bool {
    err.contains("ResponseDeserializationError")
        || err.contains("Unhandled(Unhandled { source: Error")
        || (err.contains("failed to deserialize") && !err.contains("ServiceError"))
        || err.contains("InvalidXml")
        || err.contains("DecodeError")
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

// ---------------------------------------------------------------------------
// Helper: convert an SDK error to a String for our generic categoriser
// ---------------------------------------------------------------------------

fn sdk_err_to_string<E: std::fmt::Debug>(e: E) -> String {
    format!("{e:?}")
}

macro_rules! chk {
    ($op:expr, $result:expr, $verbose:expr) => {
        categorise(
            $op,
            $result.map(|_| ()).map_err(|e| sdk_err_to_string(e)),
            $verbose,
        )
    };
}

// ---------------------------------------------------------------------------
// STS
// ---------------------------------------------------------------------------

async fn test_sts(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_sts::Client::new(&config);
    let mut results = Vec::new();

    results.push(chk!(
        "GetCallerIdentity",
        client.get_caller_identity().send().await,
        verbose
    ));

    results.push(chk!(
        "AssumeRole",
        client
            .assume_role()
            .role_arn("arn:aws:iam::000000000000:role/ConformanceRole")
            .role_session_name("conformance-session")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "GetSessionToken",
        client.get_session_token().send().await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// DynamoDB
// ---------------------------------------------------------------------------

async fn test_dynamodb(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_dynamodb::Client::new(&config);
    let mut results = Vec::new();

    // CreateTable
    let r = client
        .create_table()
        .table_name("conformance-test")
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("id")
                .key_type(KeyType::Hash)
                .build()
                .unwrap(),
        )
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("id")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .unwrap(),
        )
        .billing_mode(BillingMode::PayPerRequest)
        .send()
        .await;
    results.push(chk!("CreateTable", r, verbose));

    // ListTables
    results.push(chk!(
        "ListTables",
        client.list_tables().send().await,
        verbose
    ));

    // DescribeTable
    results.push(chk!(
        "DescribeTable",
        client
            .describe_table()
            .table_name("conformance-test")
            .send()
            .await,
        verbose
    ));

    // PutItem
    results.push(chk!(
        "PutItem",
        client
            .put_item()
            .table_name("conformance-test")
            .item("id", AttributeValue::S("test-1".into()))
            .item("name", AttributeValue::S("Test Item".into()))
            .send()
            .await,
        verbose
    ));

    // GetItem
    results.push(chk!(
        "GetItem",
        client
            .get_item()
            .table_name("conformance-test")
            .key("id", AttributeValue::S("test-1".into()))
            .send()
            .await,
        verbose
    ));

    // UpdateItem
    results.push(chk!(
        "UpdateItem",
        client
            .update_item()
            .table_name("conformance-test")
            .key("id", AttributeValue::S("test-1".into()))
            .update_expression("SET #n = :v")
            .expression_attribute_names("#n", "name")
            .expression_attribute_values(":v", AttributeValue::S("Updated".into()))
            .send()
            .await,
        verbose
    ));

    // Query
    results.push(chk!(
        "Query",
        client
            .query()
            .table_name("conformance-test")
            .key_condition_expression("#id = :id")
            .expression_attribute_names("#id", "id")
            .expression_attribute_values(":id", AttributeValue::S("test-1".into()))
            .send()
            .await,
        verbose
    ));

    // Scan
    results.push(chk!(
        "Scan",
        client.scan().table_name("conformance-test").send().await,
        verbose
    ));

    // BatchWriteItem
    results.push(chk!(
        "BatchWriteItem",
        client
            .batch_write_item()
            .request_items(
                "conformance-test",
                vec![aws_sdk_dynamodb::types::WriteRequest::builder()
                    .put_request(
                        aws_sdk_dynamodb::types::PutRequest::builder()
                            .item("id", AttributeValue::S("batch-1".into()))
                            .build()
                            .unwrap(),
                    )
                    .build()],
            )
            .send()
            .await,
        verbose
    ));

    // BatchGetItem
    results.push(chk!(
        "BatchGetItem",
        client
            .batch_get_item()
            .request_items(
                "conformance-test",
                aws_sdk_dynamodb::types::KeysAndAttributes::builder()
                    .keys(
                        std::collections::HashMap::from([(
                            "id".to_string(),
                            AttributeValue::S("batch-1".into()),
                        )])
                    )
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // TransactWriteItems
    results.push(chk!(
        "TransactWriteItems",
        client
            .transact_write_items()
            .transact_items(
                aws_sdk_dynamodb::types::TransactWriteItem::builder()
                    .put(
                        aws_sdk_dynamodb::types::Put::builder()
                            .table_name("conformance-test")
                            .item("id", AttributeValue::S("txn-1".into()))
                            .build()
                            .unwrap(),
                    )
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // TransactGetItems
    results.push(chk!(
        "TransactGetItems",
        client
            .transact_get_items()
            .transact_items(
                aws_sdk_dynamodb::types::TransactGetItem::builder()
                    .get(
                        aws_sdk_dynamodb::types::Get::builder()
                            .table_name("conformance-test")
                            .key("id", AttributeValue::S("txn-1".into()))
                            .build()
                            .unwrap(),
                    )
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // DeleteItem
    results.push(chk!(
        "DeleteItem",
        client
            .delete_item()
            .table_name("conformance-test")
            .key("id", AttributeValue::S("test-1".into()))
            .send()
            .await,
        verbose
    ));

    // DeleteTable (cleanup)
    results.push(chk!(
        "DeleteTable",
        client
            .delete_table()
            .table_name("conformance-test")
            .send()
            .await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// S3
// ---------------------------------------------------------------------------

async fn test_s3(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_s3::Client::new(&config);
    let mut results = Vec::new();

    // CreateBucket
    results.push(chk!(
        "CreateBucket",
        client
            .create_bucket()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // HeadBucket
    results.push(chk!(
        "HeadBucket",
        client
            .head_bucket()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // ListBuckets
    results.push(chk!(
        "ListBuckets",
        client.list_buckets().send().await,
        verbose
    ));

    // PutObject
    results.push(chk!(
        "PutObject",
        client
            .put_object()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .body(aws_sdk_s3::primitives::ByteStream::from_static(b"hello conformance"))
            .send()
            .await,
        verbose
    ));

    // HeadObject
    results.push(chk!(
        "HeadObject",
        client
            .head_object()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .send()
            .await,
        verbose
    ));

    // GetObject
    results.push(chk!(
        "GetObject",
        client
            .get_object()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .send()
            .await,
        verbose
    ));

    // ListObjectsV2
    results.push(chk!(
        "ListObjectsV2",
        client
            .list_objects_v2()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // CopyObject
    results.push(chk!(
        "CopyObject",
        client
            .copy_object()
            .bucket("conformance-bucket")
            .key("test-copy.txt")
            .copy_source("conformance-bucket/test-object.txt")
            .send()
            .await,
        verbose
    ));

    // GetBucketLocation
    results.push(chk!(
        "GetBucketLocation",
        client
            .get_bucket_location()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketVersioning
    results.push(chk!(
        "PutBucketVersioning",
        client
            .put_bucket_versioning()
            .bucket("conformance-bucket")
            .versioning_configuration(
                aws_sdk_s3::types::VersioningConfiguration::builder()
                    .status(aws_sdk_s3::types::BucketVersioningStatus::Enabled)
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketVersioning
    results.push(chk!(
        "GetBucketVersioning",
        client
            .get_bucket_versioning()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteObject
    results.push(chk!(
        "DeleteObject",
        client
            .delete_object()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .send()
            .await,
        verbose
    ));

    // DeleteObjects
    results.push(chk!(
        "DeleteObjects",
        client
            .delete_objects()
            .bucket("conformance-bucket")
            .delete(
                aws_sdk_s3::types::Delete::builder()
                    .objects(
                        aws_sdk_s3::types::ObjectIdentifier::builder()
                            .key("test-copy.txt")
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // DeleteBucket
    results.push(chk!(
        "DeleteBucket",
        client
            .delete_bucket()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// SQS
// ---------------------------------------------------------------------------

async fn test_sqs(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_sqs::Client::new(&config);
    let mut results = Vec::new();

    // CreateQueue
    let create_r = client
        .create_queue()
        .queue_name("conformance-queue")
        .send()
        .await;
    let queue_url = create_r
        .as_ref()
        .ok()
        .and_then(|r| r.queue_url.clone())
        .unwrap_or_else(|| {
            format!(
                "{}/000000000000/conformance-queue",
                endpoint.replace("http://", "http://sqs.us-east-1.")
            )
        });
    results.push(chk!("CreateQueue", create_r, verbose));

    // ListQueues
    results.push(chk!(
        "ListQueues",
        client.list_queues().send().await,
        verbose
    ));

    // GetQueueUrl
    results.push(chk!(
        "GetQueueUrl",
        client
            .get_queue_url()
            .queue_name("conformance-queue")
            .send()
            .await,
        verbose
    ));

    // GetQueueAttributes
    results.push(chk!(
        "GetQueueAttributes",
        client
            .get_queue_attributes()
            .queue_url(&queue_url)
            .send()
            .await,
        verbose
    ));

    // SendMessage
    let send_r = client
        .send_message()
        .queue_url(&queue_url)
        .message_body("conformance test message")
        .send()
        .await;
    results.push(chk!("SendMessage", send_r, verbose));

    // ReceiveMessage
    let recv_r = client
        .receive_message()
        .queue_url(&queue_url)
        .max_number_of_messages(1)
        .send()
        .await;
    let receipt_handle = recv_r
        .as_ref()
        .ok()
        .and_then(|r| r.messages.as_ref())
        .and_then(|m| m.first())
        .and_then(|m| m.receipt_handle.clone());
    results.push(chk!("ReceiveMessage", recv_r, verbose));

    // SendMessageBatch
    results.push(chk!(
        "SendMessageBatch",
        client
            .send_message_batch()
            .queue_url(&queue_url)
            .entries(
                aws_sdk_sqs::types::SendMessageBatchRequestEntry::builder()
                    .id("msg-1")
                    .message_body("batch message 1")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // DeleteMessage
    if let Some(handle) = receipt_handle {
        results.push(chk!(
            "DeleteMessage",
            client
                .delete_message()
                .queue_url(&queue_url)
                .receipt_handle(handle)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteMessage".to_string()));
    }

    // PurgeQueue
    results.push(chk!(
        "PurgeQueue",
        client.purge_queue().queue_url(&queue_url).send().await,
        verbose
    ));

    // SetQueueAttributes
    results.push(chk!(
        "SetQueueAttributes",
        client
            .set_queue_attributes()
            .queue_url(&queue_url)
            .attributes(
                aws_sdk_sqs::types::QueueAttributeName::MessageRetentionPeriod,
                "86400",
            )
            .send()
            .await,
        verbose
    ));

    // DeleteQueue
    results.push(chk!(
        "DeleteQueue",
        client.delete_queue().queue_url(&queue_url).send().await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// SNS
// ---------------------------------------------------------------------------

async fn test_sns(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_sns::Client::new(&config);
    let mut results = Vec::new();

    // CreateTopic
    let create_r = client
        .create_topic()
        .name("conformance-topic")
        .send()
        .await;
    let topic_arn = create_r
        .as_ref()
        .ok()
        .and_then(|r| r.topic_arn.clone())
        .unwrap_or_else(|| {
            "arn:aws:sns:us-east-1:000000000000:conformance-topic".to_string()
        });
    results.push(chk!("CreateTopic", create_r, verbose));

    // ListTopics
    results.push(chk!(
        "ListTopics",
        client.list_topics().send().await,
        verbose
    ));

    // GetTopicAttributes
    results.push(chk!(
        "GetTopicAttributes",
        client
            .get_topic_attributes()
            .topic_arn(&topic_arn)
            .send()
            .await,
        verbose
    ));

    // Publish
    results.push(chk!(
        "Publish",
        client
            .publish()
            .topic_arn(&topic_arn)
            .message("conformance test message")
            .send()
            .await,
        verbose
    ));

    // Subscribe (email — no confirmation needed in sim)
    let sub_r = client
        .subscribe()
        .topic_arn(&topic_arn)
        .protocol("email")
        .endpoint("test@example.com")
        .send()
        .await;
    let subscription_arn = sub_r
        .as_ref()
        .ok()
        .and_then(|r| r.subscription_arn.clone());
    results.push(chk!("Subscribe", sub_r, verbose));

    // ListSubscriptions
    results.push(chk!(
        "ListSubscriptions",
        client.list_subscriptions().send().await,
        verbose
    ));

    // ListSubscriptionsByTopic
    results.push(chk!(
        "ListSubscriptionsByTopic",
        client
            .list_subscriptions_by_topic()
            .topic_arn(&topic_arn)
            .send()
            .await,
        verbose
    ));

    // Unsubscribe (if we got a subscription ARN)
    if let Some(sub_arn) = subscription_arn {
        results.push(chk!(
            "Unsubscribe",
            client.unsubscribe().subscription_arn(sub_arn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("Unsubscribe".to_string()));
    }

    // DeleteTopic
    results.push(chk!(
        "DeleteTopic",
        client.delete_topic().topic_arn(&topic_arn).send().await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// IAM
// ---------------------------------------------------------------------------

async fn test_iam(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_iam::Client::new(&config);
    let mut results = Vec::new();

    // CreateUser
    results.push(chk!(
        "CreateUser",
        client
            .create_user()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // GetUser
    results.push(chk!(
        "GetUser",
        client
            .get_user()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // ListUsers
    results.push(chk!(
        "ListUsers",
        client.list_users().send().await,
        verbose
    ));

    // CreateAccessKey
    results.push(chk!(
        "CreateAccessKey",
        client
            .create_access_key()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // ListAccessKeys
    results.push(chk!(
        "ListAccessKeys",
        client
            .list_access_keys()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // CreateGroup
    results.push(chk!(
        "CreateGroup",
        client
            .create_group()
            .group_name("conformance-group")
            .send()
            .await,
        verbose
    ));

    // ListGroups
    results.push(chk!(
        "ListGroups",
        client.list_groups().send().await,
        verbose
    ));

    // AddUserToGroup
    results.push(chk!(
        "AddUserToGroup",
        client
            .add_user_to_group()
            .group_name("conformance-group")
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // GetGroup
    results.push(chk!(
        "GetGroup",
        client
            .get_group()
            .group_name("conformance-group")
            .send()
            .await,
        verbose
    ));

    // CreateRole
    let trust_policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"lambda.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#;
    results.push(chk!(
        "CreateRole",
        client
            .create_role()
            .role_name("conformance-role")
            .assume_role_policy_document(trust_policy)
            .send()
            .await,
        verbose
    ));

    // GetRole
    results.push(chk!(
        "GetRole",
        client
            .get_role()
            .role_name("conformance-role")
            .send()
            .await,
        verbose
    ));

    // ListRoles
    results.push(chk!(
        "ListRoles",
        client.list_roles().send().await,
        verbose
    ));

    // CreatePolicy
    let policy_doc = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:*","Resource":"*"}]}"#;
    let create_policy_r = client
        .create_policy()
        .policy_name("conformance-policy")
        .policy_document(policy_doc)
        .send()
        .await;
    let policy_arn = create_policy_r
        .as_ref()
        .ok()
        .and_then(|r| r.policy.as_ref())
        .and_then(|p| p.arn.clone());
    results.push(chk!("CreatePolicy", create_policy_r, verbose));

    // ListPolicies
    results.push(chk!(
        "ListPolicies",
        client.list_policies().send().await,
        verbose
    ));

    // AttachRolePolicy
    if let Some(ref arn) = policy_arn {
        results.push(chk!(
            "AttachRolePolicy",
            client
                .attach_role_policy()
                .role_name("conformance-role")
                .policy_arn(arn)
                .send()
                .await,
            verbose
        ));

        // DetachRolePolicy
        results.push(chk!(
            "DetachRolePolicy",
            client
                .detach_role_policy()
                .role_name("conformance-role")
                .policy_arn(arn)
                .send()
                .await,
            verbose
        ));

        // DeletePolicy
        results.push(chk!(
            "DeletePolicy",
            client.delete_policy().policy_arn(arn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("AttachRolePolicy".to_string()));
        results.push(OpResult::Skipped("DetachRolePolicy".to_string()));
        results.push(OpResult::Skipped("DeletePolicy".to_string()));
    }

    // RemoveUserFromGroup (cleanup)
    results.push(chk!(
        "RemoveUserFromGroup",
        client
            .remove_user_from_group()
            .group_name("conformance-group")
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // DeleteGroup
    results.push(chk!(
        "DeleteGroup",
        client
            .delete_group()
            .group_name("conformance-group")
            .send()
            .await,
        verbose
    ));

    // DeleteRole
    results.push(chk!(
        "DeleteRole",
        client
            .delete_role()
            .role_name("conformance-role")
            .send()
            .await,
        verbose
    ));

    // DeleteUser
    results.push(chk!(
        "DeleteUser",
        client
            .delete_user()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// KMS
// ---------------------------------------------------------------------------

async fn test_kms(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_kms::Client::new(&config);
    let mut results = Vec::new();

    // CreateKey
    let create_r = client
        .create_key()
        .description("conformance test key")
        .send()
        .await;
    let key_id = create_r
        .as_ref()
        .ok()
        .and_then(|r| r.key_metadata.as_ref())
        .map(|m| m.key_id.clone());
    results.push(chk!("CreateKey", create_r, verbose));

    // ListKeys
    results.push(chk!(
        "ListKeys",
        client.list_keys().send().await,
        verbose
    ));

    if let Some(ref kid) = key_id {
        // DescribeKey
        results.push(chk!(
            "DescribeKey",
            client.describe_key().key_id(kid).send().await,
            verbose
        ));

        // CreateAlias
        results.push(chk!(
            "CreateAlias",
            client
                .create_alias()
                .alias_name("alias/conformance-key")
                .target_key_id(kid)
                .send()
                .await,
            verbose
        ));

        // ListAliases
        results.push(chk!(
            "ListAliases",
            client.list_aliases().send().await,
            verbose
        ));

        // Encrypt
        let encrypt_r = client
            .encrypt()
            .key_id(kid)
            .plaintext(aws_sdk_kms::primitives::Blob::new(b"hello conformance".to_vec()))
            .send()
            .await;
        let ciphertext = encrypt_r
            .as_ref()
            .ok()
            .and_then(|r| r.ciphertext_blob.clone());
        results.push(chk!("Encrypt", encrypt_r, verbose));

        // Decrypt
        if let Some(ct) = ciphertext {
            results.push(chk!(
                "Decrypt",
                client.decrypt().ciphertext_blob(ct).send().await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("Decrypt".to_string()));
        }

        // GenerateDataKey
        results.push(chk!(
            "GenerateDataKey",
            client
                .generate_data_key()
                .key_id(kid)
                .key_spec(aws_sdk_kms::types::DataKeySpec::Aes256)
                .send()
                .await,
            verbose
        ));

        // EnableKey / DisableKey
        results.push(chk!(
            "DisableKey",
            client.disable_key().key_id(kid).send().await,
            verbose
        ));
        results.push(chk!(
            "EnableKey",
            client.enable_key().key_id(kid).send().await,
            verbose
        ));

        // ScheduleKeyDeletion
        results.push(chk!(
            "ScheduleKeyDeletion",
            client
                .schedule_key_deletion()
                .key_id(kid)
                .pending_window_in_days(7)
                .send()
                .await,
            verbose
        ));

        // DeleteAlias
        results.push(chk!(
            "DeleteAlias",
            client
                .delete_alias()
                .alias_name("alias/conformance-key")
                .send()
                .await,
            verbose
        ));
    } else {
        for op in &[
            "DescribeKey",
            "CreateAlias",
            "ListAliases",
            "Encrypt",
            "Decrypt",
            "GenerateDataKey",
            "DisableKey",
            "EnableKey",
            "ScheduleKeyDeletion",
            "DeleteAlias",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    results
}

// ---------------------------------------------------------------------------
// Secrets Manager
// ---------------------------------------------------------------------------

async fn test_secretsmanager(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_secretsmanager::Client::new(&config);
    let mut results = Vec::new();

    // CreateSecret
    let create_r = client
        .create_secret()
        .name("conformance/secret")
        .secret_string(r#"{"password":"hunter2"}"#)
        .send()
        .await;
    let secret_id = create_r
        .as_ref()
        .ok()
        .and_then(|r| r.arn.clone());
    results.push(chk!("CreateSecret", create_r, verbose));

    // ListSecrets
    results.push(chk!(
        "ListSecrets",
        client.list_secrets().send().await,
        verbose
    ));

    if let Some(ref arn) = secret_id {
        // GetSecretValue
        results.push(chk!(
            "GetSecretValue",
            client.get_secret_value().secret_id(arn).send().await,
            verbose
        ));

        // DescribeSecret
        results.push(chk!(
            "DescribeSecret",
            client.describe_secret().secret_id(arn).send().await,
            verbose
        ));

        // PutSecretValue
        results.push(chk!(
            "PutSecretValue",
            client
                .put_secret_value()
                .secret_id(arn)
                .secret_string(r#"{"password":"updated"}"#)
                .send()
                .await,
            verbose
        ));

        // UpdateSecret
        results.push(chk!(
            "UpdateSecret",
            client
                .update_secret()
                .secret_id(arn)
                .description("updated description")
                .send()
                .await,
            verbose
        ));

        // DeleteSecret
        results.push(chk!(
            "DeleteSecret",
            client
                .delete_secret()
                .secret_id(arn)
                .force_delete_without_recovery(true)
                .send()
                .await,
            verbose
        ));
    } else {
        for op in &[
            "GetSecretValue",
            "DescribeSecret",
            "PutSecretValue",
            "UpdateSecret",
            "DeleteSecret",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    results
}

// ---------------------------------------------------------------------------
// SSM
// ---------------------------------------------------------------------------

async fn test_ssm(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_ssm::Client::new(&config);
    let mut results = Vec::new();

    // PutParameter
    results.push(chk!(
        "PutParameter",
        client
            .put_parameter()
            .name("/conformance/param")
            .value("test-value")
            .r#type(ParameterType::String)
            .send()
            .await,
        verbose
    ));

    // GetParameter
    results.push(chk!(
        "GetParameter",
        client
            .get_parameter()
            .name("/conformance/param")
            .send()
            .await,
        verbose
    ));

    // GetParameters
    results.push(chk!(
        "GetParameters",
        client
            .get_parameters()
            .names("/conformance/param")
            .send()
            .await,
        verbose
    ));

    // DescribeParameters
    results.push(chk!(
        "DescribeParameters",
        client.describe_parameters().send().await,
        verbose
    ));

    // DeleteParameter
    results.push(chk!(
        "DeleteParameter",
        client
            .delete_parameter()
            .name("/conformance/param")
            .send()
            .await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// Lambda
// ---------------------------------------------------------------------------

async fn test_lambda(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_lambda::Client::new(&config);
    let mut results = Vec::new();

    // ListFunctions (before creation)
    results.push(chk!(
        "ListFunctions",
        client.list_functions().send().await,
        verbose
    ));

    // CreateFunction — use a minimal ZIP (we can't really invoke it).
    // The ZIP contains a single file handler.py with a dummy handler.
    let zip_bytes = minimal_lambda_zip();
    let create_r = client
        .create_function()
        .function_name("conformance-fn")
        .runtime(Runtime::Python312)
        .role("arn:aws:iam::000000000000:role/conformance-role")
        .handler("handler.handler")
        .code(
            FunctionCode::builder()
                .zip_file(Blob::new(zip_bytes))
                .build(),
        )
        .send()
        .await;
    results.push(chk!("CreateFunction", create_r, verbose));

    // GetFunction
    results.push(chk!(
        "GetFunction",
        client
            .get_function()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // GetFunctionConfiguration
    results.push(chk!(
        "GetFunctionConfiguration",
        client
            .get_function_configuration()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // UpdateFunctionConfiguration
    results.push(chk!(
        "UpdateFunctionConfiguration",
        client
            .update_function_configuration()
            .function_name("conformance-fn")
            .description("updated")
            .send()
            .await,
        verbose
    ));

    // ListAliases
    results.push(chk!(
        "ListAliases",
        client
            .list_aliases()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // DeleteFunction
    results.push(chk!(
        "DeleteFunction",
        client
            .delete_function()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    results
}

/// Build a minimal valid Lambda ZIP (Python handler) in memory.
fn minimal_lambda_zip() -> Vec<u8> {
    use std::io::Write;
    let handler_code = b"def handler(event, context):\n    return {'statusCode': 200}\n";

    let mut zip_buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("handler.py", opts).unwrap();
        zip.write_all(handler_code).unwrap();
        zip.finish().unwrap();
    }
    zip_buf
}

// ---------------------------------------------------------------------------
// Kinesis
// ---------------------------------------------------------------------------

async fn test_kinesis(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_kinesis::Client::new(&config);
    let mut results = Vec::new();

    // CreateStream
    results.push(chk!(
        "CreateStream",
        client
            .create_stream()
            .stream_name("conformance-stream")
            .shard_count(1)
            .send()
            .await,
        verbose
    ));

    // ListStreams
    results.push(chk!(
        "ListStreams",
        client.list_streams().send().await,
        verbose
    ));

    // DescribeStream
    results.push(chk!(
        "DescribeStream",
        client
            .describe_stream()
            .stream_name("conformance-stream")
            .send()
            .await,
        verbose
    ));

    // DescribeStreamSummary
    results.push(chk!(
        "DescribeStreamSummary",
        client
            .describe_stream_summary()
            .stream_name("conformance-stream")
            .send()
            .await,
        verbose
    ));

    // PutRecord
    results.push(chk!(
        "PutRecord",
        client
            .put_record()
            .stream_name("conformance-stream")
            .partition_key("pk-1")
            .data(aws_sdk_kinesis::primitives::Blob::new(b"hello stream".to_vec()))
            .send()
            .await,
        verbose
    ));

    // GetShardIterator — need to know shard ID first
    let describe_r = client
        .describe_stream()
        .stream_name("conformance-stream")
        .send()
        .await;
    let shard_id = describe_r
        .as_ref()
        .ok()
        .and_then(|r| r.stream_description.as_ref())
        .and_then(|sd| sd.shards.first())
        .map(|s| s.shard_id.clone());

    if let Some(ref sid) = shard_id {
        let iter_r = client
            .get_shard_iterator()
            .stream_name("conformance-stream")
            .shard_id(sid)
            .shard_iterator_type(aws_sdk_kinesis::types::ShardIteratorType::TrimHorizon)
            .send()
            .await;
        let shard_iter = iter_r
            .as_ref()
            .ok()
            .and_then(|r| r.shard_iterator.clone());
        results.push(chk!("GetShardIterator", iter_r, verbose));

        if let Some(iter) = shard_iter {
            results.push(chk!(
                "GetRecords",
                client.get_records().shard_iterator(iter).send().await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("GetRecords".to_string()));
        }
    } else {
        results.push(OpResult::Skipped("GetShardIterator".to_string()));
        results.push(OpResult::Skipped("GetRecords".to_string()));
    }

    // ListShards
    results.push(chk!(
        "ListShards",
        client
            .list_shards()
            .stream_name("conformance-stream")
            .send()
            .await,
        verbose
    ));

    // AddTagsToStream
    results.push(chk!(
        "AddTagsToStream",
        client
            .add_tags_to_stream()
            .stream_name("conformance-stream")
            .tags("env", "conformance")
            .send()
            .await,
        verbose
    ));

    // ListTagsForStream
    results.push(chk!(
        "ListTagsForStream",
        client
            .list_tags_for_stream()
            .stream_name("conformance-stream")
            .send()
            .await,
        verbose
    ));

    // DeleteStream
    results.push(chk!(
        "DeleteStream",
        client
            .delete_stream()
            .stream_name("conformance-stream")
            .send()
            .await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// Cognito IDP
// ---------------------------------------------------------------------------

async fn test_cognito_idp(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_cognitoidentityprovider::Client::new(&config);
    let mut results = Vec::new();

    // CreateUserPool
    let create_r = client
        .create_user_pool()
        .pool_name("conformance-pool")
        .send()
        .await;
    let pool_id = create_r
        .as_ref()
        .ok()
        .and_then(|r| r.user_pool.as_ref())
        .and_then(|p| p.id.clone());
    results.push(chk!("CreateUserPool", create_r, verbose));

    // ListUserPools
    results.push(chk!(
        "ListUserPools",
        client.list_user_pools().max_results(10).send().await,
        verbose
    ));

    if let Some(ref pool_id) = pool_id {
        // DescribeUserPool
        results.push(chk!(
            "DescribeUserPool",
            client.describe_user_pool().user_pool_id(pool_id).send().await,
            verbose
        ));

        // CreateUserPoolClient
        let client_r = client
            .create_user_pool_client()
            .user_pool_id(pool_id)
            .client_name("conformance-client")
            .send()
            .await;
        let app_client_id = client_r
            .as_ref()
            .ok()
            .and_then(|r| r.user_pool_client.as_ref())
            .and_then(|c| c.client_id.clone());
        results.push(chk!("CreateUserPoolClient", client_r, verbose));

        // ListUserPoolClients
        results.push(chk!(
            "ListUserPoolClients",
            client
                .list_user_pool_clients()
                .user_pool_id(pool_id)
                .send()
                .await,
            verbose
        ));

        // AdminCreateUser
        results.push(chk!(
            "AdminCreateUser",
            client
                .admin_create_user()
                .user_pool_id(pool_id)
                .username("conformance-user")
                .send()
                .await,
            verbose
        ));

        // ListUsers
        results.push(chk!(
            "ListUsers",
            client.list_users().user_pool_id(pool_id).send().await,
            verbose
        ));

        // AdminGetUser
        results.push(chk!(
            "AdminGetUser",
            client
                .admin_get_user()
                .user_pool_id(pool_id)
                .username("conformance-user")
                .send()
                .await,
            verbose
        ));

        // AdminDeleteUser (cleanup)
        results.push(chk!(
            "AdminDeleteUser",
            client
                .admin_delete_user()
                .user_pool_id(pool_id)
                .username("conformance-user")
                .send()
                .await,
            verbose
        ));

        // DeleteUserPoolClient
        if let Some(ref cid) = app_client_id {
            results.push(chk!(
                "DeleteUserPoolClient",
                client
                    .delete_user_pool_client()
                    .user_pool_id(pool_id)
                    .client_id(cid)
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("DeleteUserPoolClient".to_string()));
        }

        // DeleteUserPool
        results.push(chk!(
            "DeleteUserPool",
            client.delete_user_pool().user_pool_id(pool_id).send().await,
            verbose
        ));
    } else {
        for op in &[
            "DescribeUserPool",
            "CreateUserPoolClient",
            "ListUserPoolClients",
            "AdminCreateUser",
            "ListUsers",
            "AdminGetUser",
            "AdminDeleteUser",
            "DeleteUserPoolClient",
            "DeleteUserPool",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    results
}

// ---------------------------------------------------------------------------
// Cognito Identity
// ---------------------------------------------------------------------------

async fn test_cognito_identity(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_cognitoidentity::Client::new(&config);
    let mut results = Vec::new();

    // CreateIdentityPool
    let create_r = client
        .create_identity_pool()
        .identity_pool_name("conformance-identity-pool")
        .allow_unauthenticated_identities(false)
        .send()
        .await;
    let pool_id = create_r
        .as_ref()
        .ok()
        .map(|r| r.identity_pool_id.clone());
    results.push(chk!("CreateIdentityPool", create_r, verbose));

    // ListIdentityPools
    results.push(chk!(
        "ListIdentityPools",
        client.list_identity_pools().max_results(10).send().await,
        verbose
    ));

    if let Some(ref pid) = pool_id {
        // DescribeIdentityPool
        results.push(chk!(
            "DescribeIdentityPool",
            client
                .describe_identity_pool()
                .identity_pool_id(pid)
                .send()
                .await,
            verbose
        ));

        // DeleteIdentityPool
        results.push(chk!(
            "DeleteIdentityPool",
            client
                .delete_identity_pool()
                .identity_pool_id(pid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DescribeIdentityPool".to_string()));
        results.push(OpResult::Skipped("DeleteIdentityPool".to_string()));
    }

    results
}

// ---------------------------------------------------------------------------
// ECS
// ---------------------------------------------------------------------------

async fn test_ecs(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_ecs::Client::new(&config);
    let mut results = Vec::new();

    // CreateCluster
    let create_r = client
        .create_cluster()
        .cluster_name("conformance-cluster")
        .send()
        .await;
    results.push(chk!("CreateCluster", create_r, verbose));

    // ListClusters
    results.push(chk!(
        "ListClusters",
        client.list_clusters().send().await,
        verbose
    ));

    // DescribeClusters
    results.push(chk!(
        "DescribeClusters",
        client
            .describe_clusters()
            .clusters("conformance-cluster")
            .send()
            .await,
        verbose
    ));

    // RegisterTaskDefinition
    results.push(chk!(
        "RegisterTaskDefinition",
        client
            .register_task_definition()
            .family("conformance-task")
            .container_definitions(
                aws_sdk_ecs::types::ContainerDefinition::builder()
                    .name("conformance-container")
                    .image("public.ecr.aws/nginx/nginx:latest")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTaskDefinitions
    results.push(chk!(
        "ListTaskDefinitions",
        client.list_task_definitions().send().await,
        verbose
    ));

    // CreateService
    results.push(chk!(
        "CreateService",
        client
            .create_service()
            .cluster("conformance-cluster")
            .service_name("conformance-service")
            .task_definition("conformance-task")
            .desired_count(0)
            .send()
            .await,
        verbose
    ));

    // ListServices
    results.push(chk!(
        "ListServices",
        client
            .list_services()
            .cluster("conformance-cluster")
            .send()
            .await,
        verbose
    ));

    // DescribeServices
    results.push(chk!(
        "DescribeServices",
        client
            .describe_services()
            .cluster("conformance-cluster")
            .services("conformance-service")
            .send()
            .await,
        verbose
    ));

    // DeleteService
    results.push(chk!(
        "DeleteService",
        client
            .delete_service()
            .cluster("conformance-cluster")
            .service("conformance-service")
            .send()
            .await,
        verbose
    ));

    // DeleteCluster
    results.push(chk!(
        "DeleteCluster",
        client
            .delete_cluster()
            .cluster("conformance-cluster")
            .send()
            .await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// ECR
// ---------------------------------------------------------------------------

async fn test_ecr(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_ecr::Client::new(&config);
    let mut results = Vec::new();

    // CreateRepository
    let create_r = client
        .create_repository()
        .repository_name("conformance-repo")
        .send()
        .await;
    results.push(chk!("CreateRepository", create_r, verbose));

    // DescribeRepositories
    results.push(chk!(
        "DescribeRepositories",
        client.describe_repositories().send().await,
        verbose
    ));

    // ListImages
    results.push(chk!(
        "ListImages",
        client
            .list_images()
            .repository_name("conformance-repo")
            .send()
            .await,
        verbose
    ));

    // GetAuthorizationToken
    results.push(chk!(
        "GetAuthorizationToken",
        client.get_authorization_token().send().await,
        verbose
    ));

    // DeleteRepository
    results.push(chk!(
        "DeleteRepository",
        client
            .delete_repository()
            .repository_name("conformance-repo")
            .send()
            .await,
        verbose
    ));

    results
}
