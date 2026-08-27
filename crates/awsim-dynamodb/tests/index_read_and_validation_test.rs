//! Index reads must show what AWS shows, hold memory proportional to the page
//! they return, and validate a key the same way on every write path.
//!
//! The failures these pin were all silent: a COUNT scan that streamed a whole
//! table into one response, a Scan that swept the base table when handed an
//! index name that does not exist, an LSI cursor that our own validator then
//! rejected, and a BatchWriteItem that returned success for a write it
//! dropped.

use awsim_core::{AwsError, RequestContext, ServiceHandler};
use awsim_dynamodb::DynamoDbService;
use serde_json::{Value, json};

fn ctx() -> RequestContext {
    RequestContext::new("dynamodb", "us-east-1")
}

fn err(res: &Result<Value, AwsError>) -> &AwsError {
    match res {
        Err(e) => e,
        Ok(v) => panic!("expected an error, got {v}"),
    }
}

fn is_validation(res: &Result<Value, AwsError>) -> bool {
    matches!(res, Err(e) if e.code == "ValidationException")
}

/// Base table `T` with a GSI `G1` (hash `gsi_pk`, range `gsi_sk`) and an LSI
/// `L1` (range `lsi_sk`, KEYS_ONLY).
async fn service() -> DynamoDbService {
    let svc = DynamoDbService::new();
    svc.handle(
        "CreateTable",
        json!({
            "TableName": "T",
            "KeySchema": [
                { "AttributeName": "pk", "KeyType": "HASH" },
                { "AttributeName": "sk", "KeyType": "RANGE" },
            ],
            "AttributeDefinitions": [
                { "AttributeName": "pk", "AttributeType": "S" },
                { "AttributeName": "sk", "AttributeType": "S" },
                { "AttributeName": "gsi_pk", "AttributeType": "S" },
                { "AttributeName": "gsi_sk", "AttributeType": "S" },
                { "AttributeName": "lsi_sk", "AttributeType": "S" },
            ],
            "GlobalSecondaryIndexes": [{
                "IndexName": "G1",
                "KeySchema": [
                    { "AttributeName": "gsi_pk", "KeyType": "HASH" },
                    { "AttributeName": "gsi_sk", "KeyType": "RANGE" },
                ],
                "Projection": { "ProjectionType": "ALL" },
            }],
            "LocalSecondaryIndexes": [{
                "IndexName": "L1",
                "KeySchema": [
                    { "AttributeName": "pk", "KeyType": "HASH" },
                    { "AttributeName": "lsi_sk", "KeyType": "RANGE" },
                ],
                "Projection": { "ProjectionType": "KEYS_ONLY" },
            }],
            "BillingMode": "PAY_PER_REQUEST",
        }),
        &ctx(),
    )
    .await
    .expect("CreateTable");
    svc
}

async fn put(svc: &DynamoDbService, item: Value) {
    svc.handle("PutItem", json!({ "TableName": "T", "Item": item }), &ctx())
        .await
        .expect("PutItem");
}

// --- Unknown index name -------------------------------------------------

#[tokio::test]
async fn scan_rejects_unknown_index_name() {
    let svc = service().await;
    put(&svc, json!({ "pk": { "S": "a" }, "sk": { "S": "1" } })).await;

    let res = svc
        .handle(
            "Scan",
            json!({ "TableName": "T", "IndexName": "typo" }),
            &ctx(),
        )
        .await;
    assert!(
        is_validation(&res),
        "Scan with an unknown IndexName must not fall back to the base table: {res:?}"
    );
    assert!(
        err(&res)
            .message
            .contains("does not have the specified index"),
        "got {:?}",
        err(&res).message
    );
}

#[tokio::test]
async fn query_and_scan_agree_on_unknown_index_name() {
    let svc = service().await;
    let scan = svc
        .handle(
            "Scan",
            json!({ "TableName": "T", "IndexName": "typo" }),
            &ctx(),
        )
        .await;
    let query = svc
        .handle(
            "Query",
            json!({
                "TableName": "T",
                "IndexName": "typo",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": { "S": "a" } },
            }),
            &ctx(),
        )
        .await;
    assert_eq!(err(&scan).message, err(&query).message);
}

// --- Sparse LSI membership ---------------------------------------------

