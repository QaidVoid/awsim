//! Behavior contract tests: drive the real AWS SDK / REST clients against a
//! live in-process AWSim and assert behavior, not just envelope shape. These
//! cover parity bugs that the envelope-only coverage runner cannot catch.

use aws_sdk_dynamodb::types::{
    AttributeDefinition, AttributeValue, BillingMode, KeySchemaElement, KeyType,
    ScalarAttributeType,
};
use aws_smithy_types::error::metadata::ProvideErrorMetadata;
use serde_json::{Value, json};

#[tokio::test]
async fn dynamodb_query_filter_on_key_is_rejected_and_items_round_trip() {
    let endpoint = awsim_conformance::server::start().await;
    let config = awsim_conformance::runner::common::make_config(&endpoint).await;
    let client = aws_sdk_dynamodb::Client::new(&config);

    client
        .create_table()
        .table_name("t")
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
        .expect("create table");

    client
        .put_item()
        .table_name("t")
        .item("id", AttributeValue::S("a".into()))
        .item("name", AttributeValue::S("hi".into()))
        .send()
        .await
        .expect("put item");

    let got = client
        .get_item()
        .table_name("t")
        .key("id", AttributeValue::S("a".into()))
        .send()
        .await
        .expect("get item");
    let name = got.item().and_then(|m| m.get("name"));
    assert!(
        matches!(name, Some(AttributeValue::S(s)) if s == "hi"),
        "round-trip mismatch: {name:?}"
    );

    let err = client
        .query()
        .table_name("t")
        .key_condition_expression("#id = :id")
        .filter_expression("begins_with(#id, :p)")
        .expression_attribute_names("#id", "id")
        .expression_attribute_values(":id", AttributeValue::S("a".into()))
        .expression_attribute_values(":p", AttributeValue::S("a".into()))
        .send()
        .await
        .expect_err("query with a key attribute in FilterExpression must be rejected");
    assert_eq!(err.code(), Some("ValidationException"), "got: {err:?}");
}

#[tokio::test]
async fn cognito_management_op_enforces_authorization() {
    use awsim_conformance::{runner::common::make_config_with_key, server};

    let endpoint = server::start_iam_enforced().await;

    // Admin key bypasses authz; use it to mint a low-privilege IAM user.
    let admin = make_config_with_key(&endpoint, server::ADMIN_ACCESS_KEY, "x").await;
    let iam = aws_sdk_iam::Client::new(&admin);
    iam.create_user()
        .user_name("lowpriv")
        .send()
        .await
        .expect("create user");
    let key = iam
        .create_access_key()
        .user_name("lowpriv")
        .send()
        .await
        .expect("create access key");
    let ak = key.access_key().expect("access key in response");
    let low_id = ak.access_key_id().to_string();
    let low_secret = ak.secret_access_key().to_string();

    // Admin bypass: a management op succeeds.
    let cog_admin = aws_sdk_cognitoidentityprovider::Client::new(&admin);
    cog_admin
        .list_user_pools()
        .max_results(10)
        .send()
        .await
        .expect("admin ListUserPools");

    // Low-privilege principal with no policy: denied.
    let low = make_config_with_key(&endpoint, &low_id, &low_secret).await;
    let denied = aws_sdk_cognitoidentityprovider::Client::new(&low)
        .list_user_pools()
        .max_results(10)
        .send()
        .await
        .expect_err("non-admin must be denied a management op");
    assert_eq!(denied.code(), Some("AccessDenied"), "got: {denied:?}");

    // Unknown key: invalid token, and the key must not leak into the message.
    let bogus = "AKIACONFORMANCEUNKNOWNKEY";
    let bad = make_config_with_key(&endpoint, bogus, "x").await;
    let invalid = aws_sdk_cognitoidentityprovider::Client::new(&bad)
        .list_user_pools()
        .max_results(10)
        .send()
        .await
        .expect_err("unknown key must be rejected");
    assert_eq!(
        invalid.code(),
        Some("InvalidClientTokenId"),
        "got: {invalid:?}"
    );
    assert!(
        !invalid.message().unwrap_or_default().contains(bogus),
        "error message must not leak the access key: {:?}",
        invalid.message()
    );
}

#[tokio::test]
async fn opensearch_reindex_honors_source_query() {
    let endpoint = awsim_conformance::server::start_opensearch().await;
    let http = reqwest::Client::new();

    http.put(format!("{endpoint}/legacy"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();

    let docs = [
        ("a", json!({ "tenantId": "t1", "datasetId": "d1" })),
        ("b", json!({ "tenantId": "t1", "datasetId": "d2" })),
        ("c", json!({ "tenantId": "t1" })),
    ];
    for (id, body) in &docs {
        http.put(format!("{endpoint}/legacy/_doc/{id}"))
            .json(body)
            .send()
            .await
            .unwrap();
    }

    let resp = http
        .post(format!("{endpoint}/_reindex"))
        .json(&json!({
            "source": {
                "index": "legacy",
                "query": { "bool": { "must": [
                    { "term": { "tenantId": "t1" } },
                    { "term": { "datasetId": "d1" } },
                ] } },
            },
            "dest": { "index": "dest" },
        }))
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["created"], json!(1), "reindex response: {body}");

    let count: Value = http
        .post(format!("{endpoint}/dest/_count"))
        .json(&json!({ "query": { "match_all": {} } }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        count["count"],
        json!(1),
        "only the matching doc should copy"
    );
}
