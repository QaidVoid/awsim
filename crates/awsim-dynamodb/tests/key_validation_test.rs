//! DynamoDB rejects a Key whose element types disagree with the table's
//! declared AttributeDefinitions. Returning an empty result instead is the
//! worst possible failure for an emulator: a schema error reads as "item
//! not found", so a test asserting absence passes locally and the same
//! code misbehaves against real AWS.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_dynamodb::DynamoDbService;
use serde_json::{Value, json};

fn ctx() -> RequestContext {
    RequestContext::new("dynamodb", "us-east-1")
}

async fn service_with_table() -> DynamoDbService {
    let svc = DynamoDbService::new();
    svc.handle(
        "CreateTable",
        json!({
            "TableName": "T",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "BillingMode": "PAY_PER_REQUEST",
        }),
        &ctx(),
    )
    .await
    .expect("CreateTable");
    svc
}

fn is_validation(res: &Result<Value, awsim_core::AwsError>) -> bool {
    matches!(res, Err(e) if e.code == "ValidationException")
}

#[tokio::test]
async fn get_item_rejects_mismatched_key_type() {
    let svc = service_with_table().await;
    let res = svc
        .handle(
            "GetItem",
            json!({ "TableName": "T", "Key": { "pk": { "N": "1" } } }),
            &ctx(),
        )
        .await;
    assert!(
        is_validation(&res),
        "N supplied for an S-typed key should be a ValidationException, got {res:?}"
    );
}

#[tokio::test]
async fn get_item_missing_item_is_still_an_empty_success() {
    // The mismatch error must stay distinguishable from a genuine miss.
    let svc = service_with_table().await;
    let res = svc
        .handle(
            "GetItem",
            json!({ "TableName": "T", "Key": { "pk": { "S": "absent" } } }),
            &ctx(),
        )
        .await
        .expect("correctly typed key for a missing item should succeed");
    assert!(
        res.get("Item").is_none(),
        "missing item should return no Item, got {res:?}"
    );
}

#[tokio::test]
async fn get_item_rejects_missing_key_element() {
    let svc = service_with_table().await;
    let res = svc
        .handle("GetItem", json!({ "TableName": "T", "Key": {} }), &ctx())
        .await;
    assert!(is_validation(&res), "empty Key should be rejected: {res:?}");
}

#[tokio::test]
async fn get_item_rejects_non_key_attribute_in_key() {
    let svc = service_with_table().await;
    let res = svc
        .handle(
            "GetItem",
            json!({
                "TableName": "T",
                "Key": { "pk": { "S": "a" }, "extra": { "S": "b" } },
            }),
            &ctx(),
        )
        .await;
    assert!(
        is_validation(&res),
        "a non-key attribute in Key should be rejected: {res:?}"
    );
}

#[tokio::test]
async fn delete_item_rejects_mismatched_key_type() {
    let svc = service_with_table().await;
    let res = svc
        .handle(
            "DeleteItem",
            json!({ "TableName": "T", "Key": { "pk": { "N": "1" } } }),
            &ctx(),
        )
        .await;
    assert!(is_validation(&res), "got {res:?}");
}

#[tokio::test]
async fn update_item_rejects_mismatched_key_type() {
    let svc = service_with_table().await;
    let res = svc
        .handle(
            "UpdateItem",
            json!({
                "TableName": "T",
                "Key": { "pk": { "N": "1" } },
                "UpdateExpression": "SET #v = :v",
                "ExpressionAttributeNames": { "#v": "v" },
                "ExpressionAttributeValues": { ":v": { "S": "x" } },
            }),
            &ctx(),
        )
        .await;
    assert!(is_validation(&res), "got {res:?}");
}

#[tokio::test]
async fn batch_get_item_rejects_mismatched_key_type() {
    let svc = service_with_table().await;
    let res = svc
        .handle(
            "BatchGetItem",
            json!({
                "RequestItems": {
                    "T": { "Keys": [{ "pk": { "N": "1" } }] }
                }
            }),
            &ctx(),
        )
        .await;
    assert!(is_validation(&res), "got {res:?}");
}

#[tokio::test]
async fn transact_get_items_rejects_mismatched_key_type() {
    let svc = service_with_table().await;
    let res = svc
        .handle(
            "TransactGetItems",
            json!({
                "TransactItems": [
                    { "Get": { "TableName": "T", "Key": { "pk": { "N": "1" } } } }
                ]
            }),
            &ctx(),
        )
        .await;
    assert!(is_validation(&res), "got {res:?}");
}

#[tokio::test]
async fn correctly_typed_keys_still_work_end_to_end() {
    let svc = service_with_table().await;
    svc.handle(
        "PutItem",
        json!({ "TableName": "T", "Item": { "pk": { "S": "a" }, "v": { "S": "1" } } }),
        &ctx(),
    )
    .await
    .expect("PutItem");

    let got = svc
        .handle(
            "GetItem",
            json!({ "TableName": "T", "Key": { "pk": { "S": "a" } } }),
            &ctx(),
        )
        .await
        .expect("GetItem");
    assert_eq!(got["Item"]["v"]["S"], "1");

    svc.handle(
        "DeleteItem",
        json!({ "TableName": "T", "Key": { "pk": { "S": "a" } } }),
        &ctx(),
    )
    .await
    .expect("DeleteItem");
}