#[tokio::test]
async fn query_on_lsi_skips_items_missing_the_index_range_key() {
    let svc = service().await;
    put(
        &svc,
        json!({ "pk": { "S": "a" }, "sk": { "S": "1" }, "lsi_sk": { "S": "x" } }),
    )
    .await;
    // No lsi_sk, so this item is not in L1 at all.
    put(&svc, json!({ "pk": { "S": "a" }, "sk": { "S": "2" } })).await;

    let res = svc
        .handle(
            "Query",
            json!({
                "TableName": "T",
                "IndexName": "L1",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": { "S": "a" } },
            }),
            &ctx(),
        )
        .await
        .expect("Query");
    assert_eq!(res["Count"], 1, "only the item carrying lsi_sk is in L1");
}

#[tokio::test]
async fn query_and_scan_agree_on_lsi_membership() {
    let svc = service().await;
    put(
        &svc,
        json!({ "pk": { "S": "a" }, "sk": { "S": "1" }, "lsi_sk": { "S": "x" } }),
    )
    .await;
    put(&svc, json!({ "pk": { "S": "a" }, "sk": { "S": "2" } })).await;

    let scan = svc
        .handle(
            "Scan",
            json!({ "TableName": "T", "IndexName": "L1" }),
            &ctx(),
        )
        .await
        .expect("Scan");
    let query = svc
        .handle(
            "Query",
            json!({
                "TableName": "T",
                "IndexName": "L1",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": { "S": "a" } },
            }),
            &ctx(),
        )
        .await
        .expect("Query");
    assert_eq!(scan["Count"], query["Count"]);
}

// --- Cursor round-trip --------------------------------------------------

/// Page a read to completion, returning every item seen.
async fn page_all(svc: &DynamoDbService, op: &str, mut input: Value) -> Vec<Value> {
    let mut seen = Vec::new();
    loop {
        let res = svc
            .handle(op, input.clone(), &ctx())
            .await
            .unwrap_or_else(|e| panic!("{op} paging failed: {e:?}"));
        if let Some(items) = res["Items"].as_array() {
            seen.extend(items.iter().cloned());
        }
        match res.get("LastEvaluatedKey") {
            Some(lek) if !lek.is_null() => {
                input["ExclusiveStartKey"] = lek.clone();
            }
            _ => return seen,
        }
    }
}

#[tokio::test]
async fn lsi_cursor_round_trips() {
    let svc = service().await;
    for i in 0..5 {
        put(
            &svc,
            json!({
                "pk": { "S": "a" },
                "sk": { "S": i.to_string() },
                "lsi_sk": { "S": format!("x{i}") },
            }),
        )
        .await;
    }

    let seen = page_all(
        &svc,
        "Query",
        json!({
            "TableName": "T",
            "IndexName": "L1",
            "KeyConditionExpression": "pk = :p",
            "ExpressionAttributeValues": { ":p": { "S": "a" } },
            "Limit": 1,
        }),
    )
    .await;
    assert_eq!(
        seen.len(),
        5,
        "every item appears exactly once across pages"
    );
}

#[tokio::test]
async fn gsi_and_base_cursors_round_trip() {
    let svc = service().await;
    for i in 0..5 {
        put(
            &svc,
            json!({
                "pk": { "S": "a" },
                "sk": { "S": i.to_string() },
                "gsi_pk": { "S": "g" },
                "gsi_sk": { "S": i.to_string() },
            }),
        )
        .await;
    }

    let base = page_all(
        &svc,
        "Query",
        json!({
            "TableName": "T",
            "KeyConditionExpression": "pk = :p",
            "ExpressionAttributeValues": { ":p": { "S": "a" } },
            "Limit": 1,
        }),
    )
    .await;
    assert_eq!(base.len(), 5, "base table pagination");

    let gsi = page_all(
        &svc,
        "Query",
        json!({
            "TableName": "T",
            "IndexName": "G1",
            "KeyConditionExpression": "gsi_pk = :p",
            "ExpressionAttributeValues": { ":p": { "S": "g" } },
            "Limit": 1,
        }),
    )
    .await;
    assert_eq!(gsi.len(), 5, "GSI pagination");

    let scan = page_all(&svc, "Scan", json!({ "TableName": "T", "Limit": 1 })).await;
    assert_eq!(scan.len(), 5, "Scan pagination");
}

