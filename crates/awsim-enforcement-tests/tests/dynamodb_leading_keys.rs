//! `dynamodb:LeadingKeys` in an assumed-role session policy, the way a
//! per-tenant credential broker scopes DynamoDB access: the role may do
//! anything, and the session policy narrows each table statement to the
//! caller's tenant partitions.

use std::collections::HashMap;
use std::sync::Arc;

use aws_config::SdkConfig;
use aws_credential_types::Credentials;
use aws_sdk_dynamodb::Client;
use aws_sdk_dynamodb::types::{
    AttributeDefinition, AttributeValue, BillingMode, GlobalSecondaryIndex, KeySchemaElement,
    KeyType, KeysAndAttributes, Projection, ProjectionType, Put, ScalarAttributeType,
    TransactWriteItem,
};
use awsim_enforcement_tests::{
    ServerHandle, bootstrap_user, iam_client, make_sdk_config, sdk_err_is_access_denied,
    start_server_unenforced,
};

const TABLE: &str = "app_core_x";
const INDEX: &str = "gsi1";

const TRUST_ANYONE: &str = r#"{
  "Version": "2012-10-17",
  "Statement": [{"Effect": "Allow", "Principal": {"AWS": "*"}, "Action": "sts:AssumeRole"}]
}"#;

const ROLE_ALLOWS_ALL_DYNAMODB: &str = r#"{
  "Version": "2012-10-17",
  "Statement": [{"Effect": "Allow", "Action": "dynamodb:*", "Resource": "*"}]
}"#;

/// The session policy for tenant `acme`, with `actions` on the table and
/// its indexes.
fn tenant_session_policy(actions: &[&str]) -> String {
    serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Action": actions,
            "Resource": [
                format!("arn:aws:dynamodb:*:*:table/{TABLE}"),
                format!("arn:aws:dynamodb:*:*:table/{TABLE}/index/*"),
            ],
            "Condition": {"ForAllValues:StringLike": {
                "dynamodb:LeadingKeys": ["TENANT#acme", "TENANT#acme#*"]
            }}
        }]
    })
    .to_string()
}

const ITEM_ACTIONS: &[&str] = &[
    "dynamodb:GetItem",
    "dynamodb:BatchGetItem",
    "dynamodb:Query",
    "dynamodb:PutItem",
    "dynamodb:UpdateItem",
    "dynamodb:DeleteItem",
    "dynamodb:BatchWriteItem",
];

struct Fixture {
    srv: ServerHandle,
    port: u16,
    caller: SdkConfig,
}

/// A server with a seeded table, a role allowed all of DynamoDB, and IAM
/// enforcement switched on.
async fn setup() -> Fixture {
    let iam = Arc::new(awsim_iam::IamService::new());
    let (srv, port) = start_server_unenforced(iam).await;
    let admin = make_sdk_config(port, "admin", "admin");
    let (caller_ak, caller_sk) = bootstrap_user(&iam_client(&admin), "broker", &[]).await;

    let admin_iam = iam_client(&admin);
    admin_iam
        .create_role()
        .role_name("TenantRole")
        .assume_role_policy_document(TRUST_ANYONE)
        .send()
        .await
        .unwrap();
    admin_iam
        .put_role_policy()
        .role_name("TenantRole")
        .policy_name("AllDynamoDb")
        .policy_document(ROLE_ALLOWS_ALL_DYNAMODB)
        .send()
        .await
        .unwrap();

    let ddb = Client::new(&admin);
    let attr = |name: &str| {
        AttributeDefinition::builder()
            .attribute_name(name)
            .attribute_type(ScalarAttributeType::S)
            .build()
            .unwrap()
    };
    let key = |name: &str, key_type: KeyType| {
        KeySchemaElement::builder()
            .attribute_name(name)
            .key_type(key_type)
            .build()
            .unwrap()
    };
    ddb.create_table()
        .table_name(TABLE)
        .billing_mode(BillingMode::PayPerRequest)
        .attribute_definitions(attr("pk"))
        .attribute_definitions(attr("sk"))
        .attribute_definitions(attr("gsi1pk"))
        .key_schema(key("pk", KeyType::Hash))
        .key_schema(key("sk", KeyType::Range))
        .global_secondary_indexes(
            GlobalSecondaryIndex::builder()
                .index_name(INDEX)
                .key_schema(key("gsi1pk", KeyType::Hash))
                .projection(
                    Projection::builder()
                        .projection_type(ProjectionType::All)
                        .build(),
                )
                .build()
                .unwrap(),
        )
        .send()
        .await
        .unwrap();
    for (pk, gsi1pk) in [
        ("TENANT#acme", "TENANT#acme#users"),
        ("TENANT#globex", "USER#x"),
        ("TENANT#acme2#x", "TENANT#acme2#users"),
    ] {
        ddb.put_item()
            .table_name(TABLE)
            .set_item(Some(item(pk, "1", Some(gsi1pk))))
            .send()
            .await
            .unwrap();
    }

    srv.set_enforcement(true);
    Fixture {
        srv,
        port,
        caller: make_sdk_config(port, &caller_ak, &caller_sk),
    }
}

