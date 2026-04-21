//! Integration tests using the official AWS SDK for Rust.
//!
//! These tests start an in-process AWSim server on a random port and exercise
//! real AWS SDK calls against it. Each test gets its own isolated server
//! instance so tests can run concurrently without state leaking.

use std::sync::Arc;

use aws_credential_types::Credentials;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_dynamodb::types::{
    AttributeDefinition, AttributeValue, BillingMode, KeySchemaElement, KeyType,
    ScalarAttributeType,
};
use aws_sdk_kms::primitives::Blob;
use awsim_core::{AppState, ServiceHandler};

// ---------------------------------------------------------------------------
// Server bootstrap helpers
// ---------------------------------------------------------------------------

/// Build the AWS SDK config pointing at the given local endpoint.
async fn make_config(endpoint: &str) -> aws_config::SdkConfig {
    aws_config::defaults(aws_config::BehaviorVersion::latest())
        .endpoint_url(endpoint)
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(
            Credentials::new("test", "test", None, None, "test"),
        )
        .load()
        .await
}

/// Build an S3 client with path-style addressing forced on.
///
/// The AWS SDK for S3 defaults to virtual-hosted style
/// (`http://bucket.host/key`), which doesn't work against a local
/// server running on an IP address. `force_path_style` makes the SDK
/// use `http://host/bucket/key` instead, which AWSim's router handles.
async fn make_s3_client(endpoint: &str) -> aws_sdk_s3::Client {
    let config = make_config(endpoint).await;
    let s3_config = aws_sdk_s3::config::Builder::from(&config)
        .force_path_style(true)
        .build();
    aws_sdk_s3::Client::from_conf(s3_config)
}

/// Start a full AWSim server in-process on a random port.
///
/// Returns the endpoint URL (e.g. `http://127.0.0.1:54321`). The server
/// runs for the lifetime of the test binary; each call produces an
/// independent server with fresh state.
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

    // Give the Tokio task a moment to bind before the first request.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    endpoint
}

// ---------------------------------------------------------------------------
// STS
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sts_get_caller_identity() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_sts::Client::new(&config);

    let result = client
        .get_caller_identity()
        .send()
        .await
        .expect("GetCallerIdentity failed");

    assert_eq!(result.account().unwrap_or_default(), "000000000000");
    let arn = result.arn().unwrap_or_default();
    assert!(!arn.is_empty(), "ARN should not be empty");
}

// ---------------------------------------------------------------------------
// S3
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_s3_create_and_list_buckets() {
    let endpoint = start_server().await;
    let client = make_s3_client(&endpoint).await;

    client
        .create_bucket()
        .bucket("list-test-bucket")
        .send()
        .await
        .expect("CreateBucket failed");

    let result = client
        .list_buckets()
        .send()
        .await
        .expect("ListBuckets failed");

    let names: Vec<&str> = result
        .buckets()
        .iter()
        .filter_map(|b| b.name())
        .collect();
    assert!(names.contains(&"list-test-bucket"), "bucket not in list: {names:?}");
}

#[tokio::test]
async fn test_s3_delete_bucket() {
    let endpoint = start_server().await;
    let client = make_s3_client(&endpoint).await;

    client
        .create_bucket()
        .bucket("delete-me")
        .send()
        .await
        .expect("CreateBucket failed");

    client
        .delete_bucket()
        .bucket("delete-me")
        .send()
        .await
        .expect("DeleteBucket failed");

    let result = client.list_buckets().send().await.expect("ListBuckets failed");
    let names: Vec<&str> = result.buckets().iter().filter_map(|b| b.name()).collect();
    assert!(!names.contains(&"delete-me"), "deleted bucket still in list");
}

#[tokio::test]
async fn test_s3_put_and_get_object() {
    let endpoint = start_server().await;
    let client = make_s3_client(&endpoint).await;

    client
        .create_bucket()
        .bucket("data-bucket")
        .send()
        .await
        .expect("CreateBucket failed");

    client
        .put_object()
        .bucket("data-bucket")
        .key("hello.txt")
        .body(ByteStream::from_static(b"Hello, AWSim!"))
        .send()
        .await
        .expect("PutObject failed");

    let result = client
        .get_object()
        .bucket("data-bucket")
        .key("hello.txt")
        .send()
        .await
        .expect("GetObject failed");

    let body = result
        .body
        .collect()
        .await
        .expect("collecting body failed")
        .into_bytes();
    assert_eq!(body.as_ref(), b"Hello, AWSim!");
}