#[tokio::test]
async fn cursor_reports_an_index_key_violation_like_a_write_does() {
    let svc = service().await;

    let from_write = svc
        .handle(
            "PutItem",
            json!({
                "TableName": "T",
                "Item": {
                    "pk": { "S": "a" },
                    "sk": { "S": "1" },
                    "gsi_pk": { "S": "" },
                    "gsi_sk": { "S": "z" },
                },
            }),
            &ctx(),
        )
        .await;

    let from_cursor = svc
        .handle(
            "Query",
            json!({
                "TableName": "T",
                "IndexName": "G1",
                "KeyConditionExpression": "gsi_pk = :p",
                "ExpressionAttributeValues": { ":p": { "S": "g" } },
                "ExclusiveStartKey": {
                    "gsi_pk": { "S": "" },
                    "gsi_sk": { "S": "z" },
                    "pk": { "S": "a" },
                    "sk": { "S": "1" },
                },
            }),
            &ctx(),
        )
        .await;

    assert!(is_validation(&from_write), "{from_write:?}");
    assert!(is_validation(&from_cursor), "{from_cursor:?}");
    assert_eq!(
        err(&from_write).message,
        err(&from_cursor).message,
        "one violation, one message"
    );
    assert!(
        err(&from_cursor).message.contains("IndexName: G1"),
        "cursor error must name the index: {}",
        err(&from_cursor).message
    );
}

// --- Select -------------------------------------------------------------

#[tokio::test]
async fn all_projected_attributes_requires_an_index() {
    let svc = service().await;
    let res = svc
        .handle(
            "Scan",
            json!({ "TableName": "T", "Select": "ALL_PROJECTED_ATTRIBUTES" }),
            &ctx(),
        )
        .await;
    assert!(is_validation(&res), "{res:?}");
}

#[tokio::test]
async fn specific_attributes_requires_a_projection() {
    let svc = service().await;
    let res = svc
        .handle(
            "Scan",
            json!({ "TableName": "T", "Select": "SPECIFIC_ATTRIBUTES" }),
            &ctx(),
        )
        .await;
    assert!(is_validation(&res), "{res:?}");
}

#[tokio::test]
async fn unknown_select_value_is_rejected() {
    let svc = service().await;
    let res = svc
        .handle(
            "Scan",
            json!({ "TableName": "T", "Select": "EVERYTHING" }),
            &ctx(),
        )
        .await;
    assert!(is_validation(&res), "{res:?}");
}

#[tokio::test]
async fn all_projected_attributes_narrows_a_keys_only_lsi() {
    let svc = service().await;
    put(
        &svc,
        json!({
            "pk": { "S": "a" },
            "sk": { "S": "1" },
            "lsi_sk": { "S": "x" },
            "payload": { "S": "not projected" },
        }),
    )
    .await;

    let res = svc
        .handle(
            "Query",
            json!({
                "TableName": "T",
                "IndexName": "L1",
                "Select": "ALL_PROJECTED_ATTRIBUTES",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": { "S": "a" } },
            }),
            &ctx(),
        )
        .await
        .expect("Query");

    let item = &res["Items"][0];
    assert!(
        item.get("payload").is_none(),
        "a KEYS_ONLY LSI does not project payload: {item}"
    );
    assert_eq!(item["pk"]["S"], "a");
    assert_eq!(item["lsi_sk"]["S"], "x");
}

#[tokio::test]
async fn lsi_filter_still_sees_non_projected_attributes() {
    // An LSI lives in the base item's partition, so AWS fetches a
    // non-projected attribute back from the base item rather than hiding it.
    // The index view and the Select projection are separate concerns.
    let svc = service().await;
    put(
        &svc,
        json!({
            "pk": { "S": "a" },
            "sk": { "S": "1" },
            "lsi_sk": { "S": "x" },
            "payload": { "S": "match me" },
        }),
    )
    .await;

    let res = svc
        .handle(
            "Query",
            json!({
                "TableName": "T",
                "IndexName": "L1",
                "KeyConditionExpression": "pk = :p",
                "FilterExpression": "payload = :v",
                "ExpressionAttributeValues": {
                    ":p": { "S": "a" },
                    ":v": { "S": "match me" },
                },
            }),
            &ctx(),
        )
        .await
        .expect("Query");
    assert_eq!(res["Count"], 1, "LSI filter reads through to the base item");
}

// --- COUNT --------------------------------------------------------------