impl Fixture {
    /// A DynamoDB client signed with credentials from AssumeRole with
    /// `policy` as the inline session policy.
    async fn session(&self, policy: &str) -> Client {
        let creds = aws_sdk_sts::Client::new(&self.caller)
            .assume_role()
            .role_arn("arn:aws:iam::000000000000:role/TenantRole")
            .role_session_name("acme-user")
            .policy(policy)
            .send()
            .await
            .expect("AssumeRole")
            .credentials
            .expect("Credentials");
        let cfg = aws_config::SdkConfig::builder()
            .behavior_version(aws_config::BehaviorVersion::latest())
            .endpoint_url(format!("http://127.0.0.1:{}", self.port))
            .region(aws_config::Region::new("us-east-1"))
            .credentials_provider(
                aws_credential_types::provider::SharedCredentialsProvider::new(Credentials::new(
                    creds.access_key_id,
                    creds.secret_access_key,
                    Some(creds.session_token),
                    None,
                    "test",
                )),
            )
            .build();
        Client::new(&cfg)
    }
}

fn item(pk: &str, sk: &str, gsi1pk: Option<&str>) -> HashMap<String, AttributeValue> {
    let mut item = HashMap::from([
        ("pk".to_string(), AttributeValue::S(pk.to_string())),
        ("sk".to_string(), AttributeValue::S(sk.to_string())),
    ]);
    if let Some(gsi1pk) = gsi1pk {
        item.insert("gsi1pk".to_string(), AttributeValue::S(gsi1pk.to_string()));
    }
    item
}

