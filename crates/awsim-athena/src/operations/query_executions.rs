use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{AthenaState, QueryExecution};

fn now_str() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

// ---------------------------------------------------------------------------
// StartQueryExecution
// ---------------------------------------------------------------------------

pub fn start_query_execution(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let query_string = input["QueryString"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "QueryString is required")
    })?;

    let workgroup = input["WorkGroup"].as_str().unwrap_or("primary").to_string();
    let database = input["QueryExecutionContext"]["Database"]
        .as_str()
        .map(|s| s.to_string());
    let catalog = input["QueryExecutionContext"]["Catalog"]
        .as_str()
        .map(|s| s.to_string());
    let output_location = input["ResultConfiguration"]["OutputLocation"]
        .as_str()
        .map(|s| s.to_string());

    let now = now_str();
    let id = Uuid::new_v4().to_string();

    let qe = QueryExecution {
        id: id.clone(),
        query: query_string.to_string(),
        database,
        catalog,
        workgroup,
        output_location,
        status: "SUCCEEDED".to_string(),
        submitted_at: now.clone(),
        completed_at: now,
    };

    info!(id = %id, "Started Athena query execution (stub: SUCCEEDED immediately)");
    state.query_executions.insert(id.clone(), qe);

    Ok(json!({ "QueryExecutionId": id }))
}

// ---------------------------------------------------------------------------
// GetQueryExecution
// ---------------------------------------------------------------------------

pub fn get_query_execution(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input["QueryExecutionId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "QueryExecutionId is required")
    })?;

    let qe = state.query_executions.get(id).ok_or_else(|| {
        AwsError::not_found(
            "InvalidRequestException",
            format!("QueryExecution not found: {id}"),
        )
    })?;

    Ok(json!({ "QueryExecution": query_execution_to_value(&qe) }))
}

// ---------------------------------------------------------------------------
// GetQueryResults
// ---------------------------------------------------------------------------

pub fn get_query_results(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input["QueryExecutionId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "QueryExecutionId is required")
    })?;

    // Verify the execution exists
    if !state.query_executions.contains_key(id) {
        return Err(AwsError::not_found(
            "InvalidRequestException",
            format!("QueryExecution not found: {id}"),
        ));
    }

    // Stub: return empty result set
    Ok(json!({
        "ResultSet": {
            "Rows": [],
            "ResultSetMetadata": {
                "ColumnInfo": []
            }
        }
    }))
}

// ---------------------------------------------------------------------------
// ListQueryExecutions
// ---------------------------------------------------------------------------

pub fn list_query_executions(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let workgroup_filter = input["WorkGroup"].as_str();

    let ids: Vec<Value> = state
        .query_executions
        .iter()
        .filter(|e| {
            workgroup_filter
                .map(|wg| e.value().workgroup == wg)
                .unwrap_or(true)
        })
        .map(|e| Value::String(e.key().clone()))
        .collect();

    Ok(json!({ "QueryExecutionIds": ids }))
}

// ---------------------------------------------------------------------------
// StopQueryExecution
// ---------------------------------------------------------------------------

pub fn stop_query_execution(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input["QueryExecutionId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "QueryExecutionId is required")
    })?;

    // In the stub, queries already complete instantly, so stopping is a no-op.
    // Verify it exists and return success.
    if !state.query_executions.contains_key(id) {
        return Err(AwsError::not_found(
            "InvalidRequestException",
            format!("QueryExecution not found: {id}"),
        ));
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// BatchGetQueryExecution
// ---------------------------------------------------------------------------

pub fn batch_get_query_execution(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ids: Vec<&str> = match &input["QueryExecutionIds"] {
        Value::Array(arr) => arr.iter().filter_map(|v| v.as_str()).collect(),
        _ => vec![],
    };

    let mut found = Vec::new();
    let mut unprocessed = Vec::new();

    for id in ids {
        if let Some(qe) = state.query_executions.get(id) {
            found.push(query_execution_to_value(&qe));
        } else {
            unprocessed.push(Value::String(id.to_string()));
        }
    }

    Ok(json!({
        "QueryExecutions": found,
        "UnprocessedQueryExecutionIds": unprocessed,
    }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn query_execution_to_value(qe: &QueryExecution) -> Value {
    json!({
        "QueryExecutionId": qe.id,
        "Query": qe.query,
        "WorkGroup": qe.workgroup,
        "QueryExecutionContext": {
            "Database": qe.database,
            "Catalog": qe.catalog,
        },
        "ResultConfiguration": {
            "OutputLocation": qe.output_location,
        },
        "Status": {
            "State": qe.status,
            "SubmissionDateTime": qe.submitted_at,
            "CompletionDateTime": qe.completed_at,
        },
    })
}