#[tokio::test]
async fn count_response_carries_no_items() {
    let svc = service().await;
    put(&svc, json!({ "pk": { "S": "a" }, "sk": { "S": "1" } })).await;
    put(&svc, json!({ "pk": { "S": "a" }, "sk": { "S": "2" } })).await;

    for op in ["Scan", "Query"] {
        let input = if op == "Scan" {
            json!({ "TableName": "T", "Select": "COUNT" })
        } else {
            json!({
                "TableName": "T",
                "Select": "COUNT",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": { "S": "a" } },
            })
        };
        let res = svc.handle(op, input, &ctx()).await.expect(op);
        assert_eq!(res["Count"], 2, "{op}");
        assert_eq!(res["ScannedCount"], 2, "{op}");
        assert!(
            res.get("Items").is_none(),
            "{op} with Select=COUNT must not return Items: {res}"
        );
    }
}

#[tokio::test]
async fn count_is_accurate_under_a_filter() {
    let svc = service().await;
    put(
        &svc,
        json!({ "pk": { "S": "a" }, "sk": { "S": "1" }, "keep": { "S": "y" } }),
    )
    .await;
    put(&svc, json!({ "pk": { "S": "a" }, "sk": { "S": "2" } })).await;

    let res = svc
        .handle(
            "Scan",
            json!({
                "TableName": "T",
                "Select": "COUNT",
                "FilterExpression": "keep = :v",
                "ExpressionAttributeValues": { ":v": { "S": "y" } },
            }),
            &ctx(),
        )
        .await
        .expect("Scan");
    assert_eq!(res["Count"], 1, "matches");
    assert_eq!(res["ScannedCount"], 2, "examined");
}

/// Fill the table with more than 1 MiB of items.
async fn fill_past_the_page_cap(svc: &DynamoDbService) -> usize {
    const ITEMS: usize = 150;
    let blob = "x".repeat(10_000);
    for i in 0..ITEMS {
        put(
            svc,
            json!({
                "pk": { "S": "a" },
                "sk": { "S": format!("{i:04}") },
                "blob": { "S": blob },
            }),
        )
        .await;
    }
    ITEMS
}

#[tokio::test]
async fn count_scan_paginates_at_the_page_cap() {
    let svc = service().await;
    let total = fill_past_the_page_cap(&svc).await;

    let res = svc
        .handle(
            "Scan",
            json!({ "TableName": "T", "Select": "COUNT" }),
            &ctx(),
        )
        .await
        .expect("Scan");

    assert!(
        res.get("LastEvaluatedKey").is_some(),
        "a COUNT scan over more than 1 MiB must paginate, got {res}"
    );
    let first = res["Count"].as_u64().expect("Count") as usize;
    assert!(
        first < total,
        "the first page must stop short of the whole table: {first} of {total}"
    );
}

#[tokio::test]
async fn paging_a_count_scan_reaches_the_full_count() {
    let svc = service().await;
    let total = fill_past_the_page_cap(&svc).await;

    let mut input = json!({ "TableName": "T", "Select": "COUNT" });
    let mut counted = 0usize;
    let mut pages = 0;
    loop {
        let res = svc
            .handle("Scan", input.clone(), &ctx())
            .await
            .expect("Scan");
        counted += res["Count"].as_u64().expect("Count") as usize;
        pages += 1;
        assert!(pages < 100, "pagination should converge");
        match res.get("LastEvaluatedKey") {
            Some(lek) if !lek.is_null() => input["ExclusiveStartKey"] = lek.clone(),
            _ => break,
        }
    }
    assert!(pages > 1, "the fixture must actually span several pages");
    assert_eq!(counted, total, "counts across pages sum to the table");
}

// --- Key validation coverage -------------------------------------------

#[tokio::test]
async fn transact_update_validates_index_keys() {
    let svc = service().await;
    put(&svc, json!({ "pk": { "S": "a" }, "sk": { "S": "1" } })).await;

    let res = svc
        .handle(
            "TransactWriteItems",
            json!({
                "TransactItems": [{
                    "Update": {
                        "TableName": "T",
                        "Key": { "pk": { "S": "a" }, "sk": { "S": "1" } },
                        "UpdateExpression": "SET gsi_pk = :v",
                        "ExpressionAttributeValues": { ":v": { "S": "" } },
                    }
                }]
            }),
            &ctx(),
        )
        .await;
    assert!(
        is_validation(&res),
        "a transactional update must not smuggle an empty index key: {res:?}"
    );

    let after = svc
        .handle(
            "GetItem",
            json!({ "TableName": "T", "Key": { "pk": { "S": "a" }, "sk": { "S": "1" } } }),
            &ctx(),
        )
        .await
        .expect("GetItem");
    assert!(
        after["Item"].get("gsi_pk").is_none(),
        "the rejected transaction must not have written: {after}"
    );
}

