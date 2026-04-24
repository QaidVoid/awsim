//! Advanced integration tests using the official AWS SDK for Rust.
//!
//! These tests cover additional services and operations not covered by sdk_tests.rs.
//! Each test uses an independent server with fresh state.

use std::sync::Arc;

use aws_credential_types::Credentials;
use aws_sdk_dynamodb::types::{
    AttributeDefinition, AttributeValue, BillingMode, KeySchemaElement, KeyType, KeysAndAttributes,
    PutRequest, ScalarAttributeType, WriteRequest,
};
use aws_sdk_kms::primitives::Blob;
use aws_sdk_kms::types::DataKeySpec;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_sqs::types::MessageAttributeValue;
use awsim_core::{AppState, ServiceHandler};

// ---------------------------------------------------------------------------
// Server bootstrap helpers (duplicated from sdk_tests.rs for isolation)
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

    let iam = Arc::new(awsim_iam::IamService::new());
    state.register(iam, vec![]);

    let sts = Arc::new(awsim_sts::StsService::new());
    state.register(sts, vec![]);

    let sns = Arc::new(awsim_sns::SnsService::new());
    state.register(sns, vec![]);

    let sqs = Arc::new(awsim_sqs::SqsService::new());
    state.register(sqs, vec![]);

    let dynamodb = Arc::new(awsim_dynamodb::DynamoDbService::new());
    state.register(dynamodb, vec![]);

    let s3 = awsim_s3::S3Service::new();
    let s3_routes = s3.routes();
    state.register(Arc::new(s3), s3_routes);

    let kms = Arc::new(awsim_kms::KmsService::new());
    state.register(kms, vec![]);

    let secrets = Arc::new(awsim_secretsmanager::SecretsManagerService::new());
    state.register(secrets, vec![]);

    let logs = Arc::new(awsim_cloudwatch_logs::CloudWatchLogsService::new());
    state.register(logs, vec![]);

    let ssm = Arc::new(awsim_ssm::SsmService::new());
    state.register(ssm, vec![]);

    let lambda = awsim_lambda::LambdaService::new();
    let lambda_routes = lambda.routes();
    state.register(Arc::new(lambda), lambda_routes);

    let app = axum::Router::new()
        .route("/_awsim/health", axum::routing::get(|| async { "ok" }))
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
// STS
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sts_assume_role() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_sts::Client::new(&config);

    let role_arn = "arn:aws:iam::000000000000:role/TestRole";

    let result = client
        .assume_role()
        .role_arn(role_arn)
        .role_session_name("test-session")
        .send()
        .await
        .expect("AssumeRole failed");

    let creds = result.credentials().expect("missing credentials");
    let access_key_id = creds.access_key_id();
    assert!(
        access_key_id.starts_with("ASIA"),
        "temporary credentials should have ASIA prefix, got: {access_key_id}"
    );
    assert!(
        !creds.secret_access_key().is_empty(),
        "secret access key should not be empty"
    );
    assert!(
        !creds.session_token().is_empty(),
        "session token should not be empty"
    );

    let assumed_user = result
        .assumed_role_user()
        .expect("missing assumed role user");
    assert!(
        assumed_user.arn().contains("TestRole"),
        "ARN should contain role name"
    );
}

// ---------------------------------------------------------------------------
// DynamoDB
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_dynamodb_batch_write_and_get() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_dynamodb::Client::new(&config);

    // Create table
    client
        .create_table()
        .table_name("batch-table")
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
        .await
        .expect("CreateTable failed");

    // BatchWriteItem with 5 items
    let write_requests: Vec<WriteRequest> = (1..=5)
        .map(|i| {
            WriteRequest::builder()
                .put_request(
                    PutRequest::builder()
                        .item("id", AttributeValue::S(format!("item-{i}")))
                        .item("value", AttributeValue::N(i.to_string()))
                        .build()
                        .unwrap(),
                )
                .build()
        })
        .collect();

    let batch_write_result = client
        .batch_write_item()
        .request_items("batch-table", write_requests)
        .send()
        .await
        .expect("BatchWriteItem failed");

    let unprocessed = batch_write_result
        .unprocessed_items()
        .as_ref()
        .and_then(|m| m.get("batch-table"))
        .map(|v| v.len())
        .unwrap_or(0);
    assert_eq!(unprocessed, 0, "all items should be processed");

    // BatchGetItem for 3 of the 5 items
    let keys_to_get: Vec<std::collections::HashMap<String, AttributeValue>> = (1..=3)
        .map(|i| {
            let mut k = std::collections::HashMap::new();
            k.insert("id".to_string(), AttributeValue::S(format!("item-{i}")));
            k
        })
        .collect();

    let batch_get_result = client
        .batch_get_item()
        .request_items(
            "batch-table",
            KeysAndAttributes::builder()
                .set_keys(Some(keys_to_get))
                .build()
                .unwrap(),
        )
        .send()
        .await
        .expect("BatchGetItem failed");

    let responses = batch_get_result.responses();
    let items = responses
        .as_ref()
        .and_then(|m| m.get("batch-table"))
        .expect("missing batch-table in responses");
    assert_eq!(items.len(), 3, "should retrieve exactly 3 items");
}

