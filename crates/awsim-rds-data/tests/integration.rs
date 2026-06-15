//! End-to-end tests for the RDS Data API against a real PostgreSQL.
//!
//! These tests start an actual PostgreSQL container, so they require a
//! working Docker daemon and are marked `#[ignore]`. Run them explicitly
//! with:
//!
//! ```text
//! cargo test -p awsim-rds-data -- --ignored
//! ```

use awsim_core::{RequestContext, ServiceHandler};
use awsim_rds_data::RdsDataService;
use serde_json::{Value, json};

const CLUSTER_ARN: &str = "arn:aws:rds:us-east-1:000000000000:cluster:test";

fn ctx() -> RequestContext {
    RequestContext::new("rds-data", "us-east-1")
}

async fn execute(service: &RdsDataService, input: Value) -> Value {
    service
        .handle("ExecuteStatement", input, &ctx())
        .await
        .expect("ExecuteStatement should succeed")
}

#[tokio::test]
#[ignore = "requires a Docker daemon"]
async fn create_insert_and_select_round_trip() {
    let service = RdsDataService::new();

    execute(
        &service,
        json!({
            "resourceArn": CLUSTER_ARN,
            "sql": "CREATE TABLE items (id INT PRIMARY KEY, name TEXT)",
        }),
    )
    .await;

    let insert = execute(
        &service,
        json!({
            "resourceArn": CLUSTER_ARN,
            "sql": "INSERT INTO items (id, name) VALUES (:id, :name)",
            "parameters": [
                { "name": "id", "value": { "longValue": 1 } },
                { "name": "name", "value": { "stringValue": "widget" } },
            ],
        }),
    )
    .await;
    assert_eq!(insert["numberOfRecordsUpdated"], 1);

    let select = execute(
        &service,
        json!({
            "resourceArn": CLUSTER_ARN,
            "sql": "SELECT id, name FROM items ORDER BY id",
            "includeResultMetadata": true,
        }),
    )
    .await;
    let records = select["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0][0], json!({ "longValue": 1 }));
    assert_eq!(records[0][1], json!({ "stringValue": "widget" }));
    let metadata = select["columnMetadata"].as_array().unwrap();
    assert_eq!(metadata[0]["name"], "id");
    assert_eq!(metadata[1]["name"], "name");
}

#[tokio::test]
#[ignore = "requires a Docker daemon"]
async fn rollback_discards_changes_and_commit_keeps_them() {
    let service = RdsDataService::new();
    execute(
        &service,
        json!({
            "resourceArn": CLUSTER_ARN,
            "sql": "CREATE TABLE t (n INT)",
        }),
    )
    .await;

    // Begin, insert, roll back: the row must not survive.
    let begin = service
        .handle(
            "BeginTransaction",
            json!({ "resourceArn": CLUSTER_ARN }),
            &ctx(),
        )
        .await
        .unwrap();
    let txn = begin["transactionId"].as_str().unwrap().to_string();
    execute(
        &service,
        json!({
            "resourceArn": CLUSTER_ARN,
            "transactionId": txn,
            "sql": "INSERT INTO t (n) VALUES (1)",
        }),
    )
    .await;
    service
        .handle(
            "RollbackTransaction",
            json!({ "transactionId": txn }),
            &ctx(),
        )
        .await
        .unwrap();

    let after_rollback = execute(
        &service,
        json!({ "resourceArn": CLUSTER_ARN, "sql": "SELECT COUNT(*) FROM t" }),
    )
    .await;
    assert_eq!(after_rollback["records"][0][0], json!({ "longValue": 0 }));

    // Begin, insert, commit: the row must survive.
    let begin = service
        .handle(
            "BeginTransaction",
            json!({ "resourceArn": CLUSTER_ARN }),
            &ctx(),
        )
        .await
        .unwrap();
    let txn = begin["transactionId"].as_str().unwrap().to_string();
    execute(
        &service,
        json!({
            "resourceArn": CLUSTER_ARN,
            "transactionId": txn,
            "sql": "INSERT INTO t (n) VALUES (2)",
        }),
    )
    .await;
    service
        .handle("CommitTransaction", json!({ "transactionId": txn }), &ctx())
        .await
        .unwrap();

    let after_commit = execute(
        &service,
        json!({ "resourceArn": CLUSTER_ARN, "sql": "SELECT COUNT(*) FROM t" }),
    )
    .await;
    assert_eq!(after_commit["records"][0][0], json!({ "longValue": 1 }));
}
