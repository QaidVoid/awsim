//! Extended integration tests using the official AWS SDK for Rust.
//!
//! Covers: CloudWatch Logs, SSM Parameter Store, Lambda, DynamoDB Query,
//! S3 Multipart Upload, and SQS FIFO queues.

use std::sync::Arc;

use aws_credential_types::Credentials;
use aws_sdk_dynamodb::types::{
    AttributeDefinition, AttributeValue, BillingMode, KeySchemaElement, KeyType,
    ScalarAttributeType,
};
use aws_sdk_s3::{
    primitives::ByteStream,
    types::{CompletedMultipartUpload, CompletedPart},
};
use aws_sdk_sqs::types::QueueAttributeName;
use awsim_core::{AppState, ServiceHandler};

// ---------------------------------------------------------------------------
// Server bootstrap helpers (duplicated from sdk_tests.rs so each test file
// is self-contained and tests can run concurrently without sharing a server)
// ---------------------------------------------------------------------------

async fn make_config(endpoint: &str) -> aws_config::SdkConfig {
    aws_config::defaults(aws_config::BehaviorVersion::latest())
        .endpoint_url(endpoint)
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(Credentials::new("test", "test", None, None, "test"))
        .load()
        .await
}

async fn make_s3_client(endpoint: &str) -> aws_sdk_s3::Client {
    let config = make_config(endpoint).await;
    let s3_config = aws_sdk_s3::config::Builder::from(&config)
        .force_path_style(true)
        .build();
    aws_sdk_s3::Client::from_conf(s3_config)
}

async fn start_server() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let endpoint = format!("http://{addr}");

    let mut state = AppState::new("us-east-1".into(), "000000000000".into());

    // IAM
    let iam = Arc::new(awsim_iam::IamService::new());
    state.register(iam, vec![]);

    // STS
    let sts = Arc::new(awsim_sts::StsService::new());
    state.register(sts, vec![]);

    // SNS
    let sns = Arc::new(awsim_sns::SnsService::new());
    state.register(sns, vec![]);

    // SQS
    let sqs = Arc::new(awsim_sqs::SqsService::new());
    state.register(sqs, vec![]);

    // DynamoDB
    let dynamodb = Arc::new(awsim_dynamodb::DynamoDbService::new());
    state.register(dynamodb, vec![]);

    // S3 (REST-based, provides its own route definitions)
    let s3 = awsim_s3::S3Service::new();
    let s3_routes = s3.routes();
    state.register(Arc::new(s3), s3_routes);

    // KMS
    let kms = Arc::new(awsim_kms::KmsService::new());
    state.register(kms, vec![]);

    // Secrets Manager
    let secrets = Arc::new(awsim_secretsmanager::SecretsManagerService::new());
    state.register(secrets, vec![]);

    // CloudWatch Logs
    let logs = Arc::new(awsim_cloudwatch_logs::CloudWatchLogsService::new());
    state.register(logs, vec![]);

    // SSM Parameter Store
    let ssm = Arc::new(awsim_ssm::SsmService::new());
    state.register(ssm, vec![]);

    // Lambda (REST-based, provides its own route definitions)
    let lambda = awsim_lambda::LambdaService::new();
    let lambda_routes = lambda.routes();
    state.register(Arc::new(lambda), lambda_routes);

    let app = axum::Router::new()
        .route(
            "/_awsim/health",
            axum::routing::get(|| async { "ok" }),
        )
        .fallback(awsim_core::gateway::handle_request)
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state);

    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    endpoint
}