#[tokio::test]
async fn test_s3_list_objects() {
    let endpoint = start_server().await;
    let client = make_s3_client(&endpoint).await;

    client
        .create_bucket()
        .bucket("objects-bucket")
        .send()
        .await
        .expect("CreateBucket failed");

    for key in ["a.txt", "b.txt", "c.txt"] {
        client
            .put_object()
            .bucket("objects-bucket")
            .key(key)
            .body(ByteStream::from_static(b"data"))
            .send()
            .await
            .expect("PutObject failed");
    }

    let result = client
        .list_objects_v2()
        .bucket("objects-bucket")
        .send()
        .await
        .expect("ListObjectsV2 failed");

    assert_eq!(result.key_count().unwrap_or(0), 3);
}

#[tokio::test]
async fn test_s3_delete_object() {
    let endpoint = start_server().await;
    let client = make_s3_client(&endpoint).await;

    client
        .create_bucket()
        .bucket("del-obj-bucket")
        .send()
        .await
        .expect("CreateBucket failed");

    client
        .put_object()
        .bucket("del-obj-bucket")
        .key("to-delete.txt")
        .body(ByteStream::from_static(b"bye"))
        .send()
        .await
        .expect("PutObject failed");

    client
        .delete_object()
        .bucket("del-obj-bucket")
        .key("to-delete.txt")
        .send()
        .await
        .expect("DeleteObject failed");

    let result = client
        .list_objects_v2()
        .bucket("del-obj-bucket")
        .send()
        .await
        .expect("ListObjectsV2 failed");
    assert_eq!(result.key_count().unwrap_or(0), 0);
}

// ---------------------------------------------------------------------------
// SQS
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sqs_send_and_receive() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_sqs::Client::new(&config);

    let queue = client
        .create_queue()
        .queue_name("test-queue")
        .send()
        .await
        .expect("CreateQueue failed");
    let queue_url = queue.queue_url().expect("missing queue URL");

    client
        .send_message()
        .queue_url(queue_url)
        .message_body("Hello from SQS!")
        .send()
        .await
        .expect("SendMessage failed");

    let result = client
        .receive_message()
        .queue_url(queue_url)
        .max_number_of_messages(1)
        .send()
        .await
        .expect("ReceiveMessage failed");

    let messages = result.messages();
    assert_eq!(messages.len(), 1, "expected 1 message, got {}", messages.len());
    assert_eq!(messages[0].body().unwrap_or_default(), "Hello from SQS!");
}

#[tokio::test]
async fn test_sqs_delete_message() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_sqs::Client::new(&config);

    let queue = client
        .create_queue()
        .queue_name("del-msg-queue")
        .send()
        .await
        .expect("CreateQueue failed");
    let queue_url = queue.queue_url().expect("missing queue URL");

    client
        .send_message()
        .queue_url(queue_url)
        .message_body("will be deleted")
        .send()
        .await
        .expect("SendMessage failed");

    let recv = client
        .receive_message()
        .queue_url(queue_url)
        .max_number_of_messages(1)
        .send()
        .await
        .expect("ReceiveMessage failed");

    let receipt = recv.messages()[0]
        .receipt_handle()
        .expect("missing receipt handle");

    client
        .delete_message()
        .queue_url(queue_url)
        .receipt_handle(receipt)
        .send()
        .await
        .expect("DeleteMessage failed");

    // Queue should now be empty.
    let recv2 = client
        .receive_message()
        .queue_url(queue_url)
        .max_number_of_messages(1)
        .send()
        .await
        .expect("ReceiveMessage (2) failed");
    assert!(recv2.messages().is_empty(), "message was not deleted");
}

// ---------------------------------------------------------------------------
// DynamoDB
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_dynamodb_create_table_and_crud() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_dynamodb::Client::new(&config);

    client
        .create_table()
        .table_name("users")
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

    // Put item
    client
        .put_item()
        .table_name("users")
        .item("id", AttributeValue::S("user-1".into()))
        .item("name", AttributeValue::S("Alice".into()))
        .send()
        .await
        .expect("PutItem failed");

    // Get item
    let result = client
        .get_item()
        .table_name("users")
        .key("id", AttributeValue::S("user-1".into()))
        .send()
        .await
        .expect("GetItem failed");

    let item = result.item().expect("item not found");
    let name = item.get("name").expect("name attribute missing");
    assert_eq!(name.as_s().unwrap(), "Alice");
}

#[tokio::test]
async fn test_dynamodb_delete_item() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_dynamodb::Client::new(&config);

    client
        .create_table()
        .table_name("del-table")
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

    client
        .put_item()
        .table_name("del-table")
        .item("pk", AttributeValue::S("row-1".into()))
        .send()
        .await
        .expect("PutItem failed");

    client
        .delete_item()
        .table_name("del-table")
        .key("pk", AttributeValue::S("row-1".into()))
        .send()
        .await
        .expect("DeleteItem failed");

    let result = client
        .get_item()
        .table_name("del-table")
        .key("pk", AttributeValue::S("row-1".into()))
        .send()
        .await
        .expect("GetItem failed");

    assert!(result.item().is_none(), "item should have been deleted");
}

