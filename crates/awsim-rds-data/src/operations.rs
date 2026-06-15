use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::engine::{ExecResult, PgEngine};
use crate::types::{column_metadata, inline_parameters, parse_parameters};

fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input.get(key).and_then(|v| v.as_str()).ok_or_else(|| {
        AwsError::bad_request(
            "BadRequestException",
            format!("Missing required field: {key}"),
        )
    })
}

/// `ExecuteStatement` runs a single SQL statement and returns either the
/// number of rows it changed or the rows it produced.
pub async fn execute_statement(engine: &PgEngine, input: &Value) -> Result<Value, AwsError> {
    let resource_arn = require_str(input, "resourceArn")?;
    let sql = require_str(input, "sql")?;
    let transaction_id = input.get("transactionId").and_then(|v| v.as_str());
    let include_metadata = input
        .get("includeResultMetadata")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let params = parse_parameters(input)?;
    let final_sql = inline_parameters(sql, &params)?;

    match engine
        .execute(resource_arn, transaction_id, &final_sql)
        .await?
    {
        ExecResult::Update { rows_affected } => Ok(json!({
            "numberOfRecordsUpdated": rows_affected,
            "records": [],
            "generatedFields": [],
        })),
        ExecResult::Query { columns, records } => {
            let mut resp = json!({
                "records": records,
                "numberOfRecordsUpdated": 0,
            });
            if include_metadata {
                resp["columnMetadata"] = json!(
                    columns
                        .iter()
                        .map(|c| column_metadata(&c.name, c.type_oid, &c.type_name))
                        .collect::<Vec<_>>()
                );
            }
            Ok(resp)
        }
    }
}

/// `BatchExecuteStatement` runs one statement once per supplied parameter
/// set.
pub async fn batch_execute_statement(engine: &PgEngine, input: &Value) -> Result<Value, AwsError> {
    let resource_arn = require_str(input, "resourceArn")?;
    let sql = require_str(input, "sql")?;
    let transaction_id = input.get("transactionId").and_then(|v| v.as_str());
    let sets = input
        .get("parameterSets")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut update_results = Vec::new();
    if sets.is_empty() {
        engine.execute(resource_arn, transaction_id, sql).await?;
        update_results.push(json!({ "generatedFields": [] }));
    } else {
        for set in &sets {
            let params = parse_parameters(&json!({ "parameters": set }))?;
            let final_sql = inline_parameters(sql, &params)?;
            engine
                .execute(resource_arn, transaction_id, &final_sql)
                .await?;
            update_results.push(json!({ "generatedFields": [] }));
        }
    }
    Ok(json!({ "updateResults": update_results }))
}

/// `BeginTransaction` opens a transaction and returns its identifier.
pub async fn begin_transaction(engine: &PgEngine, input: &Value) -> Result<Value, AwsError> {
    let resource_arn = require_str(input, "resourceArn")?;
    let transaction_id = engine.begin(resource_arn).await?;
    Ok(json!({ "transactionId": transaction_id }))
}

/// `CommitTransaction` commits and closes a transaction.
pub async fn commit_transaction(engine: &PgEngine, input: &Value) -> Result<Value, AwsError> {
    let transaction_id = require_str(input, "transactionId")?;
    engine.commit(transaction_id).await?;
    Ok(json!({ "transactionStatus": "Transaction Committed" }))
}

/// `RollbackTransaction` rolls back and closes a transaction.
pub async fn rollback_transaction(engine: &PgEngine, input: &Value) -> Result<Value, AwsError> {
    let transaction_id = require_str(input, "transactionId")?;
    engine.rollback(transaction_id).await?;
    Ok(json!({ "transactionStatus": "Rollback Complete" }))
}