fn key(pk: &str) -> HashMap<String, AttributeValue> {
    item(pk, "1", None)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_leading_keys_allow_own_tenant_items() {
    let fx = setup().await;
    let ddb = fx.session(&tenant_session_policy(ITEM_ACTIONS)).await;

    let got = ddb
        .get_item()
        .table_name(TABLE)
        .set_key(Some(key("TENANT#acme")))
        .send()
        .await
        .expect("GetItem on the tenant's own partition");
    assert!(got.item.is_some());

    ddb.put_item()
        .table_name(TABLE)
        .set_item(Some(item("TENANT#acme#project#1", "1", None)))
        .send()
        .await
        .expect("PutItem under the tenant's prefix");

    let queried = ddb
        .query()
        .table_name(TABLE)
        .key_condition_expression("pk = :pk")
        .expression_attribute_values(":pk", AttributeValue::S("TENANT#acme".into()))
        .send()
        .await
        .expect("Query on the tenant's partition");
    assert_eq!(queried.count, 1);

    let queried = ddb
        .query()
        .table_name(TABLE)
        .index_name(INDEX)
        .key_condition_expression("#pk = :pk")
        .expression_attribute_names("#pk", "gsi1pk")
        .expression_attribute_values(":pk", AttributeValue::S("TENANT#acme#users".into()))
        .send()
        .await
        .expect("Query on an index partition under the tenant's prefix");
    assert_eq!(queried.count, 1);

    let batch = ddb
        .batch_get_item()
        .request_items(
            TABLE,
            KeysAndAttributes::builder()
                .keys(key("TENANT#acme"))
                .keys(key("TENANT#acme#project#1"))
                .build()
                .unwrap(),
        )
        .send()
        .await
        .expect("BatchGetItem on the tenant's partitions only");
    assert_eq!(batch.responses.unwrap()[TABLE].len(), 2);

    fx.srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_leading_keys_deny_other_tenant_items() {
    let fx = setup().await;
    let ddb = fx.session(&tenant_session_policy(ITEM_ACTIONS)).await;

    let err = ddb
        .get_item()
        .table_name(TABLE)
        .set_key(Some(key("TENANT#globex")))
        .send()
        .await
        .expect_err("GetItem on another tenant's partition");
    assert!(sdk_err_is_access_denied(&err), "{err:?}");

    let err = ddb
        .put_item()
        .table_name(TABLE)
        .set_item(Some(item("TENANT#globex#project#1", "1", None)))
        .send()
        .await
        .expect_err("PutItem under another tenant's prefix");
    assert!(sdk_err_is_access_denied(&err), "{err:?}");

    // The index's partition key is what counts on an index Query, not the
    // base table's, so a tenant cannot read a non-tenant index partition.
    let err = ddb
        .query()
        .table_name(TABLE)
        .index_name(INDEX)
        .key_condition_expression("gsi1pk = :pk")
        .expression_attribute_values(":pk", AttributeValue::S("USER#x".into()))
        .send()
        .await
        .expect_err("Query on an index partition outside the tenant");
    assert!(sdk_err_is_access_denied(&err), "{err:?}");

    // One disallowed key denies the whole batch.
    let err = ddb
        .batch_get_item()
        .request_items(
            TABLE,
            KeysAndAttributes::builder()
                .keys(key("TENANT#acme"))
                .keys(key("TENANT#globex"))
                .build()
                .unwrap(),
        )
        .send()
        .await
        .expect_err("BatchGetItem mixing tenants");
    assert!(sdk_err_is_access_denied(&err), "{err:?}");

    let put = |pk: &str| {
        TransactWriteItem::builder()
            .put(
                Put::builder()
                    .table_name(TABLE)
                    .set_item(Some(item(pk, "2", None)))
                    .build()
                    .unwrap(),
            )
            .build()
    };
    let err = ddb
        .transact_write_items()
        .transact_items(put("TENANT#acme"))
        .transact_items(put("TENANT#globex"))
        .send()
        .await
        .expect_err("TransactWriteItems mixing tenants");
    assert!(sdk_err_is_access_denied(&err), "{err:?}");

    fx.srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_leading_keys_tenant_prefix_does_not_match_longer_tenant_id() {
    let fx = setup().await;
    let ddb = fx.session(&tenant_session_policy(ITEM_ACTIONS)).await;

    let err = ddb
        .get_item()
        .table_name(TABLE)
        .set_key(Some(key("TENANT#acme2#x")))
        .send()
        .await
        .expect_err("TENANT#acme#* must not match TENANT#acme2#x");
    assert!(sdk_err_is_access_denied(&err), "{err:?}");

    fx.srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_leading_keys_scan_is_denied_unless_granted() {
    let fx = setup().await;

    let ddb = fx.session(&tenant_session_policy(ITEM_ACTIONS)).await;
    let err = ddb
        .scan()
        .table_name(TABLE)
        .send()
        .await
        .expect_err("Scan is not among the granted actions");
    assert!(sdk_err_is_access_denied(&err), "{err:?}");

    // A Scan carries no leading keys, and ForAllValues over an empty set
    // is true, so granting Scan alongside the condition exposes every
    // tenant's items. AWS behaves the same way and warns against it.
    let mut actions = ITEM_ACTIONS.to_vec();
    actions.push("dynamodb:Scan");
    let ddb = fx.session(&tenant_session_policy(&actions)).await;
    let scanned = ddb
        .scan()
        .table_name(TABLE)
        .send()
        .await
        .expect("granted Scan passes the ForAllValues condition");
    let tenants: Vec<&str> = scanned
        .items()
        .iter()
        .filter_map(|i| i.get("pk").and_then(|v| v.as_s().ok()))
        .map(String::as_str)
        .collect();
    assert!(tenants.contains(&"TENANT#globex"), "{tenants:?}");

    fx.srv.shutdown().await;
}