// ---------------------------------------------------------------------------
// CloudWatch Logs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_cloudwatch_logs_crud() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_cloudwatchlogs::Client::new(&config);

    // Create log group
    client
        .create_log_group()
        .log_group_name("/test/app")
        .send()
        .await
        .expect("CreateLogGroup failed");

    // Create log stream
    client
        .create_log_stream()
        .log_group_name("/test/app")
        .log_stream_name("stream-1")
        .send()
        .await
        .expect("CreateLogStream failed");

    // Put log events
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    client
        .put_log_events()
        .log_group_name("/test/app")
        .log_stream_name("stream-1")
        .log_events(
            aws_sdk_cloudwatchlogs::types::InputLogEvent::builder()
                .message("Hello from AWSim!")
                .timestamp(now_ms)
                .build()
                .expect("InputLogEvent build failed"),
        )
        .send()
        .await
        .expect("PutLogEvents failed");

    // Describe log groups — should contain the one we just created
    let groups = client
        .describe_log_groups()
        .send()
        .await
        .expect("DescribeLogGroups failed");
    assert!(
        !groups.log_groups().is_empty(),
        "expected at least one log group"
    );
    let names: Vec<&str> = groups
        .log_groups()
        .iter()
        .filter_map(|g| g.log_group_name())
        .collect();
    assert!(
        names.contains(&"/test/app"),
        "log group not found in list: {names:?}"
    );
}

#[tokio::test]
async fn test_cloudwatch_logs_describe_streams() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_cloudwatchlogs::Client::new(&config);

    client
        .create_log_group()
        .log_group_name("/streams/test")
        .send()
        .await
        .expect("CreateLogGroup failed");

    for name in ["alpha", "beta", "gamma"] {
        client
            .create_log_stream()
            .log_group_name("/streams/test")
            .log_stream_name(name)
            .send()
            .await
            .expect("CreateLogStream failed");
    }

    let result = client
        .describe_log_streams()
        .log_group_name("/streams/test")
        .send()
        .await
        .expect("DescribeLogStreams failed");

    assert_eq!(
        result.log_streams().len(),
        3,
        "expected 3 streams, got {}",
        result.log_streams().len()
    );
}

// ---------------------------------------------------------------------------
// SSM Parameter Store
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_ssm_parameter_store() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_ssm::Client::new(&config);

    client
        .put_parameter()
        .name("/app/config/db-host")
        .value("localhost:5432")
        .r#type(aws_sdk_ssm::types::ParameterType::String)
        .send()
        .await
        .expect("PutParameter failed");

    let result = client
        .get_parameter()
        .name("/app/config/db-host")
        .send()
        .await
        .expect("GetParameter failed");

    let param = result.parameter().expect("parameter missing from response");
    assert_eq!(
        param.value().unwrap_or_default(),
        "localhost:5432",
        "parameter value mismatch"
    );
}

#[tokio::test]
async fn test_ssm_parameter_overwrite() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_ssm::Client::new(&config);

    client
        .put_parameter()
        .name("/app/version")
        .value("1.0")
        .r#type(aws_sdk_ssm::types::ParameterType::String)
        .send()
        .await
        .expect("PutParameter (v1) failed");

    client
        .put_parameter()
        .name("/app/version")
        .value("2.0")
        .r#type(aws_sdk_ssm::types::ParameterType::String)
        .overwrite(true)
        .send()
        .await
        .expect("PutParameter (overwrite v2) failed");

    let result = client
        .get_parameter()
        .name("/app/version")
        .send()
        .await
        .expect("GetParameter failed");

    assert_eq!(
        result.parameter().unwrap().value().unwrap_or_default(),
        "2.0"
    );
}

#[tokio::test]
async fn test_ssm_get_parameters_by_path() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_ssm::Client::new(&config);

    for (name, value) in [
        ("/service/prod/db-host", "db.prod.local"),
        ("/service/prod/db-port", "5432"),
        ("/service/prod/api-key", "secret"),
    ] {
        client
            .put_parameter()
            .name(name)
            .value(value)
            .r#type(aws_sdk_ssm::types::ParameterType::String)
            .send()
            .await
            .expect("PutParameter failed");
    }

    let result = client
        .get_parameters_by_path()
        .path("/service/prod")
        .recursive(true)
        .send()
        .await
        .expect("GetParametersByPath failed");

    assert_eq!(
        result.parameters().len(),
        3,
        "expected 3 parameters, got {}",
        result.parameters().len()
    );
}