#[tokio::test]
async fn test_dynamodb_update_item_expression() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_dynamodb::Client::new(&config);

    client
        .create_table()
        .table_name("update-table")
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("pk")
                .key_type(KeyType::Hash)
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
        .billing_mode(BillingMode::PayPerRequest)
        .send()
        .await
        .expect("CreateTable failed");

    // Put initial item
    client
        .put_item()
        .table_name("update-table")
        .item("pk", AttributeValue::S("row-1".into()))
        .item("name", AttributeValue::S("Alice".into()))
        .item("score", AttributeValue::N("10".into()))
        .send()
        .await
        .expect("PutItem failed");

    // UpdateItem with SET expression
    client
        .update_item()
        .table_name("update-table")
        .key("pk", AttributeValue::S("row-1".into()))
        .update_expression("SET #n = :name, score = :score")
        .expression_attribute_names("#n", "name")
        .expression_attribute_values(":name", AttributeValue::S("Bob".into()))
        .expression_attribute_values(":score", AttributeValue::N("99".into()))
        .send()
        .await
        .expect("UpdateItem failed");

    // Verify the update
    let result = client
        .get_item()
        .table_name("update-table")
        .key("pk", AttributeValue::S("row-1".into()))
        .send()
        .await
        .expect("GetItem failed");

    let item = result.item().expect("item should exist after update");
    assert_eq!(
        item.get("name")
            .and_then(|v| v.as_s().ok())
            .map(|s| s.as_str()),
        Some("Bob"),
        "name should be updated to Bob"
    );
    assert_eq!(
        item.get("score")
            .and_then(|v| v.as_n().ok())
            .map(|s| s.as_str()),
        Some("99"),
        "score should be updated to 99"
    );
}

// ---------------------------------------------------------------------------
// S3
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_s3_list_objects_with_prefix() {
    let endpoint = start_server().await;
    let client = make_s3_client(&endpoint).await;

    client
        .create_bucket()
        .bucket("prefix-bucket")
        .send()
        .await
        .expect("CreateBucket failed");

    // Put objects in different "folders"
    for key in [
        "images/cat.jpg",
        "images/dog.jpg",
        "docs/readme.txt",
        "docs/notes.txt",
        "root.txt",
    ] {
        client
            .put_object()
            .bucket("prefix-bucket")
            .key(key)
            .body(ByteStream::from_static(b"data"))
            .send()
            .await
            .expect("PutObject failed");
    }

    // List only images/
    let result = client
        .list_objects_v2()
        .bucket("prefix-bucket")
        .prefix("images/")
        .send()
        .await
        .expect("ListObjectsV2 failed");

    assert_eq!(
        result.key_count().unwrap_or(0),
        2,
        "should find exactly 2 images"
    );

    let keys: Vec<&str> = result.contents().iter().filter_map(|o| o.key()).collect();
    assert!(
        keys.contains(&"images/cat.jpg"),
        "cat.jpg should be in results"
    );
    assert!(
        keys.contains(&"images/dog.jpg"),
        "dog.jpg should be in results"
    );

    // List only docs/
    let docs_result = client
        .list_objects_v2()
        .bucket("prefix-bucket")
        .prefix("docs/")
        .send()
        .await
        .expect("ListObjectsV2 with docs prefix failed");

    assert_eq!(
        docs_result.key_count().unwrap_or(0),
        2,
        "should find 2 docs"
    );
}