#[tokio::test]
async fn update_item_and_transact_update_agree() {
    let svc = service().await;
    put(&svc, json!({ "pk": { "S": "a" }, "sk": { "S": "1" } })).await;

    let direct = svc
        .handle(
            "UpdateItem",
            json!({
                "TableName": "T",
                "Key": { "pk": { "S": "a" }, "sk": { "S": "1" } },
                "UpdateExpression": "SET gsi_pk = :v",
                "ExpressionAttributeValues": { ":v": { "S": "" } },
            }),
            &ctx(),
        )
        .await;
    let transactional = svc
        .handle(
            "TransactWriteItems",
            json!({
                "TransactItems": [{
                    "Update": {
                        "TableName": "T",
                        "Key": { "pk": { "S": "a" }, "sk": { "S": "1" } },
                        "UpdateExpression": "SET gsi_pk = :v",
                        "ExpressionAttributeValues": { ":v": { "S": "" } },
                    }
                }]
            }),
            &ctx(),
        )
        .await;
    assert_eq!(err(&direct).message, err(&transactional).message);
}

#[tokio::test]
async fn partiql_insert_validates_index_keys() {
    let svc = service().await;
    let oversized = "y".repeat(2000);
    let res = svc
        .handle(
            "ExecuteStatement",
            json!({
                "Statement": format!(
                    "INSERT INTO \"T\" VALUE {{'pk': 'a', 'sk': '1', 'gsi_sk': '{oversized}'}}"
                ),
            }),
            &ctx(),
        )
        .await;
    assert!(
        is_validation(&res),
        "PartiQL INSERT must reject an oversized index key: {res:?}"
    );
}

#[tokio::test]
async fn batch_write_rejects_a_put_missing_the_partition_key() {
    let svc = service().await;
    let res = svc
        .handle(
            "BatchWriteItem",
            json!({
                "RequestItems": {
                    "T": [{ "PutRequest": { "Item": { "sk": { "S": "1" } } } }]
                }
            }),
            &ctx(),
        )
        .await;
    assert!(
        is_validation(&res),
        "a put with no partition key must not be silently dropped: {res:?}"
    );
}

#[tokio::test]
async fn batch_write_rejects_a_delete_missing_the_partition_key() {
    let svc = service().await;
    let res = svc
        .handle(
            "BatchWriteItem",
            json!({
                "RequestItems": {
                    "T": [{ "DeleteRequest": { "Key": { "sk": { "S": "1" } } } }]
                }
            }),
            &ctx(),
        )
        .await;
    assert!(is_validation(&res), "{res:?}");
}

#[tokio::test]
async fn a_well_formed_batch_still_writes_every_item() {
    let svc = service().await;
    let res = svc
        .handle(
            "BatchWriteItem",
            json!({
                "RequestItems": {
                    "T": [
                        { "PutRequest": { "Item": { "pk": { "S": "a" }, "sk": { "S": "1" } } } },
                        { "PutRequest": { "Item": { "pk": { "S": "a" }, "sk": { "S": "2" } } } },
                    ]
                }
            }),
            &ctx(),
        )
        .await
        .expect("BatchWriteItem");
    assert!(
        res["UnprocessedItems"]
            .as_object()
            .is_none_or(|m| m.is_empty()),
        "nothing should be unprocessed: {res}"
    );

    let scan = svc
        .handle("Scan", json!({ "TableName": "T" }), &ctx())
        .await
        .expect("Scan");
    assert_eq!(scan["Count"], 2);
}

#[tokio::test]
async fn a_non_scalar_bound_to_an_index_key_is_a_type_error() {
    let svc = service().await;
    let res = svc
        .handle(
            "PutItem",
            json!({
                "TableName": "T",
                "Item": {
                    "pk": { "S": "a" },
                    "sk": { "S": "1" },
                    "gsi_sk": { "L": [{ "S": "one" }, { "S": "two" }] },
                },
            }),
            &ctx(),
        )
        .await;
    assert!(is_validation(&res), "{res:?}");
    let msg = &err(&res).message;
    assert!(
        !msg.contains("maximum size limit"),
        "a list is the wrong type, not the wrong size: {msg}"
    );
}