// ---------------------------------------------------------------------------
// Lambda function management
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_lambda_create_list_delete() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_lambda::Client::new(&config);

    // Create function with minimal (non-functional) payload
    client
        .create_function()
        .function_name("test-func")
        .runtime(aws_sdk_lambda::types::Runtime::Nodejs20x)
        .role("arn:aws:iam::000000000000:role/test-role")
        .handler("index.handler")
        .code(
            aws_sdk_lambda::types::FunctionCode::builder()
                .zip_file(aws_sdk_lambda::primitives::Blob::new(vec![0u8; 10]))
                .build(),
        )
        .send()
        .await
        .expect("CreateFunction failed");

    // List functions — should contain the one we created
    let result = client
        .list_functions()
        .send()
        .await
        .expect("ListFunctions failed");

    assert!(
        !result.functions().is_empty(),
        "expected at least one function"
    );
    let func_names: Vec<&str> = result
        .functions()
        .iter()
        .filter_map(|f| f.function_name())
        .collect();
    assert!(
        func_names.contains(&"test-func"),
        "function not in list: {func_names:?}"
    );

    // Delete the function
    client
        .delete_function()
        .function_name("test-func")
        .send()
        .await
        .expect("DeleteFunction failed");

    // List again — should be empty
    let result2 = client
        .list_functions()
        .send()
        .await
        .expect("ListFunctions (after delete) failed");
    let func_names2: Vec<&str> = result2
        .functions()
        .iter()
        .filter_map(|f| f.function_name())
        .collect();
    assert!(
        !func_names2.contains(&"test-func"),
        "deleted function still in list"
    );
}

#[tokio::test]
async fn test_lambda_get_function() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_lambda::Client::new(&config);

    client
        .create_function()
        .function_name("get-me")
        .runtime(aws_sdk_lambda::types::Runtime::Python312)
        .role("arn:aws:iam::000000000000:role/test-role")
        .handler("handler.main")
        .code(
            aws_sdk_lambda::types::FunctionCode::builder()
                .zip_file(aws_sdk_lambda::primitives::Blob::new(vec![0u8; 10]))
                .build(),
        )
        .send()
        .await
        .expect("CreateFunction failed");

    let result = client
        .get_function()
        .function_name("get-me")
        .send()
        .await
        .expect("GetFunction failed");

    let conf = result.configuration().expect("configuration missing");
    assert_eq!(conf.function_name().unwrap_or_default(), "get-me");
    assert_eq!(conf.handler().unwrap_or_default(), "handler.main");
}

// ---------------------------------------------------------------------------
// DynamoDB Query with sort key
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_dynamodb_query() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_dynamodb::Client::new(&config);

    // Create table with composite key (pk + sk)
    client
        .create_table()
        .table_name("orders")
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("pk")
                .key_type(KeyType::Hash)
                .build()
                .unwrap(),
        )
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("sk")
                .key_type(KeyType::Range)
                .build()
                .unwrap(),
        )
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("pk")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .unwrap(),
        )
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("sk")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .unwrap(),
        )
        .billing_mode(BillingMode::PayPerRequest)
        .send()
        .await
        .expect("CreateTable failed");

    // Put 5 items for user-1 and 2 items for user-2
    for i in 1..=5u32 {
        client
            .put_item()
            .table_name("orders")
            .item("pk", AttributeValue::S("user-1".into()))
            .item("sk", AttributeValue::S(format!("order-{i:03}")))
            .item("total", AttributeValue::N(format!("{}", i * 100)))
            .send()
            .await
            .expect("PutItem failed");
    }
    for i in 1..=2u32 {
        client
            .put_item()
            .table_name("orders")
            .item("pk", AttributeValue::S("user-2".into()))
            .item("sk", AttributeValue::S(format!("order-{i:03}")))
            .item("total", AttributeValue::N(format!("{}", i * 50)))
            .send()
            .await
            .expect("PutItem (user-2) failed");
    }

    // Query — should return exactly 5 items for user-1
    let result = client
        .query()
        .table_name("orders")
        .key_condition_expression("pk = :pk")
        .expression_attribute_values(":pk", AttributeValue::S("user-1".into()))
        .send()
        .await
        .expect("Query failed");

    assert_eq!(result.count(), 5, "expected 5 items for user-1");
    assert!(result.items().iter().all(|item| {
        item.get("pk")
            .and_then(|v| v.as_s().ok())
            .map(|s| s == "user-1")
            .unwrap_or(false)
    }));
}