#[tokio::test]
async fn test_s3_copy_object() {
    let endpoint = start_server().await;
    let client = make_s3_client(&endpoint).await;

    client
        .create_bucket()
        .bucket("copy-source")
        .send()
        .await
        .expect("CreateBucket (source) failed");

    client
        .create_bucket()
        .bucket("copy-dest")
        .send()
        .await
        .expect("CreateBucket (dest) failed");

    let original_body = b"Original content for copy test";

    client
        .put_object()
        .bucket("copy-source")
        .key("original.txt")
        .body(ByteStream::from_static(original_body))
        .send()
        .await
        .expect("PutObject failed");

    // CopyObject to same bucket with new key
    client
        .copy_object()
        .copy_source("copy-source/original.txt")
        .bucket("copy-dest")
        .key("copy.txt")
        .send()
        .await
        .expect("CopyObject failed");

    // Verify the copy exists and has the same content
    let result = client
        .get_object()
        .bucket("copy-dest")
        .key("copy.txt")
        .send()
        .await
        .expect("GetObject (copy) failed");

    let body = result
        .body
        .collect()
        .await
        .expect("collecting body failed")
        .into_bytes();
    assert_eq!(
        body.as_ref(),
        original_body,
        "copy should have same content as original"
    );

    // Original should still exist
    client
        .get_object()
        .bucket("copy-source")
        .key("original.txt")
        .send()
        .await
        .expect("original should still exist after copy");
}

// ---------------------------------------------------------------------------
// SQS
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sqs_message_attributes() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_sqs::Client::new(&config);

    let queue = client
        .create_queue()
        .queue_name("attr-queue")
        .send()
        .await
        .expect("CreateQueue failed");
    let queue_url = queue.queue_url().expect("missing queue URL");

    // SendMessage with MessageAttributes
    client
        .send_message()
        .queue_url(queue_url)
        .message_body("Message with attributes")
        .message_attributes(
            "event-type",
            MessageAttributeValue::builder()
                .data_type("String")
                .string_value("order-created")
                .build()
                .unwrap(),
        )
        .message_attributes(
            "order-id",
            MessageAttributeValue::builder()
                .data_type("String")
                .string_value("ORD-12345")
                .build()
                .unwrap(),
        )
        .message_attributes(
            "amount",
            MessageAttributeValue::builder()
                .data_type("Number")
                .string_value("149.99")
                .build()
                .unwrap(),
        )
        .send()
        .await
        .expect("SendMessage failed");

    // Receive message with message attributes
    let result = client
        .receive_message()
        .queue_url(queue_url)
        .max_number_of_messages(1)
        .message_attribute_names("All")
        .send()
        .await
        .expect("ReceiveMessage failed");

    let messages = result.messages();
    assert_eq!(messages.len(), 1, "expected 1 message");

    let msg = &messages[0];
    assert_eq!(msg.body().unwrap_or_default(), "Message with attributes");

    let attrs = msg
        .message_attributes()
        .expect("message attributes should be present");
    assert!(
        attrs.contains_key("event-type"),
        "event-type attribute missing"
    );
    assert!(attrs.contains_key("order-id"), "order-id attribute missing");
    assert!(attrs.contains_key("amount"), "amount attribute missing");

    assert_eq!(
        attrs["event-type"].string_value(),
        Some("order-created"),
        "event-type value mismatch"
    );
    assert_eq!(
        attrs["order-id"].string_value(),
        Some("ORD-12345"),
        "order-id value mismatch"
    );
    assert_eq!(
        attrs["amount"].string_value(),
        Some("149.99"),
        "amount value mismatch"
    );
}

// ---------------------------------------------------------------------------
// SNS
// ---------------------------------------------------------------------------

