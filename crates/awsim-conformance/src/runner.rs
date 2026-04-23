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
        "eventbridge" => test_eventbridge(endpoint, verbose).await,
        "stepfunctions" => test_stepfunctions(endpoint, verbose).await,
        "cloudwatch-logs" => test_cloudwatch_logs(endpoint, verbose).await,
        "ec2" => test_ec2(endpoint, verbose).await,
        "cloudformation" => test_cloudformation(endpoint, verbose).await,
        "rds" => test_rds(endpoint, verbose).await,
        "route53" => test_route53(endpoint, verbose).await,
        "cloudfront" => test_cloudfront(endpoint, verbose).await,
        "elasticloadbalancingv2" => test_elb(endpoint, verbose).await,
        "acm" => test_acm(endpoint, verbose).await,
        "wafv2" => test_waf(endpoint, verbose).await,
        "scheduler" => test_scheduler(endpoint, verbose).await,
        "appsync" => test_appsync(endpoint, verbose).await,
        "glue" => test_glue(endpoint, verbose).await,
        "athena" => test_athena(endpoint, verbose).await,
        "bedrock" => test_bedrock(endpoint, verbose).await,
        "organizations" => test_organizations(endpoint, verbose).await,
        "cloudtrail" => test_cloudtrail(endpoint, verbose).await,
        "eks" => test_eks(endpoint, verbose).await,
        "firehose" => test_firehose(endpoint, verbose).await,
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

    results.push(chk!(
        "GetFederationToken",
        client
            .get_federation_token()
            .name("conformance-fed-user")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "DecodeAuthorizationMessage",
        client
            .decode_authorization_message()
            .encoded_message("FAKE-ENCODED-AUTH-MESSAGE")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "GetAccessKeyInfo",
        client
            .get_access_key_info()
            .access_key_id("ASIAEXAMPLEACCESSKEY")
            .send()
            .await,
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

    // DescribeLimits
    results.push(chk!(
        "DescribeLimits",
        client.describe_limits().send().await,
        verbose
    ));

    // ListBackups
    results.push(chk!(
        "ListBackups",
        client.list_backups().send().await,
        verbose
    ));

    // ListGlobalTables
    results.push(chk!(
        "ListGlobalTables",
        client.list_global_tables().send().await,
        verbose
    ));

    // DescribeGlobalTable (expect not-found — treated as pass)
    results.push(chk!(
        "DescribeGlobalTable",
        client
            .describe_global_table()
            .global_table_name("nonexistent-global-table")
            .send()
            .await,
        verbose
    ));

    // ListExports
    results.push(chk!(
        "ListExports",
        client.list_exports().send().await,
        verbose
    ));

    // ListImports
    results.push(chk!(
        "ListImports",
        client.list_imports().send().await,
        verbose
    ));

    // ListContributorInsights
    results.push(chk!(
        "ListContributorInsights",
        client.list_contributor_insights().send().await,
        verbose
    ));

    // DescribeContributorInsights (needs a real table — recreate briefly)
    let _ = client
        .create_table()
        .table_name("contrib-insights-test")
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

    results.push(chk!(
        "DescribeContributorInsights",
        client
            .describe_contributor_insights()
            .table_name("contrib-insights-test")
            .send()
            .await,
        verbose
    ));

    let _ = client
        .delete_table()
        .table_name("contrib-insights-test")
        .send()
        .await;

    // ExecuteStatement (PartiQL SELECT on already-deleted table — expect service error = pass)
    results.push(chk!(
        "ExecuteStatement",
        client
            .execute_statement()
            .statement(r#"SELECT * FROM "conformance-test""#)
            .send()
            .await,
        verbose
    ));

    // BatchExecuteStatement
    results.push(chk!(
        "BatchExecuteStatement",
        client
            .batch_execute_statement()
            .statements(
                aws_sdk_dynamodb::types::BatchStatementRequest::builder()
                    .statement(r#"SELECT * FROM "conformance-test""#)
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // ExecuteTransaction
    results.push(chk!(
        "ExecuteTransaction",
        client
            .execute_transaction()
            .transact_statements(
                aws_sdk_dynamodb::types::ParameterizedStatement::builder()
                    .statement(r#"SELECT * FROM "conformance-test""#)
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // UpdateContinuousBackups
    results.push(chk!(
        "UpdateContinuousBackups",
        client
            .update_continuous_backups()
            .table_name("conformance-test")
            .point_in_time_recovery_specification(
                aws_sdk_dynamodb::types::PointInTimeRecoverySpecification::builder()
                    .point_in_time_recovery_enabled(true)
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // CreateBackup + DescribeBackup + RestoreTableFromBackup
    let backup_resp = client
        .create_backup()
        .table_name("conformance-test")
        .backup_name("conformance-backup-1")
        .send()
        .await;
    let backup_arn = backup_resp
        .as_ref()
        .ok()
        .and_then(|r| r.backup_details().map(|d| d.backup_arn().to_string()));
    results.push(chk!("CreateBackup", backup_resp, verbose));

    if let Some(arn) = backup_arn {
        results.push(chk!(
            "DescribeBackup",
            client.describe_backup().backup_arn(&arn).send().await,
            verbose
        ));

        results.push(chk!(
            "RestoreTableFromBackup",
            client
                .restore_table_from_backup()
                .target_table_name("conformance-restored")
                .backup_arn(&arn)
                .send()
                .await,
            verbose
        ));

        let _ = client
            .delete_table()
            .table_name("conformance-restored")
            .send()
            .await;
    }

    // EnableKinesisStreamingDestination
    results.push(chk!(
        "EnableKinesisStreamingDestination",
        client
            .enable_kinesis_streaming_destination()
            .table_name("conformance-test")
            .stream_arn("arn:aws:kinesis:us-east-1:000000000000:stream/conformance-stream")
            .send()
            .await,
        verbose
    ));

    // DescribeKinesisStreamingDestination
    results.push(chk!(
        "DescribeKinesisStreamingDestination",
        client
            .describe_kinesis_streaming_destination()
            .table_name("conformance-test")
            .send()
            .await,
        verbose
    ));

    // DisableKinesisStreamingDestination
    results.push(chk!(
        "DisableKinesisStreamingDestination",
        client
            .disable_kinesis_streaming_destination()
            .table_name("conformance-test")
            .stream_arn("arn:aws:kinesis:us-east-1:000000000000:stream/conformance-stream")
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

    // PutBucketTagging
    results.push(chk!(
        "PutBucketTagging",
        client
            .put_bucket_tagging()
            .bucket("conformance-bucket")
            .tagging(
                aws_sdk_s3::types::Tagging::builder()
                    .tag_set(
                        aws_sdk_s3::types::Tag::builder()
                            .key("env")
                            .value("conformance")
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

    // GetBucketTagging
    results.push(chk!(
        "GetBucketTagging",
        client
            .get_bucket_tagging()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteBucketTagging
    results.push(chk!(
        "DeleteBucketTagging",
        client
            .delete_bucket_tagging()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutObjectTagging
    results.push(chk!(
        "PutObjectTagging",
        client
            .put_object_tagging()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .tagging(
                aws_sdk_s3::types::Tagging::builder()
                    .tag_set(
                        aws_sdk_s3::types::Tag::builder()
                            .key("type")
                            .value("test")
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

    // GetObjectTagging
    results.push(chk!(
        "GetObjectTagging",
        client
            .get_object_tagging()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .send()
            .await,
        verbose
    ));

    // DeleteObjectTagging
    results.push(chk!(
        "DeleteObjectTagging",
        client
            .delete_object_tagging()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .send()
            .await,
        verbose
    ));

    // PutBucketPolicy
    let policy_doc = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"s3:GetObject","Resource":"arn:aws:s3:::conformance-bucket/*"}]}"#;
    results.push(chk!(
        "PutBucketPolicy",
        client
            .put_bucket_policy()
            .bucket("conformance-bucket")
            .policy(policy_doc)
            .send()
            .await,
        verbose
    ));

    // GetBucketPolicy
    results.push(chk!(
        "GetBucketPolicy",
        client
            .get_bucket_policy()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteBucketPolicy
    results.push(chk!(
        "DeleteBucketPolicy",
        client
            .delete_bucket_policy()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketCors
    results.push(chk!(
        "PutBucketCors",
        client
            .put_bucket_cors()
            .bucket("conformance-bucket")
            .cors_configuration(
                aws_sdk_s3::types::CorsConfiguration::builder()
                    .cors_rules(
                        aws_sdk_s3::types::CorsRule::builder()
                            .allowed_methods("GET")
                            .allowed_origins("*")
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

    // GetBucketCors
    results.push(chk!(
        "GetBucketCors",
        client
            .get_bucket_cors()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteBucketCors
    results.push(chk!(
        "DeleteBucketCors",
        client
            .delete_bucket_cors()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // GetBucketAcl
    results.push(chk!(
        "GetBucketAcl",
        client
            .get_bucket_acl()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketAcl
    results.push(chk!(
        "PutBucketAcl",
        client
            .put_bucket_acl()
            .bucket("conformance-bucket")
            .acl(aws_sdk_s3::types::BucketCannedAcl::Private)
            .send()
            .await,
        verbose
    ));

    // GetObjectAcl
    results.push(chk!(
        "GetObjectAcl",
        client
            .get_object_acl()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .send()
            .await,
        verbose
    ));

    // PutBucketLifecycleConfiguration
    results.push(chk!(
        "PutBucketLifecycleConfiguration",
        client
            .put_bucket_lifecycle_configuration()
            .bucket("conformance-bucket")
            .lifecycle_configuration(
                aws_sdk_s3::types::BucketLifecycleConfiguration::builder()
                    .rules(
                        aws_sdk_s3::types::LifecycleRule::builder()
                            .id("conformance-rule")
                            .status(aws_sdk_s3::types::ExpirationStatus::Enabled)
                            .expiration(
                                aws_sdk_s3::types::LifecycleExpiration::builder()
                                    .days(30)
                                    .build(),
                            )
                            .filter(aws_sdk_s3::types::LifecycleRuleFilter::builder().prefix("logs/").build())
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

    // GetBucketLifecycleConfiguration
    results.push(chk!(
        "GetBucketLifecycleConfiguration",
        client
            .get_bucket_lifecycle_configuration()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteBucketLifecycleConfiguration
    results.push(chk!(
        "DeleteBucketLifecycleConfiguration",
        client
            .delete_bucket_lifecycle()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketEncryption
    results.push(chk!(
        "PutBucketEncryption",
        client
            .put_bucket_encryption()
            .bucket("conformance-bucket")
            .server_side_encryption_configuration(
                aws_sdk_s3::types::ServerSideEncryptionConfiguration::builder()
                    .rules(
                        aws_sdk_s3::types::ServerSideEncryptionRule::builder()
                            .apply_server_side_encryption_by_default(
                                aws_sdk_s3::types::ServerSideEncryptionByDefault::builder()
                                    .sse_algorithm(aws_sdk_s3::types::ServerSideEncryption::Aes256)
                                    .build()
                                    .unwrap(),
                            )
                            .build(),
                    )
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketEncryption
    results.push(chk!(
        "GetBucketEncryption",
        client
            .get_bucket_encryption()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteBucketEncryption
    results.push(chk!(
        "DeleteBucketEncryption",
        client
            .delete_bucket_encryption()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketLogging
    results.push(chk!(
        "PutBucketLogging",
        client
            .put_bucket_logging()
            .bucket("conformance-bucket")
            .bucket_logging_status(
                aws_sdk_s3::types::BucketLoggingStatus::builder()
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketLogging
    results.push(chk!(
        "GetBucketLogging",
        client
            .get_bucket_logging()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketWebsite
    results.push(chk!(
        "PutBucketWebsite",
        client
            .put_bucket_website()
            .bucket("conformance-bucket")
            .website_configuration(
                aws_sdk_s3::types::WebsiteConfiguration::builder()
                    .index_document(
                        aws_sdk_s3::types::IndexDocument::builder()
                            .suffix("index.html")
                            .build()
                            .unwrap(),
                    )
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketWebsite
    results.push(chk!(
        "GetBucketWebsite",
        client
            .get_bucket_website()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteBucketWebsite
    results.push(chk!(
        "DeleteBucketWebsite",
        client
            .delete_bucket_website()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketOwnershipControls
    results.push(chk!(
        "PutBucketOwnershipControls",
        client
            .put_bucket_ownership_controls()
            .bucket("conformance-bucket")
            .ownership_controls(
                aws_sdk_s3::types::OwnershipControls::builder()
                    .rules(
                        aws_sdk_s3::types::OwnershipControlsRule::builder()
                            .object_ownership(aws_sdk_s3::types::ObjectOwnership::BucketOwnerEnforced)
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

    // GetBucketOwnershipControls
    results.push(chk!(
        "GetBucketOwnershipControls",
        client
            .get_bucket_ownership_controls()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteBucketOwnershipControls
    results.push(chk!(
        "DeleteBucketOwnershipControls",
        client
            .delete_bucket_ownership_controls()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutPublicAccessBlock
    results.push(chk!(
        "PutPublicAccessBlock",
        client
            .put_public_access_block()
            .bucket("conformance-bucket")
            .public_access_block_configuration(
                aws_sdk_s3::types::PublicAccessBlockConfiguration::builder()
                    .block_public_acls(true)
                    .block_public_policy(true)
                    .ignore_public_acls(true)
                    .restrict_public_buckets(true)
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetPublicAccessBlock
    results.push(chk!(
        "GetPublicAccessBlock",
        client
            .get_public_access_block()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeletePublicAccessBlock
    results.push(chk!(
        "DeletePublicAccessBlock",
        client
            .delete_public_access_block()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketRequestPayment
    results.push(chk!(
        "PutBucketRequestPayment",
        client
            .put_bucket_request_payment()
            .bucket("conformance-bucket")
            .request_payment_configuration(
                aws_sdk_s3::types::RequestPaymentConfiguration::builder()
                    .payer(aws_sdk_s3::types::Payer::BucketOwner)
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketRequestPayment
    results.push(chk!(
        "GetBucketRequestPayment",
        client
            .get_bucket_request_payment()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketAccelerateConfiguration
    results.push(chk!(
        "PutBucketAccelerateConfiguration",
        client
            .put_bucket_accelerate_configuration()
            .bucket("conformance-bucket")
            .accelerate_configuration(
                aws_sdk_s3::types::AccelerateConfiguration::builder()
                    .status(aws_sdk_s3::types::BucketAccelerateStatus::Enabled)
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketAccelerateConfiguration
    results.push(chk!(
        "GetBucketAccelerateConfiguration",
        client
            .get_bucket_accelerate_configuration()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketNotificationConfiguration
    results.push(chk!(
        "PutBucketNotificationConfiguration",
        client
            .put_bucket_notification_configuration()
            .bucket("conformance-bucket")
            .notification_configuration(
                aws_sdk_s3::types::NotificationConfiguration::builder()
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetBucketNotificationConfiguration
    results.push(chk!(
        "GetBucketNotificationConfiguration",
        client
            .get_bucket_notification_configuration()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // PutBucketReplication (requires versioning enabled — already enabled above)
    results.push(chk!(
        "PutBucketReplication",
        client
            .put_bucket_replication()
            .bucket("conformance-bucket")
            .replication_configuration(
                aws_sdk_s3::types::ReplicationConfiguration::builder()
                    .role("arn:aws:iam::000000000000:role/replication-role")
                    .rules(
                        aws_sdk_s3::types::ReplicationRule::builder()
                            .status(aws_sdk_s3::types::ReplicationRuleStatus::Enabled)
                            .destination(
                                aws_sdk_s3::types::Destination::builder()
                                    .bucket("arn:aws:s3:::conformance-bucket-dest")
                                    .build()
                                    .unwrap(),
                            )
                            .filter(aws_sdk_s3::types::ReplicationRuleFilter::builder().prefix("").build())
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

    // GetBucketReplication
    results.push(chk!(
        "GetBucketReplication",
        client
            .get_bucket_replication()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // DeleteBucketReplication
    results.push(chk!(
        "DeleteBucketReplication",
        client
            .delete_bucket_replication()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // Multipart upload
    let mpu_r = client
        .create_multipart_upload()
        .bucket("conformance-bucket")
        .key("multipart-object.txt")
        .send()
        .await;
    let upload_id = mpu_r
        .as_ref()
        .ok()
        .and_then(|r| r.upload_id.clone());
    results.push(chk!("CreateMultipartUpload", mpu_r, verbose));

    if let Some(ref uid) = upload_id {
        // UploadPart (minimum 5MB for real S3 but sim should accept smaller)
        let part_data = vec![b'x'; 5 * 1024 * 1024]; // 5MB
        let up_r = client
            .upload_part()
            .bucket("conformance-bucket")
            .key("multipart-object.txt")
            .upload_id(uid)
            .part_number(1)
            .body(aws_sdk_s3::primitives::ByteStream::from(part_data))
            .send()
            .await;
        let etag = up_r.as_ref().ok().and_then(|r| r.e_tag.clone());
        results.push(chk!("UploadPart", up_r, verbose));

        // ListParts
        results.push(chk!(
            "ListParts",
            client
                .list_parts()
                .bucket("conformance-bucket")
                .key("multipart-object.txt")
                .upload_id(uid)
                .send()
                .await,
            verbose
        ));

        // ListMultipartUploads
        results.push(chk!(
            "ListMultipartUploads",
            client
                .list_multipart_uploads()
                .bucket("conformance-bucket")
                .send()
                .await,
            verbose
        ));

        if let Some(et) = etag {
            // CompleteMultipartUpload
            results.push(chk!(
                "CompleteMultipartUpload",
                client
                    .complete_multipart_upload()
                    .bucket("conformance-bucket")
                    .key("multipart-object.txt")
                    .upload_id(uid)
                    .multipart_upload(
                        aws_sdk_s3::types::CompletedMultipartUpload::builder()
                            .parts(
                                aws_sdk_s3::types::CompletedPart::builder()
                                    .part_number(1)
                                    .e_tag(et)
                                    .build(),
                            )
                            .build(),
                    )
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("CompleteMultipartUpload".to_string()));
        }

        // AbortMultipartUpload (create a fresh one to abort)
        let abort_mpu = client
            .create_multipart_upload()
            .bucket("conformance-bucket")
            .key("abort-multipart.txt")
            .send()
            .await;
        if let Some(abort_uid) = abort_mpu.ok().and_then(|r| r.upload_id) {
            results.push(chk!(
                "AbortMultipartUpload",
                client
                    .abort_multipart_upload()
                    .bucket("conformance-bucket")
                    .key("abort-multipart.txt")
                    .upload_id(abort_uid)
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("AbortMultipartUpload".to_string()));
        }
    } else {
        for op in &["UploadPart", "ListParts", "ListMultipartUploads", "CompleteMultipartUpload", "AbortMultipartUpload"] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    // ListObjects (v1)
    results.push(chk!(
        "ListObjects",
        client
            .list_objects()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // ListObjectVersions
    results.push(chk!(
        "ListObjectVersions",
        client
            .list_object_versions()
            .bucket("conformance-bucket")
            .send()
            .await,
        verbose
    ));

    // GetObjectAttributes
    results.push(chk!(
        "GetObjectAttributes",
        client
            .get_object_attributes()
            .bucket("conformance-bucket")
            .key("test-object.txt")
            .object_attributes(aws_sdk_s3::types::ObjectAttributes::Etag)
            .object_attributes(aws_sdk_s3::types::ObjectAttributes::ObjectSize)
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

    // DeleteObjects (also clean up multipart-object.txt)
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
                    .objects(
                        aws_sdk_s3::types::ObjectIdentifier::builder()
                            .key("multipart-object.txt")
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

    // ChangeMessageVisibility (use receipt handle if available)
    if let Some(ref handle) = receipt_handle {
        results.push(chk!(
            "ChangeMessageVisibility",
            client
                .change_message_visibility()
                .queue_url(&queue_url)
                .receipt_handle(handle)
                .visibility_timeout(30)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("ChangeMessageVisibility".to_string()));
    }

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
    if let Some(ref handle) = receipt_handle {
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

    // DeleteMessageBatch — receive a fresh message first
    let recv2 = client
        .receive_message()
        .queue_url(&queue_url)
        .max_number_of_messages(1)
        .send()
        .await;
    let handle2 = recv2
        .as_ref()
        .ok()
        .and_then(|r| r.messages.as_ref())
        .and_then(|m| m.first())
        .and_then(|m| m.receipt_handle.clone());
    if let Some(h) = handle2 {
        results.push(chk!(
            "DeleteMessageBatch",
            client
                .delete_message_batch()
                .queue_url(&queue_url)
                .entries(
                    aws_sdk_sqs::types::DeleteMessageBatchRequestEntry::builder()
                        .id("del-1")
                        .receipt_handle(h)
                        .build()
                        .unwrap(),
                )
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteMessageBatch".to_string()));
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

    // TagQueue
    results.push(chk!(
        "TagQueue",
        client
            .tag_queue()
            .queue_url(&queue_url)
            .tags("env", "conformance")
            .send()
            .await,
        verbose
    ));

    // ListQueueTags
    results.push(chk!(
        "ListQueueTags",
        client.list_queue_tags().queue_url(&queue_url).send().await,
        verbose
    ));

    // UntagQueue
    results.push(chk!(
        "UntagQueue",
        client
            .untag_queue()
            .queue_url(&queue_url)
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // ListDeadLetterSourceQueues
    results.push(chk!(
        "ListDeadLetterSourceQueues",
        client
            .list_dead_letter_source_queues()
            .queue_url(&queue_url)
            .send()
            .await,
        verbose
    ));

    // ListMessageMoveTasks
    let dlq_arn = format!(
        "arn:aws:sqs:us-east-1:000000000000:{}",
        "conformance-queue"
    );
    results.push(chk!(
        "ListMessageMoveTasks",
        client
            .list_message_move_tasks()
            .source_arn(&dlq_arn)
            .send()
            .await,
        verbose
    ));

    // AddPermission
    results.push(chk!(
        "AddPermission",
        client
            .add_permission()
            .queue_url(&queue_url)
            .label("conformance-perm")
            .aws_account_ids("000000000000")
            .actions("SendMessage")
            .send()
            .await,
        verbose
    ));

    // RemovePermission
    results.push(chk!(
        "RemovePermission",
        client
            .remove_permission()
            .queue_url(&queue_url)
            .label("conformance-perm")
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

    // SetTopicAttributes
    results.push(chk!(
        "SetTopicAttributes",
        client
            .set_topic_attributes()
            .topic_arn(&topic_arn)
            .attribute_name("DisplayName")
            .attribute_value("Conformance Topic")
            .send()
            .await,
        verbose
    ));

    // GetSubscriptionAttributes (if we got a subscription ARN)
    if let Some(ref sub_arn) = subscription_arn {
        results.push(chk!(
            "GetSubscriptionAttributes",
            client
                .get_subscription_attributes()
                .subscription_arn(sub_arn)
                .send()
                .await,
            verbose
        ));

        // SetSubscriptionAttributes
        results.push(chk!(
            "SetSubscriptionAttributes",
            client
                .set_subscription_attributes()
                .subscription_arn(sub_arn)
                .attribute_name("RawMessageDelivery")
                .attribute_value("true")
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetSubscriptionAttributes".to_string()));
        results.push(OpResult::Skipped("SetSubscriptionAttributes".to_string()));
    }

    // TagResource (SNS)
    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource_arn(&topic_arn)
            .tags(
                aws_sdk_sns::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTagsForResource (SNS)
    results.push(chk!(
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_arn(&topic_arn)
            .send()
            .await,
        verbose
    ));

    // UntagResource (SNS)
    results.push(chk!(
        "UntagResource",
        client
            .untag_resource()
            .resource_arn(&topic_arn)
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // PublishBatch
    results.push(chk!(
        "PublishBatch",
        client
            .publish_batch()
            .topic_arn(&topic_arn)
            .publish_batch_request_entries(
                aws_sdk_sns::types::PublishBatchRequestEntry::builder()
                    .id("msg-1")
                    .message("batch conformance message")
                    .build()
                    .unwrap(),
            )
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

    // AddPermission (SNS)
    results.push(chk!(
        "AddPermission",
        client
            .add_permission()
            .topic_arn(&topic_arn)
            .label("conformance-perm")
            .aws_account_id("000000000000")
            .action_name("Publish")
            .send()
            .await,
        verbose
    ));

    // RemovePermission (SNS)
    results.push(chk!(
        "RemovePermission",
        client
            .remove_permission()
            .topic_arn(&topic_arn)
            .label("conformance-perm")
            .send()
            .await,
        verbose
    ));

    // CheckIfPhoneNumberIsOptedOut
    results.push(chk!(
        "CheckIfPhoneNumberIsOptedOut",
        client
            .check_if_phone_number_is_opted_out()
            .phone_number("+15005550006")
            .send()
            .await,
        verbose
    ));

    // ListPhoneNumbersOptedOut
    results.push(chk!(
        "ListPhoneNumbersOptedOut",
        client.list_phone_numbers_opted_out().send().await,
        verbose
    ));

    // GetSMSAttributes
    results.push(chk!(
        "GetSMSAttributes",
        client.get_sms_attributes().send().await,
        verbose
    ));

    // SetSMSAttributes
    results.push(chk!(
        "SetSMSAttributes",
        client
            .set_sms_attributes()
            .attributes("DefaultSMSType", "Transactional")
            .send()
            .await,
        verbose
    ));

    // OptInPhoneNumber
    results.push(chk!(
        "OptInPhoneNumber",
        client
            .opt_in_phone_number()
            .phone_number("+15005550006")
            .send()
            .await,
        verbose
    ));

    // ListOriginationNumbers
    results.push(chk!(
        "ListOriginationNumbers",
        client.list_origination_numbers().send().await,
        verbose
    ));

    // CreatePlatformApplication
    let platform_app_r = client
        .create_platform_application()
        .name("conformance-app")
        .platform("GCM")
        .attributes("PlatformCredential", "fake-server-key")
        .send()
        .await;
    let platform_app_arn = platform_app_r
        .as_ref()
        .ok()
        .and_then(|r| r.platform_application_arn.clone());
    results.push(chk!("CreatePlatformApplication", platform_app_r, verbose));

    // ListPlatformApplications
    results.push(chk!(
        "ListPlatformApplications",
        client.list_platform_applications().send().await,
        verbose
    ));

    if let Some(ref app_arn) = platform_app_arn {
        // GetPlatformApplicationAttributes
        results.push(chk!(
            "GetPlatformApplicationAttributes",
            client
                .get_platform_application_attributes()
                .platform_application_arn(app_arn)
                .send()
                .await,
            verbose
        ));

        // SetPlatformApplicationAttributes
        results.push(chk!(
            "SetPlatformApplicationAttributes",
            client
                .set_platform_application_attributes()
                .platform_application_arn(app_arn)
                .attributes("EventDeliveryFailure", "arn:aws:sns:us-east-1:000000000000:conformance-topic")
                .send()
                .await,
            verbose
        ));

        // CreatePlatformEndpoint
        let endpoint_r = client
            .create_platform_endpoint()
            .platform_application_arn(app_arn)
            .token("fake-device-token-conformance")
            .send()
            .await;
        let endpoint_arn = endpoint_r
            .as_ref()
            .ok()
            .and_then(|r| r.endpoint_arn.clone());
        results.push(chk!("CreatePlatformEndpoint", endpoint_r, verbose));

        // ListEndpointsByPlatformApplication
        results.push(chk!(
            "ListEndpointsByPlatformApplication",
            client
                .list_endpoints_by_platform_application()
                .platform_application_arn(app_arn)
                .send()
                .await,
            verbose
        ));

        if let Some(ref ep_arn) = endpoint_arn {
            // GetEndpointAttributes
            results.push(chk!(
                "GetEndpointAttributes",
                client.get_endpoint_attributes().endpoint_arn(ep_arn).send().await,
                verbose
            ));

            // SetEndpointAttributes
            results.push(chk!(
                "SetEndpointAttributes",
                client
                    .set_endpoint_attributes()
                    .endpoint_arn(ep_arn)
                    .attributes("Enabled", "true")
                    .send()
                    .await,
                verbose
            ));

            // DeleteEndpoint
            results.push(chk!(
                "DeleteEndpoint",
                client.delete_endpoint().endpoint_arn(ep_arn).send().await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("GetEndpointAttributes".to_string()));
            results.push(OpResult::Skipped("SetEndpointAttributes".to_string()));
            results.push(OpResult::Skipped("DeleteEndpoint".to_string()));
        }

        // DeletePlatformApplication
        results.push(chk!(
            "DeletePlatformApplication",
            client
                .delete_platform_application()
                .platform_application_arn(app_arn)
                .send()
                .await,
            verbose
        ));
    } else {
        for op in &[
            "GetPlatformApplicationAttributes",
            "SetPlatformApplicationAttributes",
            "CreatePlatformEndpoint",
            "ListEndpointsByPlatformApplication",
            "GetEndpointAttributes",
            "SetEndpointAttributes",
            "DeleteEndpoint",
            "DeletePlatformApplication",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    // GetSMSSandboxAccountStatus
    results.push(chk!(
        "GetSMSSandboxAccountStatus",
        client.get_sms_sandbox_account_status().send().await,
        verbose
    ));

    // ListSMSSandboxPhoneNumbers
    results.push(chk!(
        "ListSMSSandboxPhoneNumbers",
        client.list_sms_sandbox_phone_numbers().send().await,
        verbose
    ));

    // PutDataProtectionPolicy
    let dp_policy = r#"{"Name":"conformance","Version":"2021-06-01","Statement":[]}"#;
    results.push(chk!(
        "PutDataProtectionPolicy",
        client
            .put_data_protection_policy()
            .resource_arn(&topic_arn)
            .data_protection_policy(dp_policy)
            .send()
            .await,
        verbose
    ));

    // GetDataProtectionPolicy
    results.push(chk!(
        "GetDataProtectionPolicy",
        client
            .get_data_protection_policy()
            .resource_arn(&topic_arn)
            .send()
            .await,
        verbose
    ));

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

    // CreateUser again for supplemental tests
    let _ = client
        .create_user()
        .user_name("conformance-user2")
        .send()
        .await;

    // CreateAccessKey (for conformance-user2)
    let ak_r = client
        .create_access_key()
        .user_name("conformance-user2")
        .send()
        .await;
    let access_key_id = ak_r
        .as_ref()
        .ok()
        .and_then(|r| r.access_key.as_ref())
        .map(|ak| ak.access_key_id.clone());

    // DeleteAccessKey
    if let Some(ref akid) = access_key_id {
        results.push(chk!(
            "DeleteAccessKey",
            client
                .delete_access_key()
                .user_name("conformance-user2")
                .access_key_id(akid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteAccessKey".to_string()));
    }

    // AttachUserPolicy / DetachUserPolicy
    let policy_doc2 = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"sqs:*","Resource":"*"}]}"#;
    let up_r = client
        .create_policy()
        .policy_name("conformance-user-policy")
        .policy_document(policy_doc2)
        .send()
        .await;
    let user_policy_arn = up_r
        .as_ref()
        .ok()
        .and_then(|r| r.policy.as_ref())
        .and_then(|p| p.arn.clone());

    if let Some(ref uarn) = user_policy_arn {
        results.push(chk!(
            "AttachUserPolicy",
            client
                .attach_user_policy()
                .user_name("conformance-user2")
                .policy_arn(uarn)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ListAttachedUserPolicies",
            client
                .list_attached_user_policies()
                .user_name("conformance-user2")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "DetachUserPolicy",
            client
                .detach_user_policy()
                .user_name("conformance-user2")
                .policy_arn(uarn)
                .send()
                .await,
            verbose
        ));

        // CreatePolicyVersion
        results.push(chk!(
            "CreatePolicyVersion",
            client
                .create_policy_version()
                .policy_arn(uarn)
                .policy_document(policy_doc2)
                .send()
                .await,
            verbose
        ));

        // ListPolicyVersions
        results.push(chk!(
            "ListPolicyVersions",
            client
                .list_policy_versions()
                .policy_arn(uarn)
                .send()
                .await,
            verbose
        ));

        // GetPolicyVersion
        results.push(chk!(
            "GetPolicyVersion",
            client
                .get_policy_version()
                .policy_arn(uarn)
                .version_id("v1")
                .send()
                .await,
            verbose
        ));

        let _ = client.delete_policy().policy_arn(uarn).send().await;
    } else {
        for op in &[
            "AttachUserPolicy",
            "ListAttachedUserPolicies",
            "DetachUserPolicy",
            "CreatePolicyVersion",
            "ListPolicyVersions",
            "GetPolicyVersion",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    // PutUserPolicy / GetUserPolicy / ListUserPolicies / DeleteUserPolicy
    results.push(chk!(
        "PutUserPolicy",
        client
            .put_user_policy()
            .user_name("conformance-user2")
            .policy_name("inline-policy")
            .policy_document(
                r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"*"}]}"#,
            )
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "GetUserPolicy",
        client
            .get_user_policy()
            .user_name("conformance-user2")
            .policy_name("inline-policy")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "ListUserPolicies",
        client
            .list_user_policies()
            .user_name("conformance-user2")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "DeleteUserPolicy",
        client
            .delete_user_policy()
            .user_name("conformance-user2")
            .policy_name("inline-policy")
            .send()
            .await,
        verbose
    ));

    // ListAttachedRolePolicies (use conformance-role which may not exist anymore; it was deleted above)
    // Create a temporary role for this
    let tr_doc = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"ec2.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#;
    let _ = client
        .create_role()
        .role_name("conformance-role2")
        .assume_role_policy_document(tr_doc)
        .send()
        .await;

    results.push(chk!(
        "ListAttachedRolePolicies",
        client
            .list_attached_role_policies()
            .role_name("conformance-role2")
            .send()
            .await,
        verbose
    ));

    let _ = client.delete_role().role_name("conformance-role2").send().await;

    // CreateInstanceProfile / GetInstanceProfile / DeleteInstanceProfile
    results.push(chk!(
        "CreateInstanceProfile",
        client
            .create_instance_profile()
            .instance_profile_name("conformance-profile")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "GetInstanceProfile",
        client
            .get_instance_profile()
            .instance_profile_name("conformance-profile")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "DeleteInstanceProfile",
        client
            .delete_instance_profile()
            .instance_profile_name("conformance-profile")
            .send()
            .await,
        verbose
    ));

    // TagUser / ListUserTags / UntagUser
    results.push(chk!(
        "TagUser",
        client
            .tag_user()
            .user_name("conformance-user2")
            .tags(
                aws_sdk_iam::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "ListUserTags",
        client
            .list_user_tags()
            .user_name("conformance-user2")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "UntagUser",
        client
            .untag_user()
            .user_name("conformance-user2")
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // GetAccountSummary
    results.push(chk!(
        "GetAccountSummary",
        client.get_account_summary().send().await,
        verbose
    ));

    // ListAccountAliases
    results.push(chk!(
        "ListAccountAliases",
        client.list_account_aliases().send().await,
        verbose
    ));

    // ListInstanceProfiles
    results.push(chk!(
        "ListInstanceProfiles",
        client.list_instance_profiles().send().await,
        verbose
    ));

    // CreateInstanceProfile + ListInstanceProfilesForRole
    let _ = client
        .create_instance_profile()
        .instance_profile_name("conformance-profile2")
        .send()
        .await;

    // Create a role to associate
    let tr_doc2 = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"ec2.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#;
    let _ = client
        .create_role()
        .role_name("conformance-role3")
        .assume_role_policy_document(tr_doc2)
        .send()
        .await;

    let _ = client
        .add_role_to_instance_profile()
        .instance_profile_name("conformance-profile2")
        .role_name("conformance-role3")
        .send()
        .await;

    results.push(chk!(
        "ListInstanceProfilesForRole",
        client
            .list_instance_profiles_for_role()
            .role_name("conformance-role3")
            .send()
            .await,
        verbose
    ));

    // Cleanup
    let _ = client
        .remove_role_from_instance_profile()
        .instance_profile_name("conformance-profile2")
        .role_name("conformance-role3")
        .send()
        .await;
    let _ = client
        .delete_instance_profile()
        .instance_profile_name("conformance-profile2")
        .send()
        .await;
    let _ = client
        .delete_role()
        .role_name("conformance-role3")
        .send()
        .await;

    // CreateLoginProfile / GetLoginProfile / UpdateLoginProfile / DeleteLoginProfile
    // Use conformance-user which still exists at this point.
    results.push(chk!(
        "CreateLoginProfile",
        client
            .create_login_profile()
            .user_name("conformance-user")
            .password("Pass@word1!")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "GetLoginProfile",
        client
            .get_login_profile()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "UpdateLoginProfile",
        client
            .update_login_profile()
            .user_name("conformance-user")
            .password("NewPass@word2!")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "DeleteLoginProfile",
        client
            .delete_login_profile()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // ListSigningCertificates
    results.push(chk!(
        "ListSigningCertificates",
        client
            .list_signing_certificates()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // GetAccountPasswordPolicy
    results.push(chk!(
        "GetAccountPasswordPolicy",
        client.get_account_password_policy().send().await,
        verbose
    ));

    // SimulateCustomPolicy
    results.push(chk!(
        "SimulateCustomPolicy",
        client
            .simulate_custom_policy()
            .policy_input_list(
                r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:*","Resource":"*"}]}"#,
            )
            .action_names("s3:GetObject")
            .resource_arns("*")
            .send()
            .await,
        verbose
    ));

    // SimulatePrincipalPolicy
    results.push(chk!(
        "SimulatePrincipalPolicy",
        client
            .simulate_principal_policy()
            .policy_source_arn(format!("arn:aws:iam::000000000000:user/conformance-user"))
            .action_names("s3:GetObject")
            .resource_arns("*")
            .send()
            .await,
        verbose
    ));

    // GetContextKeysForCustomPolicy
    results.push(chk!(
        "GetContextKeysForCustomPolicy",
        client
            .get_context_keys_for_custom_policy()
            .policy_input_list(
                r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:*","Resource":"*"}]}"#,
            )
            .send()
            .await,
        verbose
    ));

    // GetContextKeysForPrincipalPolicy
    results.push(chk!(
        "GetContextKeysForPrincipalPolicy",
        client
            .get_context_keys_for_principal_policy()
            .policy_source_arn(format!("arn:aws:iam::000000000000:user/conformance-user"))
            .send()
            .await,
        verbose
    ));

    // ListGroupsForUser
    results.push(chk!(
        "ListGroupsForUser",
        client
            .list_groups_for_user()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // ChangePassword
    results.push(chk!(
        "ChangePassword",
        client
            .change_password()
            .old_password("OldPass@word1!")
            .new_password("NewPass@word2!")
            .send()
            .await,
        verbose
    ));

    // CreateVirtualMFADevice + GetMFADevice + DeleteVirtualMFADevice
    let mfa_r = client
        .create_virtual_mfa_device()
        .virtual_mfa_device_name("conformance-mfa")
        .send()
        .await;
    let mfa_serial = mfa_r
        .as_ref()
        .ok()
        .and_then(|r| r.virtual_mfa_device.as_ref())
        .map(|d| d.serial_number.clone());
    results.push(chk!("CreateVirtualMFADevice", mfa_r, verbose));

    if let Some(serial) = mfa_serial.as_ref() {
        results.push(chk!(
            "GetMFADevice",
            client.get_mfa_device().serial_number(serial).send().await,
            verbose
        ));
        let _ = client
            .delete_virtual_mfa_device()
            .serial_number(serial)
            .send()
            .await;
    } else {
        results.push(OpResult::Skipped("GetMFADevice".to_string()));
    }

    // CreateServiceSpecificCredential / List / Delete
    let ssc_r = client
        .create_service_specific_credential()
        .user_name("conformance-user")
        .service_name("codecommit.amazonaws.com")
        .send()
        .await;
    let ssc_id = ssc_r
        .as_ref()
        .ok()
        .and_then(|r| r.service_specific_credential.as_ref())
        .map(|c| c.service_specific_credential_id.clone());
    results.push(chk!("CreateServiceSpecificCredential", ssc_r, verbose));

    results.push(chk!(
        "ListServiceSpecificCredentials",
        client
            .list_service_specific_credentials()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    if let Some(id) = ssc_id {
        let _ = client
            .delete_service_specific_credential()
            .service_specific_credential_id(id)
            .send()
            .await;
    }

    // UploadServerCertificate + UpdateServerCertificate + DeleteServerCertificate
    let cert_body = "-----BEGIN CERTIFICATE-----\nMIIDummy\n-----END CERTIFICATE-----";
    let private_key = "-----BEGIN PRIVATE KEY-----\nDUMMY\n-----END PRIVATE KEY-----";
    results.push(chk!(
        "UploadServerCertificate",
        client
            .upload_server_certificate()
            .server_certificate_name("conformance-cert")
            .certificate_body(cert_body)
            .private_key(private_key)
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "UpdateServerCertificate",
        client
            .update_server_certificate()
            .server_certificate_name("conformance-cert")
            .new_server_certificate_name("conformance-cert-renamed")
            .send()
            .await,
        verbose
    ));

    let _ = client
        .delete_server_certificate()
        .server_certificate_name("conformance-cert-renamed")
        .send()
        .await;

    // DeleteUser (cleanup user2)
    let _ = client
        .delete_user()
        .user_name("conformance-user2")
        .send()
        .await;

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

        // GenerateDataKeyWithoutPlaintext
        results.push(chk!(
            "GenerateDataKeyWithoutPlaintext",
            client
                .generate_data_key_without_plaintext()
                .key_id(kid)
                .key_spec(aws_sdk_kms::types::DataKeySpec::Aes256)
                .send()
                .await,
            verbose
        ));

        // ReEncrypt — re-encrypt data from the same key to itself
        if let Some(ct_for_reencrypt) = {
            client
                .encrypt()
                .key_id(kid)
                .plaintext(aws_sdk_kms::primitives::Blob::new(b"reencrypt-me".to_vec()))
                .send()
                .await
                .ok()
                .and_then(|r| r.ciphertext_blob)
        } {
            results.push(chk!(
                "ReEncrypt",
                client
                    .re_encrypt()
                    .ciphertext_blob(ct_for_reencrypt)
                    .destination_key_id(kid)
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("ReEncrypt".to_string()));
        }

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
            "GenerateDataKeyWithoutPlaintext",
            "ReEncrypt",
            "DisableKey",
            "EnableKey",
            "ScheduleKeyDeletion",
            "DeleteAlias",
            // New ops
            "UpdateKeyDescription",
            "GetKeyRotationStatus",
            "EnableKeyRotation",
            "DisableKeyRotation",
            "CreateGrant",
            "ListGrants",
            "RetireGrant",
            "RevokeGrant",
            "GetKeyPolicy",
            "PutKeyPolicy",
            "ListKeyPolicies",
            "TagResource",
            "UntagResource",
            "ListResourceTags",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    // GenerateRandom — not tied to a key
    results.push(chk!(
        "GenerateRandom",
        client.generate_random().number_of_bytes(32).send().await,
        verbose
    ));

    // CreateCustomKeyStore / DescribeCustomKeyStores / DeleteCustomKeyStore
    let cks_r = client
        .create_custom_key_store()
        .custom_key_store_name("conformance-cks")
        .send()
        .await;
    let cks_id = cks_r
        .as_ref()
        .ok()
        .and_then(|r| r.custom_key_store_id.clone());
    results.push(chk!("CreateCustomKeyStore", cks_r, verbose));

    results.push(chk!(
        "DescribeCustomKeyStores",
        client.describe_custom_key_stores().send().await,
        verbose
    ));

    if let Some(ref cks) = cks_id {
        results.push(chk!(
            "DeleteCustomKeyStore",
            client.delete_custom_key_store().custom_key_store_id(cks).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteCustomKeyStore".to_string()));
    }

    // Asymmetric key for Sign / Verify / GetPublicKey / GenerateDataKeyPair
    let asym_r = client
        .create_key()
        .key_spec(aws_sdk_kms::types::KeySpec::EccNistP256)
        .key_usage(aws_sdk_kms::types::KeyUsageType::SignVerify)
        .description("conformance asymmetric key")
        .send()
        .await;
    let asym_key_id = asym_r
        .as_ref()
        .ok()
        .and_then(|r| r.key_metadata.as_ref())
        .map(|m| m.key_id.clone());
    // We treat this CreateKey as informational (not added to results to avoid dup)
    let _ = asym_r;

    if let Some(ref akid) = asym_key_id {
        // Sign
        let msg_b64 = aws_sdk_kms::primitives::Blob::new(b"hello sign".to_vec());
        let sign_r = client
            .sign()
            .key_id(akid)
            .message(msg_b64)
            .signing_algorithm(aws_sdk_kms::types::SigningAlgorithmSpec::EcdsaSha256)
            .send()
            .await;
        let signature = sign_r
            .as_ref()
            .ok()
            .and_then(|r| r.signature.clone());
        results.push(chk!("Sign", sign_r, verbose));

        // Verify
        if let Some(sig) = signature {
            results.push(chk!(
                "Verify",
                client
                    .verify()
                    .key_id(akid)
                    .message(aws_sdk_kms::primitives::Blob::new(b"hello sign".to_vec()))
                    .signing_algorithm(aws_sdk_kms::types::SigningAlgorithmSpec::EcdsaSha256)
                    .signature(sig)
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("Verify".to_string()));
        }

        // GetPublicKey
        results.push(chk!(
            "GetPublicKey",
            client.get_public_key().key_id(akid).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("Sign".to_string()));
        results.push(OpResult::Skipped("Verify".to_string()));
        results.push(OpResult::Skipped("GetPublicKey".to_string()));
    }

    // GenerateDataKeyPair / GenerateDataKeyPairWithoutPlaintext — needs a symmetric key
    let sym_r = client
        .create_key()
        .description("conformance symmetric key for data key pair")
        .send()
        .await;
    let sym_key_id = sym_r
        .as_ref()
        .ok()
        .and_then(|r| r.key_metadata.as_ref())
        .map(|m| m.key_id.clone());
    let _ = sym_r;

    if let Some(ref skid) = sym_key_id {
        results.push(chk!(
            "GenerateDataKeyPair",
            client
                .generate_data_key_pair()
                .key_id(skid)
                .key_pair_spec(aws_sdk_kms::types::DataKeyPairSpec::EccNistP256)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "GenerateDataKeyPairWithoutPlaintext",
            client
                .generate_data_key_pair_without_plaintext()
                .key_id(skid)
                .key_pair_spec(aws_sdk_kms::types::DataKeyPairSpec::EccNistP256)
                .send()
                .await,
            verbose
        ));

        // UpdateKeyDescription
        results.push(chk!(
            "UpdateKeyDescription",
            client
                .update_key_description()
                .key_id(skid)
                .description("updated conformance description")
                .send()
                .await,
            verbose
        ));

        // GetKeyRotationStatus
        results.push(chk!(
            "GetKeyRotationStatus",
            client.get_key_rotation_status().key_id(skid).send().await,
            verbose
        ));

        // EnableKeyRotation
        results.push(chk!(
            "EnableKeyRotation",
            client.enable_key_rotation().key_id(skid).send().await,
            verbose
        ));

        // DisableKeyRotation
        results.push(chk!(
            "DisableKeyRotation",
            client.disable_key_rotation().key_id(skid).send().await,
            verbose
        ));

        // CreateGrant
        let grant_r = client
            .create_grant()
            .key_id(skid)
            .grantee_principal("arn:aws:iam::000000000000:role/ConformanceGrantee")
            .operations(aws_sdk_kms::types::GrantOperation::Encrypt)
            .send()
            .await;
        let grant_id = grant_r.as_ref().ok().and_then(|r| r.grant_id.clone());
        results.push(chk!("CreateGrant", grant_r, verbose));

        // ListGrants
        results.push(chk!(
            "ListGrants",
            client.list_grants().key_id(skid).send().await,
            verbose
        ));

        if let Some(ref gid) = grant_id {
            // RevokeGrant
            results.push(chk!(
                "RevokeGrant",
                client.revoke_grant().key_id(skid).grant_id(gid).send().await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("RevokeGrant".to_string()));
        }

        // RetireGrant — create a second grant to retire
        let grant2_r = client
            .create_grant()
            .key_id(skid)
            .grantee_principal("arn:aws:iam::000000000000:role/ConformanceGrantee")
            .operations(aws_sdk_kms::types::GrantOperation::Decrypt)
            .send()
            .await;
        let grant2_token = grant2_r.as_ref().ok().and_then(|r| r.grant_token.clone());
        let _ = grant2_r;

        if let Some(ref tok) = grant2_token {
            results.push(chk!(
                "RetireGrant",
                client.retire_grant().grant_token(tok).send().await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("RetireGrant".to_string()));
        }

        // GetKeyPolicy
        results.push(chk!(
            "GetKeyPolicy",
            client.get_key_policy().key_id(skid).policy_name("default").send().await,
            verbose
        ));

        // PutKeyPolicy
        let policy_doc = r#"{"Version":"2012-10-17","Statement":[{"Sid":"Enable IAM User Permissions","Effect":"Allow","Principal":{"AWS":"arn:aws:iam::000000000000:root"},"Action":"kms:*","Resource":"*"}]}"#;
        results.push(chk!(
            "PutKeyPolicy",
            client
                .put_key_policy()
                .key_id(skid)
                .policy_name("default")
                .policy(policy_doc)
                .send()
                .await,
            verbose
        ));

        // ListKeyPolicies
        results.push(chk!(
            "ListKeyPolicies",
            client.list_key_policies().key_id(skid).send().await,
            verbose
        ));

        // TagResource (KMS)
        results.push(chk!(
            "TagResource",
            client
                .tag_resource()
                .key_id(skid)
                .tags(
                    aws_sdk_kms::types::Tag::builder()
                        .tag_key("env")
                        .tag_value("conformance")
                        .build()
                        .unwrap(),
                )
                .send()
                .await,
            verbose
        ));

        // ListResourceTags
        results.push(chk!(
            "ListResourceTags",
            client.list_resource_tags().key_id(skid).send().await,
            verbose
        ));

        // UntagResource (KMS)
        results.push(chk!(
            "UntagResource",
            client
                .untag_resource()
                .key_id(skid)
                .tag_keys("env")
                .send()
                .await,
            verbose
        ));

        // RotateKeyOnDemand
        results.push(chk!(
            "RotateKeyOnDemand",
            client.rotate_key_on_demand().key_id(skid).send().await,
            verbose
        ));

        // ListKeyRotations
        results.push(chk!(
            "ListKeyRotations",
            client.list_key_rotations().key_id(skid).send().await,
            verbose
        ));

        // GenerateMac
        let mac_r = client
            .generate_mac()
            .key_id(skid)
            .message(aws_sdk_kms::primitives::Blob::new(b"mac me".to_vec()))
            .mac_algorithm(aws_sdk_kms::types::MacAlgorithmSpec::HmacSha256)
            .send()
            .await;
        let mac_value = mac_r.as_ref().ok().and_then(|r| r.mac.clone());
        results.push(chk!("GenerateMac", mac_r, verbose));

        // VerifyMac
        if let Some(mac) = mac_value {
            results.push(chk!(
                "VerifyMac",
                client
                    .verify_mac()
                    .key_id(skid)
                    .message(aws_sdk_kms::primitives::Blob::new(b"mac me".to_vec()))
                    .mac_algorithm(aws_sdk_kms::types::MacAlgorithmSpec::HmacSha256)
                    .mac(mac)
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("VerifyMac".to_string()));
        }
    } else {
        for op in &[
            "GenerateDataKeyPair",
            "GenerateDataKeyPairWithoutPlaintext",
            "UpdateKeyDescription",
            "GetKeyRotationStatus",
            "EnableKeyRotation",
            "DisableKeyRotation",
            "CreateGrant",
            "ListGrants",
            "RevokeGrant",
            "RetireGrant",
            "GetKeyPolicy",
            "PutKeyPolicy",
            "ListKeyPolicies",
            "TagResource",
            "ListResourceTags",
            "UntagResource",
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

        // TagResource (Secrets Manager)
        results.push(chk!(
            "TagResource",
            client
                .tag_resource()
                .secret_id(arn)
                .tags(
                    aws_sdk_secretsmanager::types::Tag::builder()
                        .key("env")
                        .value("conformance")
                        .build(),
                )
                .send()
                .await,
            verbose
        ));

        // UntagResource (Secrets Manager)
        results.push(chk!(
            "UntagResource",
            client
                .untag_resource()
                .secret_id(arn)
                .tag_keys("env")
                .send()
                .await,
            verbose
        ));

        // DeleteSecret (soft delete first, then restore)
        results.push(chk!(
            "DeleteSecret",
            client
                .delete_secret()
                .secret_id(arn)
                .recovery_window_in_days(7)
                .send()
                .await,
            verbose
        ));

        // RestoreSecret (restore the soft-deleted secret)
        results.push(chk!(
            "RestoreSecret",
            client.restore_secret().secret_id(arn).send().await,
            verbose
        ));

        // RotateSecret
        results.push(chk!(
            "RotateSecret",
            client
                .rotate_secret()
                .secret_id(arn)
                .rotation_rules(
                    aws_sdk_secretsmanager::types::RotationRulesType::builder()
                        .automatically_after_days(30)
                        .build(),
                )
                .send()
                .await,
            verbose
        ));

        // ValidateResourcePolicy
        let policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"AWS":"arn:aws:iam::000000000000:root"},"Action":"secretsmanager:GetSecretValue","Resource":"*"}]}"#;
        results.push(chk!(
            "ValidateResourcePolicy",
            client
                .validate_resource_policy()
                .secret_id(arn)
                .resource_policy(policy)
                .send()
                .await,
            verbose
        ));

        // ListSecretVersionIds
        results.push(chk!(
            "ListSecretVersionIds",
            client.list_secret_version_ids().secret_id(arn).send().await,
            verbose
        ));

        // BatchGetSecretValue
        results.push(chk!(
            "BatchGetSecretValue",
            client
                .batch_get_secret_value()
                .secret_id_list(arn)
                .send()
                .await,
            verbose
        ));

        // PutResourcePolicy
        let res_policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"AWS":"arn:aws:iam::000000000000:root"},"Action":"secretsmanager:GetSecretValue","Resource":"*"}]}"#;
        results.push(chk!(
            "PutResourcePolicy",
            client
                .put_resource_policy()
                .secret_id(arn)
                .resource_policy(res_policy)
                .send()
                .await,
            verbose
        ));

        // GetResourcePolicy
        results.push(chk!(
            "GetResourcePolicy",
            client.get_resource_policy().secret_id(arn).send().await,
            verbose
        ));

        // DeleteResourcePolicy
        results.push(chk!(
            "DeleteResourcePolicy",
            client.delete_resource_policy().secret_id(arn).send().await,
            verbose
        ));

        // Final hard delete for cleanup
        let _ = client
            .delete_secret()
            .secret_id(arn)
            .force_delete_without_recovery(true)
            .send()
            .await;
    } else {
        for op in &[
            "GetSecretValue",
            "DescribeSecret",
            "PutSecretValue",
            "UpdateSecret",
            "TagResource",
            "UntagResource",
            "DeleteSecret",
            "RestoreSecret",
            "RotateSecret",
            "ValidateResourcePolicy",
            "ListSecretVersionIds",
            "BatchGetSecretValue",
            "PutResourcePolicy",
            "GetResourcePolicy",
            "DeleteResourcePolicy",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    // GetRandomPassword (no secret needed)
    results.push(chk!(
        "GetRandomPassword",
        client.get_random_password().send().await,
        verbose
    ));

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

    // PutParameter (second one for path-based tests)
    let _ = client
        .put_parameter()
        .name("/conformance/param2")
        .value("value2")
        .r#type(ParameterType::String)
        .send()
        .await;

    // GetParametersByPath
    results.push(chk!(
        "GetParametersByPath",
        client
            .get_parameters_by_path()
            .path("/conformance")
            .send()
            .await,
        verbose
    ));

    // GetParameterHistory
    results.push(chk!(
        "GetParameterHistory",
        client
            .get_parameter_history()
            .name("/conformance/param")
            .send()
            .await,
        verbose
    ));

    // AddTagsToResource (SSM)
    results.push(chk!(
        "AddTagsToResource",
        client
            .add_tags_to_resource()
            .resource_type(aws_sdk_ssm::types::ResourceTypeForTagging::Parameter)
            .resource_id("/conformance/param")
            .tags(
                aws_sdk_ssm::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTagsForResource (SSM)
    results.push(chk!(
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_type(aws_sdk_ssm::types::ResourceTypeForTagging::Parameter)
            .resource_id("/conformance/param")
            .send()
            .await,
        verbose
    ));

    // RemoveTagsFromResource (SSM)
    results.push(chk!(
        "RemoveTagsFromResource",
        client
            .remove_tags_from_resource()
            .resource_type(aws_sdk_ssm::types::ResourceTypeForTagging::Parameter)
            .resource_id("/conformance/param")
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // LabelParameterVersion
    results.push(chk!(
        "LabelParameterVersion",
        client
            .label_parameter_version()
            .name("/conformance/param")
            .labels("conformance-label")
            .send()
            .await,
        verbose
    ));

    // SendCommand
    let send_cmd_r = client
        .send_command()
        .document_name("AWS-RunShellScript")
        .instance_ids("i-0000000000000000")
        .parameters("commands", vec!["echo hello".to_string()])
        .send()
        .await;
    let command_id = send_cmd_r
        .as_ref()
        .ok()
        .and_then(|r| r.command.as_ref())
        .and_then(|c| c.command_id.clone());
    results.push(chk!("SendCommand", send_cmd_r, verbose));

    // ListCommands
    results.push(chk!(
        "ListCommands",
        client.list_commands().send().await,
        verbose
    ));

    // GetCommandInvocation (expect service error — command on non-existent instance)
    if let Some(ref cid) = command_id {
        results.push(chk!(
            "GetCommandInvocation",
            client
                .get_command_invocation()
                .command_id(cid)
                .instance_id("i-0000000000000000")
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetCommandInvocation".to_string()));
    }

    // PutInventory
    results.push(chk!(
        "PutInventory",
        client
            .put_inventory()
            .instance_id("i-0000000000000000")
            .items(
                aws_sdk_ssm::types::InventoryItem::builder()
                    .type_name("AWS:Application")
                    .schema_version("1.1")
                    .capture_time("2024-01-01T00:00:00Z")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetInventory
    results.push(chk!(
        "GetInventory",
        client.get_inventory().send().await,
        verbose
    ));

    // CreateDocument
    let create_doc_r = client
        .create_document()
        .name("ConformanceDocument")
        .content(r#"{"schemaVersion":"2.2","description":"Conformance test doc","mainSteps":[]}"#)
        .document_type(aws_sdk_ssm::types::DocumentType::Command)
        .send()
        .await;
    results.push(chk!("CreateDocument", create_doc_r, verbose));

    // GetDocument
    results.push(chk!(
        "GetDocument",
        client
            .get_document()
            .name("ConformanceDocument")
            .send()
            .await,
        verbose
    ));

    // DescribeDocument
    results.push(chk!(
        "DescribeDocument",
        client
            .describe_document()
            .name("ConformanceDocument")
            .send()
            .await,
        verbose
    ));

    // ListDocuments
    results.push(chk!(
        "ListDocuments",
        client.list_documents().send().await,
        verbose
    ));

    // CreateAssociation
    let create_assoc_r = client
        .create_association()
        .name("ConformanceDocument")
        .instance_id("i-0000000000000000")
        .send()
        .await;
    let association_id = create_assoc_r
        .as_ref()
        .ok()
        .and_then(|r| r.association_description.as_ref())
        .and_then(|d| d.association_id.clone());
    results.push(chk!("CreateAssociation", create_assoc_r, verbose));

    // DescribeAssociation
    if let Some(ref aid) = association_id {
        results.push(chk!(
            "DescribeAssociation",
            client
                .describe_association()
                .association_id(aid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DescribeAssociation".to_string()));
    }

    // ListAssociations
    results.push(chk!(
        "ListAssociations",
        client.list_associations().send().await,
        verbose
    ));

    // DeleteAssociation
    if let Some(ref aid) = association_id {
        results.push(chk!(
            "DeleteAssociation",
            client
                .delete_association()
                .association_id(aid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteAssociation".to_string()));
    }

    // CreateMaintenanceWindow
    let create_mw_r = client
        .create_maintenance_window()
        .name("ConformanceMW")
        .schedule("cron(0 0 * * ? *)")
        .duration(1)
        .cutoff(0)
        .allow_unassociated_targets(false)
        .send()
        .await;
    let window_id = create_mw_r
        .as_ref()
        .ok()
        .and_then(|r| r.window_id.clone());
    results.push(chk!("CreateMaintenanceWindow", create_mw_r, verbose));

    // DescribeMaintenanceWindows
    results.push(chk!(
        "DescribeMaintenanceWindows",
        client.describe_maintenance_windows().send().await,
        verbose
    ));

    // DeleteMaintenanceWindow
    if let Some(ref wid) = window_id {
        results.push(chk!(
            "DeleteMaintenanceWindow",
            client
                .delete_maintenance_window()
                .window_id(wid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteMaintenanceWindow".to_string()));
    }

    // CreateOpsItem
    let create_ops_r = client
        .create_ops_item()
        .title("Conformance OpsItem")
        .description("Created by conformance test")
        .source("conformance")
        .send()
        .await;
    let ops_item_id = create_ops_r
        .as_ref()
        .ok()
        .and_then(|r| r.ops_item_id.clone());
    results.push(chk!("CreateOpsItem", create_ops_r, verbose));

    // GetOpsItem
    if let Some(ref oid) = ops_item_id {
        results.push(chk!(
            "GetOpsItem",
            client.get_ops_item().ops_item_id(oid).send().await,
            verbose
        ));

        // UpdateOpsItem
        results.push(chk!(
            "UpdateOpsItem",
            client
                .update_ops_item()
                .ops_item_id(oid)
                .description("Updated by conformance test")
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetOpsItem".to_string()));
        results.push(OpResult::Skipped("UpdateOpsItem".to_string()));
    }

    // DescribeOpsItems
    results.push(chk!(
        "DescribeOpsItems",
        client.describe_ops_items().send().await,
        verbose
    ));

    // CreatePatchBaseline
    let create_pb_r = client
        .create_patch_baseline()
        .name("ConformancePatchBaseline")
        .operating_system(aws_sdk_ssm::types::OperatingSystem::Windows)
        .description("Conformance test patch baseline")
        .send()
        .await;
    let baseline_id = create_pb_r.as_ref().ok().and_then(|r| r.baseline_id.clone());
    results.push(chk!("CreatePatchBaseline", create_pb_r, verbose));

    // DescribePatchBaselines
    results.push(chk!(
        "DescribePatchBaselines",
        client.describe_patch_baselines().send().await,
        verbose
    ));

    // GetPatchBaseline
    if let Some(ref bid) = baseline_id {
        results.push(chk!(
            "GetPatchBaseline",
            client.get_patch_baseline().baseline_id(bid).send().await,
            verbose
        ));

        // DeletePatchBaseline
        results.push(chk!(
            "DeletePatchBaseline",
            client.delete_patch_baseline().baseline_id(bid).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetPatchBaseline".to_string()));
        results.push(OpResult::Skipped("DeletePatchBaseline".to_string()));
    }

    // StartAutomationExecution
    let start_auto_r = client
        .start_automation_execution()
        .document_name("AWS-RunShellScript")
        .send()
        .await;
    let auto_exec_id = start_auto_r
        .as_ref()
        .ok()
        .and_then(|r| r.automation_execution_id.clone());
    results.push(chk!("StartAutomationExecution", start_auto_r, verbose));

    // GetAutomationExecution
    if let Some(ref aid) = auto_exec_id {
        results.push(chk!(
            "GetAutomationExecution",
            client
                .get_automation_execution()
                .automation_execution_id(aid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetAutomationExecution".to_string()));
    }

    // DescribeAutomationExecutions
    results.push(chk!(
        "DescribeAutomationExecutions",
        client.describe_automation_executions().send().await,
        verbose
    ));

    // StartSession
    let start_sess_r = client
        .start_session()
        .target("i-0000000000000000")
        .send()
        .await;
    let session_id = start_sess_r.as_ref().ok().and_then(|r| r.session_id.clone());
    results.push(chk!("StartSession", start_sess_r, verbose));

    // DescribeSessions
    results.push(chk!(
        "DescribeSessions",
        client
            .describe_sessions()
            .state(aws_sdk_ssm::types::SessionState::Active)
            .send()
            .await,
        verbose
    ));

    // TerminateSession
    if let Some(ref sid) = session_id {
        results.push(chk!(
            "TerminateSession",
            client.terminate_session().session_id(sid).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("TerminateSession".to_string()));
    }

    // DeleteDocument (cleanup)
    results.push(chk!(
        "DeleteDocument",
        client
            .delete_document()
            .name("ConformanceDocument")
            .send()
            .await,
        verbose
    ));

    // DeleteParameters (batch delete)
    results.push(chk!(
        "DeleteParameters",
        client
            .delete_parameters()
            .names("/conformance/param")
            .names("/conformance/param2")
            .send()
            .await,
        verbose
    ));

    // DeleteParameter (may already be deleted by DeleteParameters — will get service error = pass)
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

    // UpdateFunctionCode
    let zip_bytes2 = minimal_lambda_zip();
    results.push(chk!(
        "UpdateFunctionCode",
        client
            .update_function_code()
            .function_name("conformance-fn")
            .zip_file(Blob::new(zip_bytes2))
            .send()
            .await,
        verbose
    ));

    // PublishVersion
    results.push(chk!(
        "PublishVersion",
        client
            .publish_version()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // ListVersionsByFunction
    results.push(chk!(
        "ListVersionsByFunction",
        client
            .list_versions_by_function()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // CreateAlias
    let alias_r = client
        .create_alias()
        .function_name("conformance-fn")
        .name("conformance-alias")
        .function_version("$LATEST")
        .send()
        .await;
    results.push(chk!("CreateAlias", alias_r, verbose));

    // GetAlias
    results.push(chk!(
        "GetAlias",
        client
            .get_alias()
            .function_name("conformance-fn")
            .name("conformance-alias")
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

    // DeleteAlias
    results.push(chk!(
        "DeleteAlias",
        client
            .delete_alias()
            .function_name("conformance-fn")
            .name("conformance-alias")
            .send()
            .await,
        verbose
    ));

    // CreateEventSourceMapping
    let esm_r = client
        .create_event_source_mapping()
        .function_name("conformance-fn")
        .event_source_arn("arn:aws:kinesis:us-east-1:000000000000:stream/conformance-stream")
        .starting_position(aws_sdk_lambda::types::EventSourcePosition::TrimHorizon)
        .send()
        .await;
    let esm_uuid = esm_r
        .as_ref()
        .ok()
        .and_then(|r| r.uuid.clone());
    results.push(chk!("CreateEventSourceMapping", esm_r, verbose));

    // ListEventSourceMappings
    results.push(chk!(
        "ListEventSourceMappings",
        client
            .list_event_source_mappings()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // GetEventSourceMapping
    if let Some(ref uuid) = esm_uuid {
        results.push(chk!(
            "GetEventSourceMapping",
            client.get_event_source_mapping().uuid(uuid).send().await,
            verbose
        ));

        // DeleteEventSourceMapping
        results.push(chk!(
            "DeleteEventSourceMapping",
            client.delete_event_source_mapping().uuid(uuid).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetEventSourceMapping".to_string()));
        results.push(OpResult::Skipped("DeleteEventSourceMapping".to_string()));
    }

    // PublishLayerVersion
    let layer_zip = minimal_lambda_zip();
    let layer_r = client
        .publish_layer_version()
        .layer_name("conformance-layer")
        .content(
            aws_sdk_lambda::types::LayerVersionContentInput::builder()
                .zip_file(Blob::new(layer_zip))
                .build(),
        )
        .send()
        .await;
    results.push(chk!("PublishLayerVersion", layer_r, verbose));

    // ListLayers
    results.push(chk!(
        "ListLayers",
        client.list_layers().send().await,
        verbose
    ));

    // ListLayerVersions
    results.push(chk!(
        "ListLayerVersions",
        client
            .list_layer_versions()
            .layer_name("conformance-layer")
            .send()
            .await,
        verbose
    ));

    // DeleteLayerVersion
    results.push(chk!(
        "DeleteLayerVersion",
        client
            .delete_layer_version()
            .layer_name("conformance-layer")
            .version_number(1)
            .send()
            .await,
        verbose
    ));

    // TagResource (Lambda)
    let fn_arn = format!("arn:aws:lambda:us-east-1:000000000000:function:conformance-fn");
    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource(fn_arn.clone())
            .tags("env", "conformance")
            .send()
            .await,
        verbose
    ));

    // ListTags
    results.push(chk!(
        "ListTags",
        client.list_tags().resource(fn_arn.clone()).send().await,
        verbose
    ));

    // UntagResource (Lambda)
    results.push(chk!(
        "UntagResource",
        client
            .untag_resource()
            .resource(fn_arn.clone())
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // AddPermission
    results.push(chk!(
        "AddPermission",
        client
            .add_permission()
            .function_name("conformance-fn")
            .statement_id("conformance-stmt")
            .action("lambda:InvokeFunction")
            .principal("apigateway.amazonaws.com")
            .send()
            .await,
        verbose
    ));

    // GetPolicy
    results.push(chk!(
        "GetPolicy",
        client
            .get_policy()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // RemovePermission
    results.push(chk!(
        "RemovePermission",
        client
            .remove_permission()
            .function_name("conformance-fn")
            .statement_id("conformance-stmt")
            .send()
            .await,
        verbose
    ));

    // GetAccountSettings
    results.push(chk!(
        "GetAccountSettings",
        client.get_account_settings().send().await,
        verbose
    ));

    // Invoke
    results.push(chk!(
        "Invoke",
        client
            .invoke()
            .function_name("conformance-fn")
            .payload(Blob::new(br#"{"hello":"world"}"#.to_vec()))
            .send()
            .await,
        verbose
    ));

    // CreateFunctionUrlConfig
    results.push(chk!(
        "CreateFunctionUrlConfig",
        client
            .create_function_url_config()
            .function_name("conformance-fn")
            .auth_type(aws_sdk_lambda::types::FunctionUrlAuthType::None)
            .send()
            .await,
        verbose
    ));

    // GetFunctionUrlConfig
    results.push(chk!(
        "GetFunctionUrlConfig",
        client
            .get_function_url_config()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // ListFunctionUrlConfigs
    results.push(chk!(
        "ListFunctionUrlConfigs",
        client
            .list_function_url_configs()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // DeleteFunctionUrlConfig
    results.push(chk!(
        "DeleteFunctionUrlConfig",
        client
            .delete_function_url_config()
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

    // PutRecords (batch)
    results.push(chk!(
        "PutRecords",
        client
            .put_records()
            .stream_name("conformance-stream")
            .records(
                aws_sdk_kinesis::types::PutRecordsRequestEntry::builder()
                    .partition_key("pk-batch")
                    .data(aws_sdk_kinesis::primitives::Blob::new(b"batch record 1".to_vec()))
                    .build()
                    .unwrap(),
            )
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

    // RemoveTagsFromStream
    results.push(chk!(
        "RemoveTagsFromStream",
        client
            .remove_tags_from_stream()
            .stream_name("conformance-stream")
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // IncreaseStreamRetentionPeriod
    results.push(chk!(
        "IncreaseStreamRetentionPeriod",
        client
            .increase_stream_retention_period()
            .stream_name("conformance-stream")
            .retention_period_hours(48)
            .send()
            .await,
        verbose
    ));

    // DecreaseStreamRetentionPeriod (back to default 24h)
    results.push(chk!(
        "DecreaseStreamRetentionPeriod",
        client
            .decrease_stream_retention_period()
            .stream_name("conformance-stream")
            .retention_period_hours(24)
            .send()
            .await,
        verbose
    ));

    // MergeShards (requires 2 shards — will get service error = pass)
    if let Some(ref sid) = shard_id {
        results.push(chk!(
            "MergeShards",
            client
                .merge_shards()
                .stream_name("conformance-stream")
                .shard_to_merge(sid)
                .adjacent_shard_to_merge(sid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("MergeShards".to_string()));
    }

    // SplitShard (on the single shard)
    if let Some(ref sid) = shard_id {
        results.push(chk!(
            "SplitShard",
            client
                .split_shard()
                .stream_name("conformance-stream")
                .shard_to_split(sid)
                .new_starting_hash_key("170141183460469231731687303715884105728")
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("SplitShard".to_string()));
    }

    // RegisterStreamConsumer
    let stream_arn_r = client
        .describe_stream_summary()
        .stream_name("conformance-stream")
        .send()
        .await;
    let stream_arn = stream_arn_r
        .as_ref()
        .ok()
        .and_then(|r| r.stream_description_summary.as_ref())
        .map(|s| s.stream_arn.clone())
        .unwrap_or_else(|| "arn:aws:kinesis:us-east-1:000000000000:stream/conformance-stream".to_string());

    let consumer_r = client
        .register_stream_consumer()
        .stream_arn(&stream_arn)
        .consumer_name("conformance-consumer")
        .send()
        .await;
    let consumer_arn = consumer_r
        .as_ref()
        .ok()
        .and_then(|r| r.consumer.as_ref())
        .map(|c| c.consumer_arn.clone());
    results.push(chk!("RegisterStreamConsumer", consumer_r, verbose));

    // ListStreamConsumers
    results.push(chk!(
        "ListStreamConsumers",
        client
            .list_stream_consumers()
            .stream_arn(&stream_arn)
            .send()
            .await,
        verbose
    ));

    // DescribeStreamConsumer
    if let Some(ref carn) = consumer_arn {
        results.push(chk!(
            "DescribeStreamConsumer",
            client
                .describe_stream_consumer()
                .consumer_arn(carn)
                .send()
                .await,
            verbose
        ));

        // DeregisterStreamConsumer
        results.push(chk!(
            "DeregisterStreamConsumer",
            client
                .deregister_stream_consumer()
                .consumer_arn(carn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DescribeStreamConsumer".to_string()));
        results.push(OpResult::Skipped("DeregisterStreamConsumer".to_string()));
    }

    // EnableEnhancedMonitoring
    results.push(chk!(
        "EnableEnhancedMonitoring",
        client
            .enable_enhanced_monitoring()
            .stream_name("conformance-stream")
            .shard_level_metrics(aws_sdk_kinesis::types::MetricsName::IncomingBytes)
            .send()
            .await,
        verbose
    ));

    // DisableEnhancedMonitoring
    results.push(chk!(
        "DisableEnhancedMonitoring",
        client
            .disable_enhanced_monitoring()
            .stream_name("conformance-stream")
            .shard_level_metrics(aws_sdk_kinesis::types::MetricsName::IncomingBytes)
            .send()
            .await,
        verbose
    ));

    // StartStreamEncryption
    results.push(chk!(
        "StartStreamEncryption",
        client
            .start_stream_encryption()
            .stream_name("conformance-stream")
            .encryption_type(aws_sdk_kinesis::types::EncryptionType::Kms)
            .key_id("alias/aws/kinesis")
            .send()
            .await,
        verbose
    ));

    // StopStreamEncryption
    results.push(chk!(
        "StopStreamEncryption",
        client
            .stop_stream_encryption()
            .stream_name("conformance-stream")
            .encryption_type(aws_sdk_kinesis::types::EncryptionType::Kms)
            .key_id("alias/aws/kinesis")
            .send()
            .await,
        verbose
    ));

    // UpdateShardCount
    results.push(chk!(
        "UpdateShardCount",
        client
            .update_shard_count()
            .stream_name("conformance-stream")
            .target_shard_count(2)
            .scaling_type(aws_sdk_kinesis::types::ScalingType::UniformScaling)
            .send()
            .await,
        verbose
    ));

    // DescribeLimits
    results.push(chk!(
        "DescribeLimits",
        client.describe_limits().send().await,
        verbose
    ));

    // PutResourcePolicy
    results.push(chk!(
        "PutResourcePolicy",
        client
            .put_resource_policy()
            .resource_arn(&stream_arn)
            .policy(r#"{"Version":"2012-10-17","Statement":[]}"#)
            .send()
            .await,
        verbose
    ));

    // GetResourcePolicy
    results.push(chk!(
        "GetResourcePolicy",
        client
            .get_resource_policy()
            .resource_arn(&stream_arn)
            .send()
            .await,
        verbose
    ));

    // UpdateStreamMode
    results.push(chk!(
        "UpdateStreamMode",
        client
            .update_stream_mode()
            .stream_arn(&stream_arn)
            .stream_mode_details(
                aws_sdk_kinesis::types::StreamModeDetails::builder()
                    .stream_mode(aws_sdk_kinesis::types::StreamMode::OnDemand)
                    .build()
                    .unwrap()
            )
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

        // DescribeUserPoolClient
        if let Some(ref cid) = app_client_id {
            results.push(chk!(
                "DescribeUserPoolClient",
                client
                    .describe_user_pool_client()
                    .user_pool_id(pool_id)
                    .client_id(cid)
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("DescribeUserPoolClient".to_string()));
        }

        // CreateGroup (Cognito IDP)
        results.push(chk!(
            "CreateGroup",
            client
                .create_group()
                .user_pool_id(pool_id)
                .group_name("conformance-group")
                .send()
                .await,
            verbose
        ));

        // ListGroups (Cognito IDP)
        results.push(chk!(
            "ListGroups",
            client
                .list_groups()
                .user_pool_id(pool_id)
                .send()
                .await,
            verbose
        ));

        // AdminAddUserToGroup
        results.push(chk!(
            "AdminAddUserToGroup",
            client
                .admin_add_user_to_group()
                .user_pool_id(pool_id)
                .username("conformance-user")
                .group_name("conformance-group")
                .send()
                .await,
            verbose
        ));

        // AdminListGroupsForUser
        results.push(chk!(
            "AdminListGroupsForUser",
            client
                .admin_list_groups_for_user()
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

        // SignUp (needs client credentials)
        if let Some(ref cid) = app_client_id {
            results.push(chk!(
                "SignUp",
                client
                    .sign_up()
                    .client_id(cid)
                    .username("signup-user")
                    .password("Pass@word1!")
                    .send()
                    .await,
                verbose
            ));

            // ConfirmSignUp (auto-confirm in sim — may pass or need admin confirm)
            results.push(chk!(
                "ConfirmSignUp",
                client
                    .confirm_sign_up()
                    .client_id(cid)
                    .username("signup-user")
                    .confirmation_code("123456")
                    .send()
                    .await,
                verbose
            ));

            // InitiateAuth (USER_PASSWORD_AUTH)
            results.push(chk!(
                "InitiateAuth",
                client
                    .initiate_auth()
                    .client_id(cid)
                    .auth_flow(aws_sdk_cognitoidentityprovider::types::AuthFlowType::UserPasswordAuth)
                    .auth_parameters("USERNAME", "signup-user")
                    .auth_parameters("PASSWORD", "Pass@word1!")
                    .send()
                    .await,
                verbose
            ));

            // ForgotPassword
            results.push(chk!(
                "ForgotPassword",
                client
                    .forgot_password()
                    .client_id(cid)
                    .username("signup-user")
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("SignUp".to_string()));
            results.push(OpResult::Skipped("ConfirmSignUp".to_string()));
            results.push(OpResult::Skipped("InitiateAuth".to_string()));
            results.push(OpResult::Skipped("ForgotPassword".to_string()));
        }

        // UpdateUserPool
        results.push(chk!(
            "UpdateUserPool",
            client.update_user_pool().user_pool_id(pool_id).send().await,
            verbose
        ));

        // AdminEnableUser / AdminDisableUser
        // Re-create the user for enable/disable tests (was deleted above)
        let _ = client
            .admin_create_user()
            .user_pool_id(pool_id)
            .username("enable-test-user")
            .send()
            .await;

        results.push(chk!(
            "AdminDisableUser",
            client
                .admin_disable_user()
                .user_pool_id(pool_id)
                .username("enable-test-user")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "AdminEnableUser",
            client
                .admin_enable_user()
                .user_pool_id(pool_id)
                .username("enable-test-user")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "AdminResetUserPassword",
            client
                .admin_reset_user_password()
                .user_pool_id(pool_id)
                .username("enable-test-user")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "AdminSetUserMFAPreference",
            client
                .admin_set_user_mfa_preference()
                .user_pool_id(pool_id)
                .username("enable-test-user")
                .send()
                .await,
            verbose
        ));

        // Cleanup enable-test-user
        let _ = client
            .admin_delete_user()
            .user_pool_id(pool_id)
            .username("enable-test-user")
            .send()
            .await;

        // SetUserPoolMfaConfig / GetUserPoolMfaConfig
        results.push(chk!(
            "SetUserPoolMfaConfig",
            client
                .set_user_pool_mfa_config()
                .user_pool_id(pool_id)
                .mfa_configuration(
                    aws_sdk_cognitoidentityprovider::types::UserPoolMfaType::Optional,
                )
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "GetUserPoolMfaConfig",
            client
                .get_user_pool_mfa_config()
                .user_pool_id(pool_id)
                .send()
                .await,
            verbose
        ));

        // Group management
        results.push(chk!(
            "GetGroup",
            client
                .get_group()
                .user_pool_id(pool_id)
                .group_name("conformance-group")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "UpdateGroup",
            client
                .update_group()
                .user_pool_id(pool_id)
                .group_name("conformance-group")
                .description("updated description")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ListUsersInGroup",
            client
                .list_users_in_group()
                .user_pool_id(pool_id)
                .group_name("conformance-group")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "AdminRemoveUserFromGroup",
            client
                .admin_remove_user_from_group()
                .user_pool_id(pool_id)
                .username("signup-user")
                .group_name("conformance-group")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "DeleteGroup",
            client
                .delete_group()
                .user_pool_id(pool_id)
                .group_name("conformance-group")
                .send()
                .await,
            verbose
        ));

        // Identity Providers
        results.push(chk!(
            "CreateIdentityProvider",
            client
                .create_identity_provider()
                .user_pool_id(pool_id)
                .provider_name("conformance-oidc")
                .provider_type(
                    aws_sdk_cognitoidentityprovider::types::IdentityProviderTypeType::Oidc,
                )
                .provider_details("client_id", "test-client")
                .provider_details("client_secret", "test-secret")
                .provider_details("attributes_request_method", "GET")
                .provider_details(
                    "oidc_issuer",
                    "https://accounts.example.com",
                )
                .provider_details("authorize_scopes", "openid")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ListIdentityProviders",
            client
                .list_identity_providers()
                .user_pool_id(pool_id)
                .send()
                .await,
            verbose
        ));

        if let Some(ref cid) = app_client_id {
            results.push(chk!(
                "AddUserPoolClientSecret",
                client
                    .add_user_pool_client_secret()
                    .user_pool_id(pool_id)
                    .client_id(cid)
                    .send()
                    .await,
                verbose
            ));

            results.push(chk!(
                "ListUserPoolClientSecrets",
                client
                    .list_user_pool_client_secrets()
                    .user_pool_id(pool_id)
                    .client_id(cid)
                    .send()
                    .await,
                verbose
            ));
        }

        results.push(chk!(
            "ListTerms",
            client.list_terms().user_pool_id(pool_id).send().await,
            verbose
        ));

        results.push(chk!(
            "DeleteIdentityProvider",
            client
                .delete_identity_provider()
                .user_pool_id(pool_id)
                .provider_name("conformance-oidc")
                .send()
                .await,
            verbose
        ));

        // Resource Servers
        results.push(chk!(
            "CreateResourceServer",
            client
                .create_resource_server()
                .user_pool_id(pool_id)
                .identifier("https://api.conformance.test")
                .name("conformance-resource-server")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ListResourceServers",
            client
                .list_resource_servers()
                .user_pool_id(pool_id)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "DeleteResourceServer",
            client
                .delete_resource_server()
                .user_pool_id(pool_id)
                .identifier("https://api.conformance.test")
                .send()
                .await,
            verbose
        ));

        // UpdateUserPoolClient
        if let Some(ref cid) = app_client_id {
            results.push(chk!(
                "UpdateUserPoolClient",
                client
                    .update_user_pool_client()
                    .user_pool_id(pool_id)
                    .client_id(cid)
                    .client_name("conformance-client-updated")
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("UpdateUserPoolClient".to_string()));
        }

        // Tags
        let pool_arn = format!(
            "arn:aws:cognito-idp:us-east-1:000000000000:userpool/{}",
            pool_id
        );
        results.push(chk!(
            "TagResource",
            client
                .tag_resource()
                .resource_arn(&pool_arn)
                .tags("env", "conformance")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ListTagsForResource",
            client
                .list_tags_for_resource()
                .resource_arn(&pool_arn)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "UntagResource",
            client
                .untag_resource()
                .resource_arn(&pool_arn)
                .tag_keys("env")
                .send()
                .await,
            verbose
        ));

        // DescribeUserPoolDomain (for a domain that doesn't exist — should return empty)
        results.push(chk!(
            "DescribeUserPoolDomain",
            client
                .describe_user_pool_domain()
                .domain("nonexistent-conformance-domain")
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
            "DescribeUserPoolClient",
            "AdminCreateUser",
            "ListUsers",
            "AdminGetUser",
            "CreateGroup",
            "ListGroups",
            "AdminAddUserToGroup",
            "AdminListGroupsForUser",
            "AdminDeleteUser",
            "SignUp",
            "ConfirmSignUp",
            "InitiateAuth",
            "ForgotPassword",
            "UpdateUserPool",
            "AdminDisableUser",
            "AdminEnableUser",
            "AdminResetUserPassword",
            "AdminSetUserMFAPreference",
            "SetUserPoolMfaConfig",
            "GetUserPoolMfaConfig",
            "GetGroup",
            "UpdateGroup",
            "ListUsersInGroup",
            "AdminRemoveUserFromGroup",
            "DeleteGroup",
            "CreateIdentityProvider",
            "ListIdentityProviders",
            "DeleteIdentityProvider",
            "CreateResourceServer",
            "ListResourceServers",
            "DeleteResourceServer",
            "UpdateUserPoolClient",
            "TagResource",
            "ListTagsForResource",
            "UntagResource",
            "DescribeUserPoolDomain",
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

        // UpdateIdentityPool
        results.push(chk!(
            "UpdateIdentityPool",
            client
                .update_identity_pool()
                .identity_pool_id(pid)
                .identity_pool_name("conformance-identity-pool-updated")
                .allow_unauthenticated_identities(false)
                .send()
                .await,
            verbose
        ));

        // GetId
        let get_id_r = client
            .get_id()
            .account_id("000000000000")
            .identity_pool_id(pid)
            .send()
            .await;
        let identity_id = get_id_r
            .as_ref()
            .ok()
            .and_then(|r| r.identity_id.clone());
        results.push(chk!("GetId", get_id_r, verbose));

        // GetCredentialsForIdentity
        if let Some(ref iid) = identity_id {
            results.push(chk!(
                "GetCredentialsForIdentity",
                client
                    .get_credentials_for_identity()
                    .identity_id(iid)
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("GetCredentialsForIdentity".to_string()));
        }

        // SetIdentityPoolRoles
        results.push(chk!(
            "SetIdentityPoolRoles",
            client
                .set_identity_pool_roles()
                .identity_pool_id(pid)
                .roles(
                    "authenticated",
                    "arn:aws:iam::000000000000:role/conformance-cognito-role",
                )
                .send()
                .await,
            verbose
        ));

        // GetIdentityPoolRoles
        results.push(chk!(
            "GetIdentityPoolRoles",
            client
                .get_identity_pool_roles()
                .identity_pool_id(pid)
                .send()
                .await,
            verbose
        ));

        // ListIdentities
        results.push(chk!(
            "ListIdentities",
            client
                .list_identities()
                .identity_pool_id(pid)
                .max_results(10)
                .send()
                .await,
            verbose
        ));

        // SetPrincipalTagAttributeMap
        results.push(chk!(
            "SetPrincipalTagAttributeMap",
            client
                .set_principal_tag_attribute_map()
                .identity_pool_id(pid)
                .identity_provider_name("graph.facebook.com")
                .use_defaults(true)
                .send()
                .await,
            verbose
        ));

        // GetPrincipalTagAttributeMap
        results.push(chk!(
            "GetPrincipalTagAttributeMap",
            client
                .get_principal_tag_attribute_map()
                .identity_pool_id(pid)
                .identity_provider_name("graph.facebook.com")
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
        for op in &[
            "DescribeIdentityPool",
            "UpdateIdentityPool",
            "GetId",
            "GetCredentialsForIdentity",
            "SetIdentityPoolRoles",
            "GetIdentityPoolRoles",
            "DeleteIdentityPool",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
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
    let td_r = client
        .register_task_definition()
        .family("conformance-task")
        .container_definitions(
            aws_sdk_ecs::types::ContainerDefinition::builder()
                .name("conformance-container")
                .image("public.ecr.aws/nginx/nginx:latest")
                .build(),
        )
        .send()
        .await;
    let task_def_arn = td_r
        .as_ref()
        .ok()
        .and_then(|r| r.task_definition.as_ref())
        .and_then(|td| td.task_definition_arn.clone());
    results.push(chk!("RegisterTaskDefinition", td_r, verbose));

    // ListTaskDefinitions
    results.push(chk!(
        "ListTaskDefinitions",
        client.list_task_definitions().send().await,
        verbose
    ));

    // ListTaskDefinitionFamilies
    results.push(chk!(
        "ListTaskDefinitionFamilies",
        client.list_task_definition_families().send().await,
        verbose
    ));

    // DescribeTaskDefinition
    results.push(chk!(
        "DescribeTaskDefinition",
        client
            .describe_task_definition()
            .task_definition("conformance-task")
            .send()
            .await,
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

    // UpdateService
    results.push(chk!(
        "UpdateService",
        client
            .update_service()
            .cluster("conformance-cluster")
            .service("conformance-service")
            .desired_count(0)
            .send()
            .await,
        verbose
    ));

    // RunTask
    let run_task_r = client
        .run_task()
        .cluster("conformance-cluster")
        .task_definition("conformance-task")
        .send()
        .await;
    let task_arn = run_task_r
        .as_ref()
        .ok()
        .and_then(|r| r.tasks.as_ref())
        .and_then(|t| t.first())
        .and_then(|t| t.task_arn.clone());
    results.push(chk!("RunTask", run_task_r, verbose));

    // ListTasks
    results.push(chk!(
        "ListTasks",
        client
            .list_tasks()
            .cluster("conformance-cluster")
            .send()
            .await,
        verbose
    ));

    // DescribeTasks
    if let Some(ref tarn) = task_arn {
        results.push(chk!(
            "DescribeTasks",
            client
                .describe_tasks()
                .cluster("conformance-cluster")
                .tasks(tarn)
                .send()
                .await,
            verbose
        ));

        // StopTask
        results.push(chk!(
            "StopTask",
            client
                .stop_task()
                .cluster("conformance-cluster")
                .task(tarn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DescribeTasks".to_string()));
        results.push(OpResult::Skipped("StopTask".to_string()));
    }

    // DeregisterTaskDefinition
    if let Some(ref tdarn) = task_def_arn {
        results.push(chk!(
            "DeregisterTaskDefinition",
            client
                .deregister_task_definition()
                .task_definition(tdarn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeregisterTaskDefinition".to_string()));
    }

    // TagResource (ECS)
    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource_arn(format!("arn:aws:ecs:us-east-1:000000000000:cluster/conformance-cluster"))
            .tags(
                aws_sdk_ecs::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTagsForResource (ECS)
    results.push(chk!(
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_arn(format!("arn:aws:ecs:us-east-1:000000000000:cluster/conformance-cluster"))
            .send()
            .await,
        verbose
    ));

    // UntagResource (ECS)
    results.push(chk!(
        "UntagResource",
        client
            .untag_resource()
            .resource_arn(format!("arn:aws:ecs:us-east-1:000000000000:cluster/conformance-cluster"))
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // PutClusterCapacityProviders
    results.push(chk!(
        "PutClusterCapacityProviders",
        client
            .put_cluster_capacity_providers()
            .cluster("conformance-cluster")
            .default_capacity_provider_strategy(
                aws_sdk_ecs::types::CapacityProviderStrategyItem::builder()
                    .capacity_provider("FARGATE")
                    .weight(1)
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // DescribeCapacityProviders
    results.push(chk!(
        "DescribeCapacityProviders",
        client
            .describe_capacity_providers()
            .send()
            .await,
        verbose
    ));

    // PutAccountSetting
    results.push(chk!(
        "PutAccountSetting",
        client
            .put_account_setting()
            .name(aws_sdk_ecs::types::SettingName::ContainerInsights)
            .value("enabled")
            .send()
            .await,
        verbose
    ));

    // ListAccountSettings
    results.push(chk!(
        "ListAccountSettings",
        client
            .list_account_settings()
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

    // DescribeContainerInstances
    results.push(chk!(
        "DescribeContainerInstances",
        client
            .describe_container_instances()
            .cluster("conformance-cluster")
            .container_instances("ci-stub")
            .send()
            .await,
        verbose
    ));

    // PutAttributes
    results.push(chk!(
        "PutAttributes",
        client
            .put_attributes()
            .cluster("conformance-cluster")
            .attributes(
                aws_sdk_ecs::types::Attribute::builder()
                    .name("env")
                    .value("conformance")
                    .target_type(aws_sdk_ecs::types::TargetType::ContainerInstance)
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // ListAttributes
    results.push(chk!(
        "ListAttributes",
        client
            .list_attributes()
            .cluster("conformance-cluster")
            .target_type(aws_sdk_ecs::types::TargetType::ContainerInstance)
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

    // DescribeImages
    results.push(chk!(
        "DescribeImages",
        client
            .describe_images()
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

    // TagResource (ECR)
    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource_arn("arn:aws:ecr:us-east-1:000000000000:repository/conformance-repo")
            .tags(
                aws_sdk_ecr::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTagsForResource (ECR)
    results.push(chk!(
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_arn("arn:aws:ecr:us-east-1:000000000000:repository/conformance-repo")
            .send()
            .await,
        verbose
    ));

    // UntagResource (ECR)
    results.push(chk!(
        "UntagResource",
        client
            .untag_resource()
            .resource_arn("arn:aws:ecr:us-east-1:000000000000:repository/conformance-repo")
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // PutImage (needs a manifest — use a minimal OCI manifest; may fail with schema error)
    let manifest = r#"{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{"mediaType":"application/vnd.docker.container.image.v1+json","size":7023,"digest":"sha256:b5b2b2c507a0944348e0303114d8d93aaaa081732b86451d9bce1f432a537bc7"},"layers":[]}"#;
    results.push(chk!(
        "PutImage",
        client
            .put_image()
            .repository_name("conformance-repo")
            .image_manifest(manifest)
            .image_tag("latest")
            .send()
            .await,
        verbose
    ));

    // BatchGetImage
    results.push(chk!(
        "BatchGetImage",
        client
            .batch_get_image()
            .repository_name("conformance-repo")
            .image_ids(
                aws_sdk_ecr::types::ImageIdentifier::builder()
                    .image_tag("latest")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // BatchDeleteImage
    results.push(chk!(
        "BatchDeleteImage",
        client
            .batch_delete_image()
            .repository_name("conformance-repo")
            .image_ids(
                aws_sdk_ecr::types::ImageIdentifier::builder()
                    .image_tag("latest")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // PutLifecyclePolicy
    results.push(chk!(
        "PutLifecyclePolicy",
        client
            .put_lifecycle_policy()
            .repository_name("conformance-repo")
            .lifecycle_policy_text(r#"{"rules":[{"rulePriority":1,"selection":{"tagStatus":"untagged","countType":"imageCountMoreThan","countNumber":5},"action":{"type":"expire"}}]}"#)
            .send()
            .await,
        verbose
    ));

    // GetLifecyclePolicy
    results.push(chk!(
        "GetLifecyclePolicy",
        client
            .get_lifecycle_policy()
            .repository_name("conformance-repo")
            .send()
            .await,
        verbose
    ));

    // DeleteLifecyclePolicy
    results.push(chk!(
        "DeleteLifecyclePolicy",
        client
            .delete_lifecycle_policy()
            .repository_name("conformance-repo")
            .send()
            .await,
        verbose
    ));

    // SetRepositoryPolicy
    results.push(chk!(
        "SetRepositoryPolicy",
        client
            .set_repository_policy()
            .repository_name("conformance-repo")
            .policy_text(r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":["ecr:GetDownloadUrlForLayer","ecr:BatchGetImage"]}]}"#)
            .send()
            .await,
        verbose
    ));

    // GetRepositoryPolicy
    results.push(chk!(
        "GetRepositoryPolicy",
        client
            .get_repository_policy()
            .repository_name("conformance-repo")
            .send()
            .await,
        verbose
    ));

    // DeleteRepositoryPolicy
    results.push(chk!(
        "DeleteRepositoryPolicy",
        client
            .delete_repository_policy()
            .repository_name("conformance-repo")
            .send()
            .await,
        verbose
    ));

    // StartImageScan
    results.push(chk!(
        "StartImageScan",
        client
            .start_image_scan()
            .repository_name("conformance-repo")
            .image_id(
                aws_sdk_ecr::types::ImageIdentifier::builder()
                    .image_tag("latest")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // DescribeImageScanFindings
    results.push(chk!(
        "DescribeImageScanFindings",
        client
            .describe_image_scan_findings()
            .repository_name("conformance-repo")
            .image_id(
                aws_sdk_ecr::types::ImageIdentifier::builder()
                    .image_tag("latest")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // BatchCheckLayerAvailability
    results.push(chk!(
        "BatchCheckLayerAvailability",
        client
            .batch_check_layer_availability()
            .repository_name("conformance-repo")
            .layer_digests("sha256:b5b2b2c507a0944348e0303114d8d93aaaa081732b86451d9bce1f432a537bc7")
            .send()
            .await,
        verbose
    ));

    // InitiateLayerUpload
    let layer_upload_r = client
        .initiate_layer_upload()
        .repository_name("conformance-repo")
        .send()
        .await;
    let layer_upload_id = layer_upload_r
        .as_ref()
        .ok()
        .and_then(|r| r.upload_id.clone());
    results.push(chk!("InitiateLayerUpload", layer_upload_r, verbose));

    if let Some(ref upload_id) = layer_upload_id {
        // UploadLayerPart
        results.push(chk!(
            "UploadLayerPart",
            client
                .upload_layer_part()
                .repository_name("conformance-repo")
                .upload_id(upload_id)
                .part_first_byte(0)
                .part_last_byte(3)
                .layer_part_blob(aws_sdk_ecr::primitives::Blob::new(b"test".to_vec()))
                .send()
                .await,
            verbose
        ));

        // CompleteLayerUpload
        results.push(chk!(
            "CompleteLayerUpload",
            client
                .complete_layer_upload()
                .repository_name("conformance-repo")
                .upload_id(upload_id)
                .layer_digests("sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08")
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("UploadLayerPart".to_string()));
        results.push(OpResult::Skipped("CompleteLayerUpload".to_string()));
    }

    // GetDownloadUrlForLayer
    results.push(chk!(
        "GetDownloadUrlForLayer",
        client
            .get_download_url_for_layer()
            .repository_name("conformance-repo")
            .layer_digest("sha256:b5b2b2c507a0944348e0303114d8d93aaaa081732b86451d9bce1f432a537bc7")
            .send()
            .await,
        verbose
    ));

    // PutImageTagMutability
    results.push(chk!(
        "PutImageTagMutability",
        client
            .put_image_tag_mutability()
            .repository_name("conformance-repo")
            .image_tag_mutability(aws_sdk_ecr::types::ImageTagMutability::Immutable)
            .send()
            .await,
        verbose
    ));

    // DescribeRegistry
    results.push(chk!(
        "DescribeRegistry",
        client.describe_registry().send().await,
        verbose
    ));

    // CreatePullThroughCacheRule
    results.push(chk!(
        "CreatePullThroughCacheRule",
        client
            .create_pull_through_cache_rule()
            .ecr_repository_prefix("dockerhub")
            .upstream_registry_url("registry-1.docker.io")
            .send()
            .await,
        verbose
    ));

    // DescribePullThroughCacheRules
    results.push(chk!(
        "DescribePullThroughCacheRules",
        client.describe_pull_through_cache_rules().send().await,
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

// ---------------------------------------------------------------------------
// EventBridge
// ---------------------------------------------------------------------------

async fn test_eventbridge(endpoint: &str, verbose: bool) -> Vec<OpResult> {
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

// ---------------------------------------------------------------------------
// Step Functions
// ---------------------------------------------------------------------------

async fn test_stepfunctions(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_sfn::Client::new(&config);
    let mut results = Vec::new();

    let asl = r#"{"Comment":"Conformance test","StartAt":"Pass","States":{"Pass":{"Type":"Pass","End":true}}}"#;

    // CreateStateMachine
    let sm_r = client
        .create_state_machine()
        .name("conformance-sm")
        .definition(asl)
        .role_arn("arn:aws:iam::000000000000:role/conformance-role")
        .send()
        .await;
    let sm_arn = sm_r
        .as_ref()
        .ok()
        .map(|r| r.state_machine_arn.clone())
        .unwrap_or_else(|| {
            "arn:aws:states:us-east-1:000000000000:stateMachine:conformance-sm".to_string()
        });
    results.push(chk!("CreateStateMachine", sm_r, verbose));

    // ListStateMachines
    results.push(chk!(
        "ListStateMachines",
        client.list_state_machines().send().await,
        verbose
    ));

    // DescribeStateMachine
    results.push(chk!(
        "DescribeStateMachine",
        client
            .describe_state_machine()
            .state_machine_arn(&sm_arn)
            .send()
            .await,
        verbose
    ));

    // StartExecution
    let exec_r = client
        .start_execution()
        .state_machine_arn(&sm_arn)
        .name("conformance-exec")
        .input(r#"{"key":"value"}"#)
        .send()
        .await;
    let exec_arn = exec_r
        .as_ref()
        .ok()
        .map(|r| r.execution_arn.clone());
    results.push(chk!("StartExecution", exec_r, verbose));

    // ListExecutions
    results.push(chk!(
        "ListExecutions",
        client
            .list_executions()
            .state_machine_arn(&sm_arn)
            .send()
            .await,
        verbose
    ));

    if let Some(ref earn) = exec_arn {
        // DescribeExecution
        results.push(chk!(
            "DescribeExecution",
            client.describe_execution().execution_arn(earn).send().await,
            verbose
        ));

        // GetExecutionHistory
        results.push(chk!(
            "GetExecutionHistory",
            client
                .get_execution_history()
                .execution_arn(earn)
                .send()
                .await,
            verbose
        ));

        // StopExecution
        results.push(chk!(
            "StopExecution",
            client.stop_execution().execution_arn(earn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DescribeExecution".to_string()));
        results.push(OpResult::Skipped("GetExecutionHistory".to_string()));
        results.push(OpResult::Skipped("StopExecution".to_string()));
    }

    // CreateActivity
    let act_r = client
        .create_activity()
        .name("conformance-activity")
        .send()
        .await;
    let act_arn = act_r
        .as_ref()
        .ok()
        .map(|r| r.activity_arn.clone());
    results.push(chk!("CreateActivity", act_r, verbose));

    // ListActivities
    results.push(chk!(
        "ListActivities",
        client.list_activities().send().await,
        verbose
    ));

    if let Some(ref aarn) = act_arn {
        // DescribeActivity
        results.push(chk!(
            "DescribeActivity",
            client.describe_activity().activity_arn(aarn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DescribeActivity".to_string()));
    }

    // TagResource (SFN)
    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource_arn(&sm_arn)
            .tags(
                aws_sdk_sfn::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTagsForResource (SFN)
    results.push(chk!(
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_arn(&sm_arn)
            .send()
            .await,
        verbose
    ));

    // UntagResource (SFN)
    results.push(chk!(
        "UntagResource",
        client
            .untag_resource()
            .resource_arn(&sm_arn)
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // DeleteActivity
    if let Some(ref aarn) = act_arn {
        results.push(chk!(
            "DeleteActivity",
            client.delete_activity().activity_arn(aarn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteActivity".to_string()));
    }

    // DeleteStateMachine
    results.push(chk!(
        "DeleteStateMachine",
        client
            .delete_state_machine()
            .state_machine_arn(&sm_arn)
            .send()
            .await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// CloudWatch Logs
// ---------------------------------------------------------------------------

async fn test_cloudwatch_logs(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_cloudwatchlogs::Client::new(&config);
    let mut results = Vec::new();

    // CreateLogGroup
    results.push(chk!(
        "CreateLogGroup",
        client
            .create_log_group()
            .log_group_name("/conformance/logs")
            .send()
            .await,
        verbose
    ));

    // DescribeLogGroups
    results.push(chk!(
        "DescribeLogGroups",
        client
            .describe_log_groups()
            .log_group_name_prefix("/conformance")
            .send()
            .await,
        verbose
    ));

    // CreateLogStream
    results.push(chk!(
        "CreateLogStream",
        client
            .create_log_stream()
            .log_group_name("/conformance/logs")
            .log_stream_name("conformance-stream")
            .send()
            .await,
        verbose
    ));

    // DescribeLogStreams
    results.push(chk!(
        "DescribeLogStreams",
        client
            .describe_log_streams()
            .log_group_name("/conformance/logs")
            .send()
            .await,
        verbose
    ));

    // PutLogEvents
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    results.push(chk!(
        "PutLogEvents",
        client
            .put_log_events()
            .log_group_name("/conformance/logs")
            .log_stream_name("conformance-stream")
            .log_events(
                aws_sdk_cloudwatchlogs::types::InputLogEvent::builder()
                    .timestamp(now_ms)
                    .message("conformance test log event")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetLogEvents
    results.push(chk!(
        "GetLogEvents",
        client
            .get_log_events()
            .log_group_name("/conformance/logs")
            .log_stream_name("conformance-stream")
            .send()
            .await,
        verbose
    ));

    // FilterLogEvents
    results.push(chk!(
        "FilterLogEvents",
        client
            .filter_log_events()
            .log_group_name("/conformance/logs")
            .send()
            .await,
        verbose
    ));

    // PutSubscriptionFilter
    results.push(chk!(
        "PutSubscriptionFilter",
        client
            .put_subscription_filter()
            .log_group_name("/conformance/logs")
            .filter_name("conformance-filter")
            .filter_pattern("")
            .destination_arn(
                "arn:aws:lambda:us-east-1:000000000000:function:conformance-fn",
            )
            .send()
            .await,
        verbose
    ));

    // DescribeSubscriptionFilters
    results.push(chk!(
        "DescribeSubscriptionFilters",
        client
            .describe_subscription_filters()
            .log_group_name("/conformance/logs")
            .send()
            .await,
        verbose
    ));

    // DeleteSubscriptionFilter
    results.push(chk!(
        "DeleteSubscriptionFilter",
        client
            .delete_subscription_filter()
            .log_group_name("/conformance/logs")
            .filter_name("conformance-filter")
            .send()
            .await,
        verbose
    ));

    // DeleteLogStream
    results.push(chk!(
        "DeleteLogStream",
        client
            .delete_log_stream()
            .log_group_name("/conformance/logs")
            .log_stream_name("conformance-stream")
            .send()
            .await,
        verbose
    ));

    // DeleteLogGroup
    results.push(chk!(
        "DeleteLogGroup",
        client
            .delete_log_group()
            .log_group_name("/conformance/logs")
            .send()
            .await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// EC2
// ---------------------------------------------------------------------------

async fn test_ec2(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_ec2::Client::new(&config);
    let mut results = Vec::new();

    // RunInstances
    let run_r = client
        .run_instances()
        .image_id("ami-00000000conformance")
        .instance_type(aws_sdk_ec2::types::InstanceType::T2Micro)
        .min_count(1)
        .max_count(1)
        .send()
        .await;
    let instance_id = run_r
        .as_ref()
        .ok()
        .and_then(|r| r.instances.as_ref())
        .and_then(|i| i.first())
        .and_then(|i| i.instance_id.clone());
    results.push(chk!("RunInstances", run_r, verbose));

    // DescribeInstances
    results.push(chk!(
        "DescribeInstances",
        client.describe_instances().send().await,
        verbose
    ));

    if let Some(ref iid) = instance_id {
        // CreateTags
        results.push(chk!(
            "CreateTags",
            client
                .create_tags()
                .resources(iid)
                .tags(
                    aws_sdk_ec2::types::Tag::builder()
                        .key("env")
                        .value("conformance")
                        .build(),
                )
                .send()
                .await,
            verbose
        ));

        // DescribeTags
        results.push(chk!(
            "DescribeTags",
            client
                .describe_tags()
                .filters(
                    aws_sdk_ec2::types::Filter::builder()
                        .name("resource-id")
                        .values(iid)
                        .build(),
                )
                .send()
                .await,
            verbose
        ));

        // TerminateInstances
        results.push(chk!(
            "TerminateInstances",
            client
                .terminate_instances()
                .instance_ids(iid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("CreateTags".to_string()));
        results.push(OpResult::Skipped("DescribeTags".to_string()));
        results.push(OpResult::Skipped("TerminateInstances".to_string()));
    }

    results
}

// ---------------------------------------------------------------------------
// CloudFormation
// ---------------------------------------------------------------------------

async fn test_cloudformation(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_cloudformation::Client::new(&config);
    let mut results = Vec::new();

    let template = r#"{"AWSTemplateFormatVersion":"2010-09-09","Description":"Conformance test stack","Resources":{"ConformanceBucket":{"Type":"AWS::S3::Bucket","Properties":{"BucketName":"conformance-cfn-bucket"}}}}"#;

    // CreateStack
    results.push(chk!(
        "CreateStack",
        client
            .create_stack()
            .stack_name("conformance-stack")
            .template_body(template)
            .send()
            .await,
        verbose
    ));

    // DescribeStacks
    results.push(chk!(
        "DescribeStacks",
        client
            .describe_stacks()
            .stack_name("conformance-stack")
            .send()
            .await,
        verbose
    ));

    // DescribeStackResources
    results.push(chk!(
        "DescribeStackResources",
        client
            .describe_stack_resources()
            .stack_name("conformance-stack")
            .send()
            .await,
        verbose
    ));

    // ListStacks
    results.push(chk!(
        "ListStacks",
        client.list_stacks().send().await,
        verbose
    ));

    // GetTemplate
    results.push(chk!(
        "GetTemplate",
        client
            .get_template()
            .stack_name("conformance-stack")
            .send()
            .await,
        verbose
    ));

    // GetTemplateSummary
    results.push(chk!(
        "GetTemplateSummary",
        client
            .get_template_summary()
            .template_body(template)
            .send()
            .await,
        verbose
    ));

    // DeleteStack
    results.push(chk!(
        "DeleteStack",
        client
            .delete_stack()
            .stack_name("conformance-stack")
            .send()
            .await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// RDS
// ---------------------------------------------------------------------------

async fn test_rds(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_rds::Client::new(&config);
    let mut results = Vec::new();

    // DescribeDBEngineVersions
    results.push(chk!(
        "DescribeDBEngineVersions",
        client.describe_db_engine_versions().send().await,
        verbose
    ));

    // CreateDBInstance
    let db_r = client
        .create_db_instance()
        .db_instance_identifier("conformance-db")
        .db_instance_class("db.t3.micro")
        .engine("mysql")
        .master_username("admin")
        .master_user_password("Password123!")
        .allocated_storage(20)
        .send()
        .await;
    results.push(chk!("CreateDBInstance", db_r, verbose));

    // DescribeDBInstances
    results.push(chk!(
        "DescribeDBInstances",
        client.describe_db_instances().send().await,
        verbose
    ));

    // CreateDBSnapshot
    let snap_r = client
        .create_db_snapshot()
        .db_instance_identifier("conformance-db")
        .db_snapshot_identifier("conformance-snapshot")
        .send()
        .await;
    results.push(chk!("CreateDBSnapshot", snap_r, verbose));

    // DescribeDBSnapshots
    results.push(chk!(
        "DescribeDBSnapshots",
        client.describe_db_snapshots().send().await,
        verbose
    ));

    // DeleteDBSnapshot
    results.push(chk!(
        "DeleteDBSnapshot",
        client
            .delete_db_snapshot()
            .db_snapshot_identifier("conformance-snapshot")
            .send()
            .await,
        verbose
    ));

    // DeleteDBInstance
    results.push(chk!(
        "DeleteDBInstance",
        client
            .delete_db_instance()
            .db_instance_identifier("conformance-db")
            .skip_final_snapshot(true)
            .send()
            .await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// Route53
// ---------------------------------------------------------------------------

async fn test_route53(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_route53::Client::new(&config);
    let mut results = Vec::new();

    // CreateHostedZone
    let create_zone_r = client
        .create_hosted_zone()
        .name("conformance.example.com.")
        .caller_reference(uuid::Uuid::new_v4().to_string())
        .send()
        .await;
    let zone_id = create_zone_r
        .as_ref()
        .ok()
        .and_then(|r| r.hosted_zone.as_ref())
        .map(|z| z.id.clone());
    results.push(chk!("CreateHostedZone", create_zone_r, verbose));

    // ListHostedZones
    results.push(chk!(
        "ListHostedZones",
        client.list_hosted_zones().send().await,
        verbose
    ));

    // GetHostedZoneCount
    results.push(chk!(
        "GetHostedZoneCount",
        client.get_hosted_zone_count().send().await,
        verbose
    ));

    if let Some(ref zid) = zone_id {
        // GetHostedZone
        results.push(chk!(
            "GetHostedZone",
            client.get_hosted_zone().id(zid).send().await,
            verbose
        ));

        // ChangeResourceRecordSets (add an A record)
        results.push(chk!(
            "ChangeResourceRecordSets",
            client
                .change_resource_record_sets()
                .hosted_zone_id(zid)
                .change_batch(
                    aws_sdk_route53::types::ChangeBatch::builder()
                        .changes(
                            aws_sdk_route53::types::Change::builder()
                                .action(aws_sdk_route53::types::ChangeAction::Create)
                                .resource_record_set(
                                    aws_sdk_route53::types::ResourceRecordSet::builder()
                                        .name("www.conformance.example.com.")
                                        .r#type(aws_sdk_route53::types::RrType::A)
                                        .ttl(300)
                                        .resource_records(
                                            aws_sdk_route53::types::ResourceRecord::builder()
                                                .value("1.2.3.4")
                                                .build()
                                                .unwrap(),
                                        )
                                        .build()
                                        .unwrap(),
                                )
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

        // ListResourceRecordSets
        results.push(chk!(
            "ListResourceRecordSets",
            client
                .list_resource_record_sets()
                .hosted_zone_id(zid)
                .send()
                .await,
            verbose
        ));

        // DeleteHostedZone
        results.push(chk!(
            "DeleteHostedZone",
            client.delete_hosted_zone().id(zid).send().await,
            verbose
        ));
    } else {
        for op in &[
            "GetHostedZone",
            "ChangeResourceRecordSets",
            "ListResourceRecordSets",
            "DeleteHostedZone",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    // CreateHealthCheck
    let create_hc_r = client
        .create_health_check()
        .caller_reference(uuid::Uuid::new_v4().to_string())
        .health_check_config(
            aws_sdk_route53::types::HealthCheckConfig::builder()
                .ip_address("1.2.3.4")
                .port(80)
                .r#type(aws_sdk_route53::types::HealthCheckType::Http)
                .resource_path("/")
                .request_interval(30)
                .failure_threshold(3)
                .build()
                .unwrap(),
        )
        .send()
        .await;
    let health_check_id = create_hc_r
        .as_ref()
        .ok()
        .and_then(|r| r.health_check.as_ref())
        .map(|h| h.id.clone());
    results.push(chk!("CreateHealthCheck", create_hc_r, verbose));

    // ListHealthChecks
    results.push(chk!(
        "ListHealthChecks",
        client.list_health_checks().send().await,
        verbose
    ));

    // GetHealthCheck
    if let Some(ref hcid) = health_check_id {
        results.push(chk!(
            "GetHealthCheck",
            client.get_health_check().health_check_id(hcid).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetHealthCheck".to_string()));
    }

    // ListGeoLocations
    results.push(chk!(
        "ListGeoLocations",
        client.list_geo_locations().send().await,
        verbose
    ));

    // ListReusableDelegationSets
    results.push(chk!(
        "ListReusableDelegationSets",
        client.list_reusable_delegation_sets().send().await,
        verbose
    ));

    // DeleteHealthCheck
    if let Some(ref hcid) = health_check_id {
        results.push(chk!(
            "DeleteHealthCheck",
            client.delete_health_check().health_check_id(hcid).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteHealthCheck".to_string()));
    }

    results
}

// ---------------------------------------------------------------------------
// CloudFront
// ---------------------------------------------------------------------------

async fn test_cloudfront(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_cloudfront::Client::new(&config);
    let mut results = Vec::new();

    // ListDistributions (empty)
    results.push(chk!(
        "ListDistributions",
        client.list_distributions().send().await,
        verbose
    ));

    // ListCachePolicies
    results.push(chk!(
        "ListCachePolicies",
        client.list_cache_policies().send().await,
        verbose
    ));

    // ListCloudFrontOriginAccessIdentities
    results.push(chk!(
        "ListCloudFrontOriginAccessIdentities",
        client.list_cloud_front_origin_access_identities().send().await,
        verbose
    ));

    // ListOriginAccessControls
    results.push(chk!(
        "ListOriginAccessControls",
        client.list_origin_access_controls().send().await,
        verbose
    ));

    // ListOriginRequestPolicies
    results.push(chk!(
        "ListOriginRequestPolicies",
        client.list_origin_request_policies().send().await,
        verbose
    ));

    // ListKeyGroups
    results.push(chk!(
        "ListKeyGroups",
        client.list_key_groups().send().await,
        verbose
    ));

    // ListPublicKeys
    results.push(chk!(
        "ListPublicKeys",
        client.list_public_keys().send().await,
        verbose
    ));

    // ListFunctions
    results.push(chk!(
        "ListFunctions",
        client.list_functions().send().await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// ELB (Elastic Load Balancing v2)
// ---------------------------------------------------------------------------

async fn test_elb(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_elasticloadbalancingv2::Client::new(&config);
    let mut results = Vec::new();

    // CreateLoadBalancer
    let create_lb_r = client
        .create_load_balancer()
        .name("conformance-lb")
        .r#type(aws_sdk_elasticloadbalancingv2::types::LoadBalancerTypeEnum::Application)
        .scheme(aws_sdk_elasticloadbalancingv2::types::LoadBalancerSchemeEnum::InternetFacing)
        .send()
        .await;
    let lb_arn = create_lb_r
        .as_ref()
        .ok()
        .and_then(|r| r.load_balancers.as_ref())
        .and_then(|lbs| lbs.first())
        .and_then(|lb| lb.load_balancer_arn.clone());
    results.push(chk!("CreateLoadBalancer", create_lb_r, verbose));

    // DescribeLoadBalancers
    results.push(chk!(
        "DescribeLoadBalancers",
        client.describe_load_balancers().send().await,
        verbose
    ));

    // CreateTargetGroup
    let create_tg_r = client
        .create_target_group()
        .name("conformance-tg")
        .protocol(aws_sdk_elasticloadbalancingv2::types::ProtocolEnum::Http)
        .port(80)
        .target_type(aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum::Instance)
        .vpc_id("vpc-00000000")
        .send()
        .await;
    let tg_arn = create_tg_r
        .as_ref()
        .ok()
        .and_then(|r| r.target_groups.as_ref())
        .and_then(|tgs| tgs.first())
        .and_then(|tg| tg.target_group_arn.clone());
    results.push(chk!("CreateTargetGroup", create_tg_r, verbose));

    // DescribeTargetGroups
    results.push(chk!(
        "DescribeTargetGroups",
        client.describe_target_groups().send().await,
        verbose
    ));

    // DescribeLoadBalancerAttributes
    if let Some(ref arn) = lb_arn {
        results.push(chk!(
            "DescribeLoadBalancerAttributes",
            client
                .describe_load_balancer_attributes()
                .load_balancer_arn(arn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped(
            "DescribeLoadBalancerAttributes".to_string(),
        ));
    }

    // DescribeTargetGroupAttributes
    if let Some(ref arn) = tg_arn {
        results.push(chk!(
            "DescribeTargetGroupAttributes",
            client
                .describe_target_group_attributes()
                .target_group_arn(arn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped(
            "DescribeTargetGroupAttributes".to_string(),
        ));
    }

    // CreateListener (requires lb + tg arns)
    let listener_arn = if let (Some(l_arn), Some(t_arn)) = (&lb_arn, &tg_arn) {
        let create_listener_r = client
            .create_listener()
            .load_balancer_arn(l_arn)
            .protocol(aws_sdk_elasticloadbalancingv2::types::ProtocolEnum::Http)
            .port(80)
            .default_actions(
                aws_sdk_elasticloadbalancingv2::types::Action::builder()
                    .r#type(
                        aws_sdk_elasticloadbalancingv2::types::ActionTypeEnum::Forward,
                    )
                    .target_group_arn(t_arn)
                    .build(),
            )
            .send()
            .await;
        let arn = create_listener_r
            .as_ref()
            .ok()
            .and_then(|r| r.listeners.as_ref())
            .and_then(|ls| ls.first())
            .and_then(|l| l.listener_arn.clone());
        results.push(chk!("CreateListener", create_listener_r, verbose));
        arn
    } else {
        results.push(OpResult::Skipped("CreateListener".to_string()));
        None
    };

    // DescribeListeners
    results.push(chk!(
        "DescribeListeners",
        client.describe_listeners().send().await,
        verbose
    ));

    // DeleteListener
    if let Some(ref l_arn) = listener_arn {
        results.push(chk!(
            "DeleteListener",
            client
                .delete_listener()
                .listener_arn(l_arn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteListener".to_string()));
    }

    // DeleteTargetGroup
    if let Some(ref arn) = tg_arn {
        results.push(chk!(
            "DeleteTargetGroup",
            client.delete_target_group().target_group_arn(arn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteTargetGroup".to_string()));
    }

    // DescribeAccountLimits
    results.push(chk!(
        "DescribeAccountLimits",
        client.describe_account_limits().send().await,
        verbose
    ));

    // DescribeSSLPolicies
    results.push(chk!(
        "DescribeSSLPolicies",
        client.describe_ssl_policies().send().await,
        verbose
    ));

    // DeleteLoadBalancer
    if let Some(ref arn) = lb_arn {
        results.push(chk!(
            "DeleteLoadBalancer",
            client.delete_load_balancer().load_balancer_arn(arn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteLoadBalancer".to_string()));
    }

    results
}

// ---------------------------------------------------------------------------
// ACM (Certificate Manager)
// ---------------------------------------------------------------------------

async fn test_acm(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_acm::Client::new(&config);
    let mut results = Vec::new();

    // RequestCertificate
    let request_r = client
        .request_certificate()
        .domain_name("conformance.example.com")
        .send()
        .await;
    let cert_arn = request_r
        .as_ref()
        .ok()
        .and_then(|r| r.certificate_arn.clone());
    results.push(chk!("RequestCertificate", request_r, verbose));

    // ListCertificates
    results.push(chk!(
        "ListCertificates",
        client.list_certificates().send().await,
        verbose
    ));

    if let Some(ref arn) = cert_arn {
        // DescribeCertificate
        results.push(chk!(
            "DescribeCertificate",
            client.describe_certificate().certificate_arn(arn).send().await,
            verbose
        ));

        // GetCertificate
        results.push(chk!(
            "GetCertificate",
            client.get_certificate().certificate_arn(arn).send().await,
            verbose
        ));

        // AddTagsToCertificate
        results.push(chk!(
            "AddTagsToCertificate",
            client
                .add_tags_to_certificate()
                .certificate_arn(arn)
                .tags(
                    aws_sdk_acm::types::Tag::builder()
                        .key("env")
                        .value("conformance")
                        .build()
                        .unwrap(),
                )
                .send()
                .await,
            verbose
        ));

        // ListTagsForCertificate
        results.push(chk!(
            "ListTagsForCertificate",
            client
                .list_tags_for_certificate()
                .certificate_arn(arn)
                .send()
                .await,
            verbose
        ));

        // RenewCertificate
        results.push(chk!(
            "RenewCertificate",
            client.renew_certificate().certificate_arn(arn).send().await,
            verbose
        ));

        // DeleteCertificate
        results.push(chk!(
            "DeleteCertificate",
            client.delete_certificate().certificate_arn(arn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DescribeCertificate".to_string()));
        results.push(OpResult::Skipped("GetCertificate".to_string()));
        results.push(OpResult::Skipped("AddTagsToCertificate".to_string()));
        results.push(OpResult::Skipped("ListTagsForCertificate".to_string()));
        results.push(OpResult::Skipped("RenewCertificate".to_string()));
        results.push(OpResult::Skipped("DeleteCertificate".to_string()));
    }

    results
}

// ---------------------------------------------------------------------------
// WAF (WAFv2)
// ---------------------------------------------------------------------------

async fn test_waf(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_wafv2::Client::new(&config);
    let mut results = Vec::new();

    let scope = aws_sdk_wafv2::types::Scope::Regional;

    // CreateWebACL
    let create_acl_r = client
        .create_web_acl()
        .name("conformance-web-acl")
        .scope(scope.clone())
        .default_action(
            aws_sdk_wafv2::types::DefaultAction::builder()
                .allow(aws_sdk_wafv2::types::AllowAction::builder().build())
                .build(),
        )
        .visibility_config(
            aws_sdk_wafv2::types::VisibilityConfig::builder()
                .cloud_watch_metrics_enabled(false)
                .metric_name("conformance-web-acl")
                .sampled_requests_enabled(false)
                .build()
                .unwrap(),
        )
        .send()
        .await;
    let (acl_id, acl_lock_token) = create_acl_r
        .as_ref()
        .ok()
        .and_then(|r| r.summary.as_ref())
        .map(|s| (s.id.clone(), s.lock_token.clone()))
        .unwrap_or((None, None));
    results.push(chk!("CreateWebACL", create_acl_r, verbose));

    // ListWebACLs
    results.push(chk!(
        "ListWebACLs",
        client.list_web_acls().scope(scope.clone()).send().await,
        verbose
    ));

    if let (Some(id), Some(token)) = (&acl_id, &acl_lock_token) {
        // GetWebACL
        results.push(chk!(
            "GetWebACL",
            client
                .get_web_acl()
                .name("conformance-web-acl")
                .scope(scope.clone())
                .id(id)
                .send()
                .await,
            verbose
        ));

        // DeleteWebACL
        results.push(chk!(
            "DeleteWebACL",
            client
                .delete_web_acl()
                .name("conformance-web-acl")
                .scope(scope.clone())
                .id(id)
                .lock_token(token)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetWebACL".to_string()));
        results.push(OpResult::Skipped("DeleteWebACL".to_string()));
    }

    // CreateIPSet
    let create_ip_r = client
        .create_ip_set()
        .name("conformance-ip-set")
        .scope(scope.clone())
        .ip_address_version(aws_sdk_wafv2::types::IpAddressVersion::Ipv4)
        .addresses("1.2.3.4/32")
        .send()
        .await;
    let (ip_set_id, ip_lock_token) = create_ip_r
        .as_ref()
        .ok()
        .and_then(|r| r.summary.as_ref())
        .map(|s| (s.id.clone(), s.lock_token.clone()))
        .unwrap_or((None, None));
    results.push(chk!("CreateIPSet", create_ip_r, verbose));

    // ListIPSets
    results.push(chk!(
        "ListIPSets",
        client.list_ip_sets().scope(scope.clone()).send().await,
        verbose
    ));

    // GetIPSet
    if let (Some(id), Some(_token)) = (&ip_set_id, &ip_lock_token) {
        results.push(chk!(
            "GetIPSet",
            client
                .get_ip_set()
                .name("conformance-ip-set")
                .scope(scope.clone())
                .id(id)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetIPSet".to_string()));
    }

    // CheckCapacity
    results.push(chk!(
        "CheckCapacity",
        client
            .check_capacity()
            .scope(scope.clone())
            .send()
            .await,
        verbose
    ));

    // ListAvailableManagedRuleGroups
    results.push(chk!(
        "ListAvailableManagedRuleGroups",
        client
            .list_available_managed_rule_groups()
            .scope(scope.clone())
            .send()
            .await,
        verbose
    ));

    // PutLoggingConfiguration
    let logging_resource_arn =
        "arn:aws:wafv2:us-east-1:000000000000:regional/webacl/conformance-logging/abc";
    let put_log_r = client
        .put_logging_configuration()
        .logging_configuration(
            aws_sdk_wafv2::types::LoggingConfiguration::builder()
                .resource_arn(logging_resource_arn)
                .log_destination_configs(
                    "arn:aws:logs:us-east-1:000000000000:log-group:aws-waf-logs-conformance",
                )
                .build()
                .unwrap(),
        )
        .send()
        .await;
    results.push(chk!("PutLoggingConfiguration", put_log_r, verbose));

    // GetLoggingConfiguration
    results.push(chk!(
        "GetLoggingConfiguration",
        client
            .get_logging_configuration()
            .resource_arn(logging_resource_arn)
            .send()
            .await,
        verbose
    ));

    // ListLoggingConfigurations
    results.push(chk!(
        "ListLoggingConfigurations",
        client
            .list_logging_configurations()
            .scope(scope.clone())
            .send()
            .await,
        verbose
    ));

    // DeleteLoggingConfiguration
    results.push(chk!(
        "DeleteLoggingConfiguration",
        client
            .delete_logging_configuration()
            .resource_arn(logging_resource_arn)
            .send()
            .await,
        verbose
    ));

    // DeleteIPSet
    if let (Some(id), Some(token)) = (&ip_set_id, &ip_lock_token) {
        results.push(chk!(
            "DeleteIPSet",
            client
                .delete_ip_set()
                .name("conformance-ip-set")
                .scope(scope.clone())
                .id(id)
                .lock_token(token)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteIPSet".to_string()));
    }

    results
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

async fn test_scheduler(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_scheduler::Client::new(&config);
    let mut results = Vec::new();

    // CreateScheduleGroup
    let create_grp_r = client
        .create_schedule_group()
        .name("conformance-group")
        .send()
        .await;
    results.push(chk!("CreateScheduleGroup", create_grp_r, verbose));

    // ListScheduleGroups
    results.push(chk!(
        "ListScheduleGroups",
        client.list_schedule_groups().send().await,
        verbose
    ));

    // CreateSchedule
    let create_sched_r = client
        .create_schedule()
        .name("conformance-schedule")
        .schedule_expression("rate(1 minute)")
        .flexible_time_window(
            aws_sdk_scheduler::types::FlexibleTimeWindow::builder()
                .mode(aws_sdk_scheduler::types::FlexibleTimeWindowMode::Off)
                .build()
                .unwrap(),
        )
        .target(
            aws_sdk_scheduler::types::Target::builder()
                .arn("arn:aws:lambda:us-east-1:000000000000:function:conformance")
                .role_arn("arn:aws:iam::000000000000:role/scheduler-role")
                .build()
                .unwrap(),
        )
        .send()
        .await;
    results.push(chk!("CreateSchedule", create_sched_r, verbose));

    // ListSchedules
    results.push(chk!(
        "ListSchedules",
        client.list_schedules().send().await,
        verbose
    ));

    // GetSchedule
    results.push(chk!(
        "GetSchedule",
        client
            .get_schedule()
            .name("conformance-schedule")
            .send()
            .await,
        verbose
    ));

    // UpdateSchedule
    results.push(chk!(
        "UpdateSchedule",
        client
            .update_schedule()
            .name("conformance-schedule")
            .schedule_expression("rate(5 minutes)")
            .flexible_time_window(
                aws_sdk_scheduler::types::FlexibleTimeWindow::builder()
                    .mode(aws_sdk_scheduler::types::FlexibleTimeWindowMode::Off)
                    .build()
                    .unwrap(),
            )
            .target(
                aws_sdk_scheduler::types::Target::builder()
                    .arn("arn:aws:lambda:us-east-1:000000000000:function:conformance")
                    .role_arn("arn:aws:iam::000000000000:role/scheduler-role")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetScheduleGroup
    results.push(chk!(
        "GetScheduleGroup",
        client
            .get_schedule_group()
            .name("conformance-group")
            .send()
            .await,
        verbose
    ));

    // TagResource (on schedule)
    let sched_arn = format!(
        "arn:aws:scheduler:us-east-1:000000000000:schedule/default/conformance-schedule"
    );
    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource_arn(&sched_arn)
            .tags(
                aws_sdk_scheduler::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTagsForResource
    results.push(chk!(
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_arn(&sched_arn)
            .send()
            .await,
        verbose
    ));

    // UntagResource
    results.push(chk!(
        "UntagResource",
        client
            .untag_resource()
            .resource_arn(&sched_arn)
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // DeleteSchedule
    results.push(chk!(
        "DeleteSchedule",
        client
            .delete_schedule()
            .name("conformance-schedule")
            .send()
            .await,
        verbose
    ));

    // DeleteScheduleGroup
    results.push(chk!(
        "DeleteScheduleGroup",
        client
            .delete_schedule_group()
            .name("conformance-group")
            .send()
            .await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// AppSync
// ---------------------------------------------------------------------------

async fn test_appsync(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_appsync::Client::new(&config);
    let mut results = Vec::new();

    // CreateGraphqlApi
    let create_r = client
        .create_graphql_api()
        .name("conformance-api")
        .authentication_type(aws_sdk_appsync::types::AuthenticationType::ApiKey)
        .send()
        .await;
    let api_id = create_r
        .as_ref()
        .ok()
        .and_then(|r| r.graphql_api.as_ref())
        .and_then(|a| a.api_id.clone());
    results.push(chk!("CreateGraphqlApi", create_r, verbose));

    // ListGraphqlApis
    results.push(chk!(
        "ListGraphqlApis",
        client.list_graphql_apis().send().await,
        verbose
    ));

    if let Some(ref aid) = api_id {
        // GetGraphqlApi
        results.push(chk!(
            "GetGraphqlApi",
            client.get_graphql_api().api_id(aid).send().await,
            verbose
        ));

        // CreateApiKey
        results.push(chk!(
            "CreateApiKey",
            client
                .create_api_key()
                .api_id(aid)
                .description("conformance-key")
                .send()
                .await,
            verbose
        ));

        // ListApiKeys
        results.push(chk!(
            "ListApiKeys",
            client.list_api_keys().api_id(aid).send().await,
            verbose
        ));

        // CreateDataSource
        results.push(chk!(
            "CreateDataSource",
            client
                .create_data_source()
                .api_id(aid)
                .name("noneds")
                .r#type(aws_sdk_appsync::types::DataSourceType::None)
                .send()
                .await,
            verbose
        ));

        // GetDataSource
        results.push(chk!(
            "GetDataSource",
            client.get_data_source().api_id(aid).name("noneds").send().await,
            verbose
        ));

        // ListSourceApiAssociations
        results.push(chk!(
            "ListSourceApiAssociations",
            client.list_source_api_associations().api_id(aid).send().await,
            verbose
        ));

        // DeleteGraphqlApi
        results.push(chk!(
            "DeleteGraphqlApi",
            client.delete_graphql_api().api_id(aid).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetGraphqlApi".to_string()));
        results.push(OpResult::Skipped("CreateApiKey".to_string()));
        results.push(OpResult::Skipped("ListApiKeys".to_string()));
        results.push(OpResult::Skipped("CreateDataSource".to_string()));
        results.push(OpResult::Skipped("GetDataSource".to_string()));
        results.push(OpResult::Skipped("ListSourceApiAssociations".to_string()));
        results.push(OpResult::Skipped("DeleteGraphqlApi".to_string()));
    }

    results
}

// ---------------------------------------------------------------------------
// Glue
// ---------------------------------------------------------------------------

async fn test_glue(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_glue::Client::new(&config);
    let mut results = Vec::new();

    // CreateDatabase
    results.push(chk!(
        "CreateDatabase",
        client
            .create_database()
            .database_input(
                aws_sdk_glue::types::DatabaseInput::builder()
                    .name("conformance_db")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetDatabase
    results.push(chk!(
        "GetDatabase",
        client.get_database().name("conformance_db").send().await,
        verbose
    ));

    // GetDatabases
    results.push(chk!(
        "GetDatabases",
        client.get_databases().send().await,
        verbose
    ));

    // CreateTable
    results.push(chk!(
        "CreateTable",
        client
            .create_table()
            .database_name("conformance_db")
            .table_input(
                aws_sdk_glue::types::TableInput::builder()
                    .name("conformance_table")
                    .storage_descriptor(
                        aws_sdk_glue::types::StorageDescriptor::builder()
                            .location("s3://conformance-bucket/data/")
                            .input_format(
                                "org.apache.hadoop.mapred.TextInputFormat",
                            )
                            .output_format(
                                "org.apache.hadoop.hive.ql.io.HiveIgnoreKeyTextOutputFormat",
                            )
                            .serde_info(
                                aws_sdk_glue::types::SerDeInfo::builder()
                                    .serialization_library(
                                        "org.apache.hadoop.hive.serde2.lazy.LazySimpleSerDe",
                                    )
                                    .build(),
                            )
                            .build(),
                    )
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetTable
    results.push(chk!(
        "GetTable",
        client
            .get_table()
            .database_name("conformance_db")
            .name("conformance_table")
            .send()
            .await,
        verbose
    ));

    // GetTables
    results.push(chk!(
        "GetTables",
        client
            .get_tables()
            .database_name("conformance_db")
            .send()
            .await,
        verbose
    ));

    // CreateCrawler
    results.push(chk!(
        "CreateCrawler",
        client
            .create_crawler()
            .name("conformance-crawler")
            .role("arn:aws:iam::000000000000:role/glue-crawler-role")
            .targets(
                aws_sdk_glue::types::CrawlerTargets::builder()
                    .s3_targets(
                        aws_sdk_glue::types::S3Target::builder()
                            .path("s3://conformance-bucket/")
                            .build(),
                    )
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetCrawler
    results.push(chk!(
        "GetCrawler",
        client
            .get_crawler()
            .name("conformance-crawler")
            .send()
            .await,
        verbose
    ));

    // GetCrawlers
    results.push(chk!(
        "GetCrawlers",
        client.get_crawlers().send().await,
        verbose
    ));

    // CreateJob
    results.push(chk!(
        "CreateJob",
        client
            .create_job()
            .name("conformance-job")
            .role("arn:aws:iam::000000000000:role/glue-job-role")
            .command(
                aws_sdk_glue::types::JobCommand::builder()
                    .name("glueetl")
                    .script_location("s3://conformance-bucket/scripts/etl.py")
                    .python_version("3")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetJob
    results.push(chk!(
        "GetJob",
        client.get_job().job_name("conformance-job").send().await,
        verbose
    ));

    // GetJobs
    results.push(chk!(
        "GetJobs",
        client.get_jobs().send().await,
        verbose
    ));

    // CreateTrigger
    results.push(chk!(
        "CreateTrigger",
        client
            .create_trigger()
            .name("conformance-trigger")
            .r#type(aws_sdk_glue::types::TriggerType::OnDemand)
            .actions(
                aws_sdk_glue::types::Action::builder()
                    .job_name("conformance-job")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetTriggers
    results.push(chk!(
        "GetTriggers",
        client.get_triggers().send().await,
        verbose
    ));

    // CreateWorkflow
    results.push(chk!(
        "CreateWorkflow",
        client
            .create_workflow()
            .name("conformance-workflow")
            .send()
            .await,
        verbose
    ));

    // GetWorkflow
    results.push(chk!(
        "GetWorkflow",
        client
            .get_workflow()
            .name("conformance-workflow")
            .send()
            .await,
        verbose
    ));

    // DeleteWorkflow
    results.push(chk!(
        "DeleteWorkflow",
        client
            .delete_workflow()
            .name("conformance-workflow")
            .send()
            .await,
        verbose
    ));

    // DeleteTrigger
    results.push(chk!(
        "DeleteTrigger",
        client
            .delete_trigger()
            .name("conformance-trigger")
            .send()
            .await,
        verbose
    ));

    // DeleteTable (cleanup)
    results.push(chk!(
        "DeleteTable",
        client
            .delete_table()
            .database_name("conformance_db")
            .name("conformance_table")
            .send()
            .await,
        verbose
    ));

    // DeleteCrawler (cleanup)
    results.push(chk!(
        "DeleteCrawler",
        client
            .delete_crawler()
            .name("conformance-crawler")
            .send()
            .await,
        verbose
    ));

    // DeleteJob (cleanup)
    results.push(chk!(
        "DeleteJob",
        client.delete_job().job_name("conformance-job").send().await,
        verbose
    ));

    // DeleteDatabase (cleanup)
    results.push(chk!(
        "DeleteDatabase",
        client.delete_database().name("conformance_db").send().await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// Athena
// ---------------------------------------------------------------------------

async fn test_athena(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_athena::Client::new(&config);
    let mut results = Vec::new();

    // CreateWorkGroup
    results.push(chk!(
        "CreateWorkGroup",
        client
            .create_work_group()
            .name("conformance-workgroup")
            .send()
            .await,
        verbose
    ));

    // ListWorkGroups
    results.push(chk!(
        "ListWorkGroups",
        client.list_work_groups().send().await,
        verbose
    ));

    // GetWorkGroup
    results.push(chk!(
        "GetWorkGroup",
        client
            .get_work_group()
            .work_group("conformance-workgroup")
            .send()
            .await,
        verbose
    ));

    // StartQueryExecution
    let start_qe_r = client
        .start_query_execution()
        .query_string("SELECT 1")
        .work_group("conformance-workgroup")
        .query_execution_context(
            aws_sdk_athena::types::QueryExecutionContext::builder()
                .database("default")
                .build(),
        )
        .result_configuration(
            aws_sdk_athena::types::ResultConfiguration::builder()
                .output_location("s3://conformance-bucket/athena-results/")
                .build(),
        )
        .send()
        .await;
    let query_execution_id = start_qe_r
        .as_ref()
        .ok()
        .and_then(|r| r.query_execution_id.clone());
    results.push(chk!("StartQueryExecution", start_qe_r, verbose));

    // GetQueryExecution
    if let Some(ref qid) = query_execution_id {
        results.push(chk!(
            "GetQueryExecution",
            client
                .get_query_execution()
                .query_execution_id(qid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetQueryExecution".to_string()));
    }

    // ListQueryExecutions
    results.push(chk!(
        "ListQueryExecutions",
        client.list_query_executions().send().await,
        verbose
    ));

    // CreateNamedQuery
    let create_nq_r = client
        .create_named_query()
        .name("conformance-query")
        .database("default")
        .query_string("SELECT 1")
        .send()
        .await;
    let named_query_id = create_nq_r
        .as_ref()
        .ok()
        .and_then(|r| r.named_query_id.clone());
    results.push(chk!("CreateNamedQuery", create_nq_r, verbose));

    // ListNamedQueries
    results.push(chk!(
        "ListNamedQueries",
        client.list_named_queries().send().await,
        verbose
    ));

    // DeleteNamedQuery
    if let Some(ref nqid) = named_query_id {
        results.push(chk!(
            "DeleteNamedQuery",
            client
                .delete_named_query()
                .named_query_id(nqid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteNamedQuery".to_string()));
    }

    // ListDataCatalogs
    results.push(chk!(
        "ListDataCatalogs",
        client.list_data_catalogs().send().await,
        verbose
    ));

    // ListEngineVersions
    results.push(chk!(
        "ListEngineVersions",
        client.list_engine_versions().send().await,
        verbose
    ));

    // TagResource (Athena)
    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource_arn("arn:aws:athena:us-east-1:000000000000:workgroup/conformance-workgroup")
            .tags(
                aws_sdk_athena::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTagsForResource (Athena)
    results.push(chk!(
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_arn("arn:aws:athena:us-east-1:000000000000:workgroup/conformance-workgroup")
            .send()
            .await,
        verbose
    ));

    // DeleteWorkGroup (cleanup)
    results.push(chk!(
        "DeleteWorkGroup",
        client
            .delete_work_group()
            .work_group("conformance-workgroup")
            .recursive_delete_option(true)
            .send()
            .await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// Bedrock
// ---------------------------------------------------------------------------

async fn test_bedrock(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_bedrock::Client::new(&config);
    let mut results = Vec::new();

    // ListFoundationModels
    results.push(chk!(
        "ListFoundationModels",
        client.list_foundation_models().send().await,
        verbose
    ));

    // GetFoundationModel (use a known stub model id)
    results.push(chk!(
        "GetFoundationModel",
        client
            .get_foundation_model()
            .model_identifier("anthropic.claude-v2:1")
            .send()
            .await,
        verbose
    ));

    // ListGuardrails
    results.push(chk!(
        "ListGuardrails",
        client.list_guardrails().send().await,
        verbose
    ));

    // ListProvisionedModelThroughputs
    results.push(chk!(
        "ListProvisionedModelThroughputs",
        client.list_provisioned_model_throughputs().send().await,
        verbose
    ));

    // ListCustomModels
    results.push(chk!(
        "ListCustomModels",
        client.list_custom_models().send().await,
        verbose
    ));

    // ListModelCustomizationJobs
    results.push(chk!(
        "ListModelCustomizationJobs",
        client.list_model_customization_jobs().send().await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// Organizations
// ---------------------------------------------------------------------------

async fn test_organizations(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_organizations::Client::new(&config);
    let mut results = Vec::new();

    results.push(chk!(
        "CreateOrganization",
        client.create_organization().feature_set(aws_sdk_organizations::types::OrganizationFeatureSet::All).send().await,
        verbose
    ));
    results.push(chk!(
        "DescribeOrganization",
        client.describe_organization().send().await,
        verbose
    ));
    results.push(chk!(
        "CreateAccount",
        client.create_account().email("conf@example.com").account_name("conf-acct").send().await,
        verbose
    ));
    results.push(chk!(
        "ListAccounts",
        client.list_accounts().send().await,
        verbose
    ));
    results.push(chk!(
        "ListRoots",
        client.list_roots().send().await,
        verbose
    ));
    results.push(chk!(
        "CreateOrganizationalUnit",
        client.create_organizational_unit().parent_id("r-0000").name("conf-ou").send().await,
        verbose
    ));
    results.push(chk!(
        "ListOrganizationalUnitsForParent",
        client.list_organizational_units_for_parent().parent_id("r-0000").send().await,
        verbose
    ));
    results.push(chk!(
        "CreatePolicy",
        client
            .create_policy()
            .name("conf-policy")
            .description("conformance")
            .content("{\"Version\":\"2012-10-17\",\"Statement\":[]}")
            .r#type(aws_sdk_organizations::types::PolicyType::ServiceControlPolicy)
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "ListPolicies",
        client.list_policies().filter(aws_sdk_organizations::types::PolicyType::ServiceControlPolicy).send().await,
        verbose
    ));
    results.push(chk!(
        "ListChildren",
        client
            .list_children()
            .parent_id("r-0000")
            .child_type(aws_sdk_organizations::types::ChildType::OrganizationalUnit)
            .send()
            .await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// CloudTrail
// ---------------------------------------------------------------------------

async fn test_cloudtrail(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_cloudtrail::Client::new(&config);
    let mut results = Vec::new();

    results.push(chk!(
        "CreateTrail",
        client.create_trail().name("conf-trail").s3_bucket_name("conf-bucket").send().await,
        verbose
    ));
    results.push(chk!(
        "DescribeTrails",
        client.describe_trails().send().await,
        verbose
    ));
    results.push(chk!(
        "ListTrails",
        client.list_trails().send().await,
        verbose
    ));
    results.push(chk!(
        "GetTrailStatus",
        client.get_trail_status().name("conf-trail").send().await,
        verbose
    ));
    results.push(chk!(
        "StartLogging",
        client.start_logging().name("conf-trail").send().await,
        verbose
    ));
    results.push(chk!(
        "StopLogging",
        client.stop_logging().name("conf-trail").send().await,
        verbose
    ));
    results.push(chk!(
        "UpdateTrail",
        client.update_trail().name("conf-trail").s3_bucket_name("conf-bucket-2").send().await,
        verbose
    ));
    results.push(chk!(
        "PutEventSelectors",
        client
            .put_event_selectors()
            .trail_name("conf-trail")
            .event_selectors(
                aws_sdk_cloudtrail::types::EventSelector::builder()
                    .read_write_type(aws_sdk_cloudtrail::types::ReadWriteType::All)
                    .include_management_events(true)
                    .build(),
            )
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "GetEventSelectors",
        client.get_event_selectors().trail_name("conf-trail").send().await,
        verbose
    ));
    results.push(chk!(
        "LookupEvents",
        client.lookup_events().send().await,
        verbose
    ));
    results.push(chk!(
        "DeleteTrail",
        client.delete_trail().name("conf-trail").send().await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// EKS
// ---------------------------------------------------------------------------

async fn test_eks(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_eks::Client::new(&config);
    let mut results = Vec::new();

    results.push(chk!(
        "CreateCluster",
        client
            .create_cluster()
            .name("conf-cluster")
            .role_arn("arn:aws:iam::000000000000:role/conf")
            .resources_vpc_config(
                aws_sdk_eks::types::VpcConfigRequest::builder()
                    .subnet_ids("subnet-123")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "DescribeCluster",
        client.describe_cluster().name("conf-cluster").send().await,
        verbose
    ));
    results.push(chk!(
        "ListClusters",
        client.list_clusters().send().await,
        verbose
    ));
    results.push(chk!(
        "CreateNodegroup",
        client
            .create_nodegroup()
            .cluster_name("conf-cluster")
            .nodegroup_name("conf-ng")
            .node_role("arn:aws:iam::000000000000:role/ng")
            .subnets("subnet-123")
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "DescribeNodegroup",
        client
            .describe_nodegroup()
            .cluster_name("conf-cluster")
            .nodegroup_name("conf-ng")
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "ListNodegroups",
        client.list_nodegroups().cluster_name("conf-cluster").send().await,
        verbose
    ));
    results.push(chk!(
        "CreateFargateProfile",
        client
            .create_fargate_profile()
            .cluster_name("conf-cluster")
            .fargate_profile_name("conf-fp")
            .pod_execution_role_arn("arn:aws:iam::000000000000:role/fp")
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "DescribeFargateProfile",
        client
            .describe_fargate_profile()
            .cluster_name("conf-cluster")
            .fargate_profile_name("conf-fp")
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "ListFargateProfiles",
        client.list_fargate_profiles().cluster_name("conf-cluster").send().await,
        verbose
    ));
    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource_arn("arn:aws:eks:us-east-1:000000000000:cluster/conf-cluster")
            .tags("env", "conformance")
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_arn("arn:aws:eks:us-east-1:000000000000:cluster/conf-cluster")
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "DeleteFargateProfile",
        client
            .delete_fargate_profile()
            .cluster_name("conf-cluster")
            .fargate_profile_name("conf-fp")
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "DeleteNodegroup",
        client
            .delete_nodegroup()
            .cluster_name("conf-cluster")
            .nodegroup_name("conf-ng")
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "DeleteCluster",
        client.delete_cluster().name("conf-cluster").send().await,
        verbose
    ));

    results
}

// ---------------------------------------------------------------------------
// Firehose
// ---------------------------------------------------------------------------

async fn test_firehose(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_firehose::Client::new(&config);
    let mut results = Vec::new();

    results.push(chk!(
        "CreateDeliveryStream",
        client
            .create_delivery_stream()
            .delivery_stream_name("conf-stream")
            .delivery_stream_type(aws_sdk_firehose::types::DeliveryStreamType::DirectPut)
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "DescribeDeliveryStream",
        client.describe_delivery_stream().delivery_stream_name("conf-stream").send().await,
        verbose
    ));
    results.push(chk!(
        "ListDeliveryStreams",
        client.list_delivery_streams().send().await,
        verbose
    ));
    results.push(chk!(
        "PutRecord",
        client
            .put_record()
            .delivery_stream_name("conf-stream")
            .record(
                aws_sdk_firehose::types::Record::builder()
                    .data(aws_sdk_firehose::primitives::Blob::new(b"hello".to_vec()))
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "PutRecordBatch",
        client
            .put_record_batch()
            .delivery_stream_name("conf-stream")
            .records(
                aws_sdk_firehose::types::Record::builder()
                    .data(aws_sdk_firehose::primitives::Blob::new(b"a".to_vec()))
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "TagDeliveryStream",
        client
            .tag_delivery_stream()
            .delivery_stream_name("conf-stream")
            .tags(
                aws_sdk_firehose::types::Tag::builder()
                    .key("env")
                    .value("conf")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "ListTagsForDeliveryStream",
        client.list_tags_for_delivery_stream().delivery_stream_name("conf-stream").send().await,
        verbose
    ));
    results.push(chk!(
        "UntagDeliveryStream",
        client
            .untag_delivery_stream()
            .delivery_stream_name("conf-stream")
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "StartDeliveryStreamEncryption",
        client.start_delivery_stream_encryption().delivery_stream_name("conf-stream").send().await,
        verbose
    ));
    results.push(chk!(
        "StopDeliveryStreamEncryption",
        client.stop_delivery_stream_encryption().delivery_stream_name("conf-stream").send().await,
        verbose
    ));
    results.push(chk!(
        "DeleteDeliveryStream",
        client.delete_delivery_stream().delivery_stream_name("conf-stream").send().await,
        verbose
    ));

    results
}