#[tokio::test]
async fn test_dynamodb_query_with_filter() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_dynamodb::Client::new(&config);

    client
        .create_table()
        .table_name("events")
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("pk")
                .key_type(KeyType::Hash)
                .build()
                .unwrap(),
        )
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("sk")
                .key_type(KeyType::Range)
                .build()
                .unwrap(),
        )
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("pk")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .unwrap(),
        )
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("sk")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .unwrap(),
        )
        .billing_mode(BillingMode::PayPerRequest)
        .send()
        .await
        .expect("CreateTable failed");

    // Put items with different statuses
    for (sk, status) in [
        ("event-001", "ACTIVE"),
        ("event-002", "INACTIVE"),
        ("event-003", "ACTIVE"),
        ("event-004", "INACTIVE"),
    ] {
        client
            .put_item()
            .table_name("events")
            .item("pk", AttributeValue::S("tenant-x".into()))
            .item("sk", AttributeValue::S(sk.into()))
            .item("status", AttributeValue::S(status.into()))
            .send()
            .await
            .expect("PutItem failed");
    }

    // Query with filter for ACTIVE only
    let result = client
        .query()
        .table_name("events")
        .key_condition_expression("pk = :pk")
        .filter_expression("#s = :status")
        .expression_attribute_names("#s", "status")
        .expression_attribute_values(":pk", AttributeValue::S("tenant-x".into()))
        .expression_attribute_values(":status", AttributeValue::S("ACTIVE".into()))
        .send()
        .await
        .expect("Query with filter failed");

    assert_eq!(result.count(), 2, "expected 2 ACTIVE items");
}

// ---------------------------------------------------------------------------
// S3 Multipart Upload
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_s3_multipart_upload() {
    let endpoint = start_server().await;
    let client = make_s3_client(&endpoint).await;

    client
        .create_bucket()
        .bucket("multipart-test")
        .send()
        .await
        .expect("CreateBucket failed");

    // Initiate multipart upload
    let create = client
        .create_multipart_upload()
        .bucket("multipart-test")
        .key("large-file.bin")
        .send()
        .await
        .expect("CreateMultipartUpload failed");
    let upload_id = create.upload_id().expect("upload_id missing");

    // Upload parts
    let part1 = client
        .upload_part()
        .bucket("multipart-test")
        .key("large-file.bin")
        .upload_id(upload_id)
        .part_number(1)
        .body(ByteStream::from_static(b"part one data "))
        .send()
        .await
        .expect("UploadPart 1 failed");

    let part2 = client
        .upload_part()
        .bucket("multipart-test")
        .key("large-file.bin")
        .upload_id(upload_id)
        .part_number(2)
        .body(ByteStream::from_static(b"part two data"))
        .send()
        .await
        .expect("UploadPart 2 failed");

    // Complete multipart upload
    client
        .complete_multipart_upload()
        .bucket("multipart-test")
        .key("large-file.bin")
        .upload_id(upload_id)
        .multipart_upload(
            CompletedMultipartUpload::builder()
                .parts(
                    CompletedPart::builder()
                        .part_number(1)
                        .e_tag(part1.e_tag().unwrap_or_default())
                        .build(),
                )
                .parts(
                    CompletedPart::builder()
                        .part_number(2)
                        .e_tag(part2.e_tag().unwrap_or_default())
                        .build(),
                )
                .build(),
        )
        .send()
        .await
        .expect("CompleteMultipartUpload failed");

    // Verify by downloading the assembled object
    let obj = client
        .get_object()
        .bucket("multipart-test")
        .key("large-file.bin")
        .send()
        .await
        .expect("GetObject failed");

    let body = obj
        .body
        .collect()
        .await
        .expect("collecting body failed")
        .into_bytes();

    assert_eq!(
        body.as_ref(),
        b"part one data part two data",
        "assembled body does not match"
    );
}