/// SNS ListSubscriptions uses AwsQuery list serialization and requires <member> wrappers.
/// This is a known limitation of the AwsQuery XML serializer.
#[ignore = "AwsQuery list serialisation does not yet emit <member> wrappers required by the SNS SDK"]
#[tokio::test]
async fn test_sns_subscribe_and_list() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let sns_client = aws_sdk_sns::Client::new(&config);
    let sqs_client = aws_sdk_sqs::Client::new(&config);

    // Create an SQS queue for SNS to deliver to
    let queue = sqs_client
        .create_queue()
        .queue_name("sns-delivery-queue")
        .send()
        .await
        .expect("CreateQueue failed");
    let queue_url = queue.queue_url().expect("missing queue URL");
    let queue_arn = format!(
        "arn:aws:sqs:us-east-1:000000000000:{}",
        "sns-delivery-queue"
    );

    // Create SNS topic
    let topic = sns_client
        .create_topic()
        .name("list-sub-topic")
        .send()
        .await
        .expect("CreateTopic failed");
    let topic_arn = topic.topic_arn().expect("missing topic ARN");

    // Subscribe SQS to topic
    let sub_result = sns_client
        .subscribe()
        .topic_arn(topic_arn)
        .protocol("sqs")
        .endpoint(&queue_arn)
        .send()
        .await
        .expect("Subscribe failed");

    let sub_arn = sub_result
        .subscription_arn()
        .expect("missing subscription ARN");
    assert!(
        sub_arn.contains("list-sub-topic"),
        "subscription ARN should contain topic name"
    );

    // ListSubscriptions
    let list_result = sns_client
        .list_subscriptions()
        .send()
        .await
        .expect("ListSubscriptions failed");

    assert!(
        !list_result.subscriptions().is_empty(),
        "should have at least one subscription"
    );

    let _ = queue_url;
}

// ---------------------------------------------------------------------------
// Secrets Manager
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_secrets_manager_versioning() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_secretsmanager::Client::new(&config);

    // CreateSecret with initial value
    let created = client
        .create_secret()
        .name("versioned-secret")
        .secret_string("version-1-value")
        .send()
        .await
        .expect("CreateSecret failed");

    let _arn = created.arn().expect("missing ARN");

    // Get initial version
    let v1 = client
        .get_secret_value()
        .secret_id("versioned-secret")
        .send()
        .await
        .expect("GetSecretValue (v1) failed");
    assert_eq!(
        v1.secret_string().unwrap_or_default(),
        "version-1-value",
        "initial version should match"
    );

    // PutSecretValue to create a new version
    client
        .put_secret_value()
        .secret_id("versioned-secret")
        .secret_string("version-2-value")
        .send()
        .await
        .expect("PutSecretValue failed");

    // GetSecretValue should now return the latest version
    let v2 = client
        .get_secret_value()
        .secret_id("versioned-secret")
        .send()
        .await
        .expect("GetSecretValue (v2) failed");
    assert_eq!(
        v2.secret_string().unwrap_or_default(),
        "version-2-value",
        "after PutSecretValue, latest should be v2"
    );

    // Version IDs should differ
    let v1_id = v1.version_id().unwrap_or_default();
    let v2_id = v2.version_id().unwrap_or_default();
    assert_ne!(v1_id, v2_id, "version IDs should differ between versions");
}

// ---------------------------------------------------------------------------
// KMS
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_kms_generate_data_key() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_kms::Client::new(&config);

    // Create a KMS key
    let key = client.create_key().send().await.expect("CreateKey failed");
    let key_id = key
        .key_metadata()
        .expect("missing key metadata")
        .key_id()
        .to_string();

    // Generate data key
    let result = client
        .generate_data_key()
        .key_id(&key_id)
        .key_spec(DataKeySpec::Aes256)
        .send()
        .await
        .expect("GenerateDataKey failed");

    let plaintext_dek = result
        .plaintext()
        .expect("plaintext DEK should be returned");
    let encrypted_dek = result
        .ciphertext_blob()
        .expect("encrypted DEK should be returned");

    // AES-256 key should be 32 bytes
    assert_eq!(
        plaintext_dek.as_ref().len(),
        32,
        "AES-256 plaintext DEK should be 32 bytes"
    );
    assert!(
        !encrypted_dek.as_ref().is_empty(),
        "encrypted DEK should not be empty"
    );

    // Decrypt the encrypted DEK to verify round-trip
    let decrypt_result = client
        .decrypt()
        .ciphertext_blob(Blob::new(encrypted_dek.as_ref().to_vec()))
        .send()
        .await
        .expect("Decrypt of encrypted DEK failed");

    assert_eq!(
        decrypt_result
            .plaintext()
            .expect("missing plaintext after decrypt")
            .as_ref(),
        plaintext_dek.as_ref(),
        "decrypted DEK should match original plaintext DEK"
    );
}