#[tokio::test]
async fn an_oversized_scalar_index_key_is_still_a_size_error() {
    let svc = service().await;
    let res = svc
        .handle(
            "PutItem",
            json!({
                "TableName": "T",
                "Item": {
                    "pk": { "S": "a" },
                    "sk": { "S": "1" },
                    "gsi_sk": { "S": "y".repeat(2000) },
                },
            }),
            &ctx(),
        )
        .await;
    assert!(is_validation(&res), "{res:?}");
    assert!(
        err(&res).message.contains("maximum size limit"),
        "got {:?}",
        err(&res).message
    );
}

// --- Streams ------------------------------------------------------------

async fn service_with_stream(view_type: &str) -> DynamoDbService {
    let svc = DynamoDbService::new();
    svc.handle(
        "CreateTable",
        json!({
            "TableName": "S",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "StreamSpecification": { "StreamEnabled": true, "StreamViewType": view_type },
            "BillingMode": "PAY_PER_REQUEST",
        }),
        &ctx(),
    )
    .await
    .expect("CreateTable");
    svc
}

/// The size a stream record reports for one write.
async fn stream_record_size(view_type: &str) -> u64 {
    let svc = service_with_stream(view_type).await;
    svc.handle(
        "PutItem",
        json!({
            "TableName": "S",
            "Item": { "pk": { "S": "a" }, "payload": { "S": "x".repeat(500) } },
        }),
        &ctx(),
    )
    .await
    .expect("PutItem");

    let desc = svc
        .handle("DescribeTable", json!({ "TableName": "S" }), &ctx())
        .await
        .expect("DescribeTable");
    let arn = desc["Table"]["LatestStreamArn"]
        .as_str()
        .expect("LatestStreamArn");

    let shards = svc
        .handle("DescribeStream", json!({ "StreamArn": arn }), &ctx())
        .await
        .expect("DescribeStream");
    let shard_id = shards["StreamDescription"]["Shards"][0]["ShardId"]
        .as_str()
        .expect("ShardId");

    let iter = svc
        .handle(
            "GetShardIterator",
            json!({
                "StreamArn": arn,
                "ShardId": shard_id,
                "ShardIteratorType": "TRIM_HORIZON",
            }),
            &ctx(),
        )
        .await
        .expect("GetShardIterator");

    let records = svc
        .handle(
            "GetRecords",
            json!({ "ShardIterator": iter["ShardIterator"] }),
            &ctx(),
        )
        .await
        .expect("GetRecords");

    records["Records"][0]["dynamodb"]["SizeBytes"]
        .as_u64()
        .expect("SizeBytes")
}

#[tokio::test]
async fn stream_record_size_reflects_what_the_record_carries() {
    let keys_only = stream_record_size("KEYS_ONLY").await;
    let with_image = stream_record_size("NEW_IMAGE").await;

    assert!(keys_only > 0, "a record always carries its keys");
    assert!(
        with_image > keys_only + 500,
        "NEW_IMAGE carries the 500-byte payload that KEYS_ONLY drops: \
         {with_image} vs {keys_only}"
    );
}

// --- Limit --------------------------------------------------------------

#[tokio::test]
async fn limit_accepts_a_whole_valued_float() {
    let svc = service().await;
    put(&svc, json!({ "pk": { "S": "a" }, "sk": { "S": "1" } })).await;
    put(&svc, json!({ "pk": { "S": "a" }, "sk": { "S": "2" } })).await;

    let res = svc
        .handle("Scan", json!({ "TableName": "T", "Limit": 1.0 }), &ctx())
        .await
        .expect("Limit 1.0 is a limit of 1");
    assert_eq!(res["Count"], 1);
}

#[tokio::test]
async fn limit_rejects_zero_negative_and_fractional() {
    let svc = service().await;
    for bad in [json!(0), json!(-1), json!(1.5)] {
        let res = svc
            .handle(
                "Scan",
                json!({ "TableName": "T", "Limit": bad.clone() }),
                &ctx(),
            )
            .await;
        assert!(
            is_validation(&res),
            "Limit {bad} should be rejected: {res:?}"
        );
    }
}