#[tokio::test]
async fn test_s3_abort_multipart_upload() {
    let endpoint = start_server().await;
    let client = make_s3_client(&endpoint).await;

    client
        .create_bucket()
        .bucket("abort-test")
        .send()
        .await
        .expect("CreateBucket failed");

    let create = client
        .create_multipart_upload()
        .bucket("abort-test")
        .key("will-be-aborted.bin")
        .send()
        .await
        .expect("CreateMultipartUpload failed");
    let upload_id = create.upload_id().expect("upload_id missing");

    // Upload one part
    client
        .upload_part()
        .bucket("abort-test")
        .key("will-be-aborted.bin")
        .upload_id(upload_id)
        .part_number(1)
        .body(ByteStream::from_static(b"some data"))
        .send()
        .await
        .expect("UploadPart failed");

    // Abort — should succeed
    client
        .abort_multipart_upload()
        .bucket("abort-test")
        .key("will-be-aborted.bin")
        .upload_id(upload_id)
        .send()
        .await
        .expect("AbortMultipartUpload failed");

    // Object should not exist after abort
    let result = client
        .list_objects_v2()
        .bucket("abort-test")
        .send()
        .await
        .expect("ListObjectsV2 failed");
    assert_eq!(
        result.key_count().unwrap_or(0),
        0,
        "object should not exist after abort"
    );
}

// ---------------------------------------------------------------------------
// SQS FIFO queue
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sqs_fifo_queue() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_sqs::Client::new(&config);

    let queue = client
        .create_queue()
        .queue_name("test.fifo")
        .attributes(QueueAttributeName::FifoQueue, "true")
        .send()
        .await
        .expect("CreateQueue (FIFO) failed");
    let url = queue.queue_url().expect("missing queue URL");

    client
        .send_message()
        .queue_url(url)
        .message_body("msg1")
        .message_group_id("group1")
        .message_deduplication_id("dedup1")
        .send()
        .await
        .expect("SendMessage to FIFO failed");

    let result = client
        .receive_message()
        .queue_url(url)
        .max_number_of_messages(1)
        .send()
        .await
        .expect("ReceiveMessage from FIFO failed");

    let messages = result.messages();
    assert_eq!(
        messages.len(),
        1,
        "expected 1 message, got {}",
        messages.len()
    );
    assert_eq!(
        messages[0].body().unwrap_or_default(),
        "msg1",
        "message body mismatch"
    );
}

#[tokio::test]
async fn test_sqs_fifo_deduplication() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_sqs::Client::new(&config);

    let queue = client
        .create_queue()
        .queue_name("dedup.fifo")
        .attributes(QueueAttributeName::FifoQueue, "true")
        .send()
        .await
        .expect("CreateQueue (FIFO) failed");
    let url = queue.queue_url().expect("missing queue URL");

    // Send the same message twice with the same deduplication ID
    let first = client
        .send_message()
        .queue_url(url)
        .message_body("unique-message")
        .message_group_id("g1")
        .message_deduplication_id("same-dedup")
        .send()
        .await
        .expect("SendMessage (1) failed");

    let second = client
        .send_message()
        .queue_url(url)
        .message_body("unique-message")
        .message_group_id("g1")
        .message_deduplication_id("same-dedup")
        .send()
        .await
        .expect("SendMessage (2) failed");

    // FIFO deduplication: the second send should return the same message ID
    assert_eq!(
        first.message_id().unwrap_or_default(),
        second.message_id().unwrap_or_default(),
        "deduplicated FIFO message should have the same message ID"
    );

    // Only one message should be in the queue
    let result = client
        .receive_message()
        .queue_url(url)
        .max_number_of_messages(10)
        .send()
        .await
        .expect("ReceiveMessage failed");

    assert_eq!(
        result.messages().len(),
        1,
        "deduplicated queue should have exactly 1 message, got {}",
        result.messages().len()
    );
}
