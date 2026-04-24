use crate::chk;
use crate::runner::common::*;
use aws_sdk_dynamodb::types::{
    AttributeDefinition, AttributeValue, BillingMode, KeySchemaElement, KeyType,
    ScalarAttributeType,
};

pub async fn test_dynamodb(endpoint: &str, verbose: bool) -> Vec<OpResult> {
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
                vec![
                    aws_sdk_dynamodb::types::WriteRequest::builder()
                        .put_request(
                            aws_sdk_dynamodb::types::PutRequest::builder()
                                .item("id", AttributeValue::S("batch-1".into()))
                                .build()
                                .unwrap(),
                        )
                        .build()
                ],
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
                    .keys(std::collections::HashMap::from([(
                        "id".to_string(),
                        AttributeValue::S("batch-1".into()),
                    )]))
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