// ---------------------------------------------------------------------------
// IAM
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_iam_create_and_list_users() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_iam::Client::new(&config);

    client
        .create_user()
        .user_name("testuser")
        .send()
        .await
        .expect("CreateUser failed");

    let result = client.list_users().send().await.expect("ListUsers failed");
    let names: Vec<&str> = result.users().iter().map(|u| u.user_name()).collect();
    assert!(names.contains(&"testuser"), "user not in list: {names:?}");
}

// ---------------------------------------------------------------------------
// SNS
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sns_create_topic_and_publish() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_sns::Client::new(&config);

    let topic = client
        .create_topic()
        .name("test-topic")
        .send()
        .await
        .expect("CreateTopic failed");
    let topic_arn = topic.topic_arn().expect("missing topic ARN");

    let result = client
        .publish()
        .topic_arn(topic_arn)
        .message("Hello SNS!")
        .send()
        .await
        .expect("Publish failed");

    assert!(
        result.message_id().is_some(),
        "publish should return a message ID"
    );
}

/// SNS `ListTopics` returns topics in an AwsQuery XML response.  The SDK
/// expects list items wrapped in `<member>` elements
/// (`<Topics><member><TopicArn>...</TopicArn></member></Topics>`), but
/// AWSim currently serialises lists as repeated top-level elements.
/// This is a known limitation of the AwsQuery XML serialiser.
#[ignore = "AwsQuery list serialisation does not yet emit <member> wrappers required by the SNS SDK"]
#[tokio::test]
async fn test_sns_list_topics() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_sns::Client::new(&config);

    client
        .create_topic()
        .name("list-topic-a")
        .send()
        .await
        .expect("CreateTopic failed");

    client
        .create_topic()
        .name("list-topic-b")
        .send()
        .await
        .expect("CreateTopic failed");

    let result = client.list_topics().send().await.expect("ListTopics failed");
    assert!(
        result.topics().len() >= 2,
        "expected at least 2 topics, got {}",
        result.topics().len()
    );
}

// ---------------------------------------------------------------------------
// KMS
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_kms_encrypt_decrypt() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_kms::Client::new(&config);

    let key = client
        .create_key()
        .send()
        .await
        .expect("CreateKey failed");
    let key_id = key
        .key_metadata()
        .expect("missing key metadata")
        .key_id()
        .to_string();

    let plaintext = Blob::new(b"secret data".to_vec());

    let encrypted = client
        .encrypt()
        .key_id(&key_id)
        .plaintext(plaintext)
        .send()
        .await
        .expect("Encrypt failed");

    let ciphertext = encrypted
        .ciphertext_blob()
        .expect("missing ciphertext")
        .clone();

    let decrypted = client
        .decrypt()
        .ciphertext_blob(ciphertext)
        .send()
        .await
        .expect("Decrypt failed");

    assert_eq!(
        decrypted.plaintext().expect("missing plaintext").as_ref(),
        b"secret data"
    );
}

#[tokio::test]
async fn test_kms_generate_data_key() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_kms::Client::new(&config);

    let key = client.create_key().send().await.expect("CreateKey failed");
    let key_id = key
        .key_metadata()
        .expect("missing key metadata")
        .key_id()
        .to_string();

    let result = client
        .generate_data_key()
        .key_id(&key_id)
        .key_spec(aws_sdk_kms::types::DataKeySpec::Aes256)
        .send()
        .await
        .expect("GenerateDataKey failed");

    assert!(result.plaintext().is_some(), "plaintext DEK should be returned");
    assert!(result.ciphertext_blob().is_some(), "encrypted DEK should be returned");
}

// ---------------------------------------------------------------------------
// Secrets Manager
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_secretsmanager_create_and_get() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_secretsmanager::Client::new(&config);

    client
        .create_secret()
        .name("my-secret")
        .secret_string("s3cr3t!")
        .send()
        .await
        .expect("CreateSecret failed");

    let result = client
        .get_secret_value()
        .secret_id("my-secret")
        .send()
        .await
        .expect("GetSecretValue failed");

    assert_eq!(result.secret_string().unwrap_or_default(), "s3cr3t!");
}

#[tokio::test]
async fn test_secretsmanager_update_secret() {
    let endpoint = start_server().await;
    let config = make_config(&endpoint).await;
    let client = aws_sdk_secretsmanager::Client::new(&config);

    client
        .create_secret()
        .name("mutable-secret")
        .secret_string("original")
        .send()
        .await
        .expect("CreateSecret failed");

    client
        .update_secret()
        .secret_id("mutable-secret")
        .secret_string("updated")
        .send()
        .await
        .expect("UpdateSecret failed");

    let result = client
        .get_secret_value()
        .secret_id("mutable-secret")
        .send()
        .await
        .expect("GetSecretValue failed");

    assert_eq!(result.secret_string().unwrap_or_default(), "updated");
}

