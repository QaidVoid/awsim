use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{AthenaState, QueryExecution};

/// Substitute Athena `?` placeholders with the supplied
/// `ExecutionParameters`. Placeholders inside single- or double-quoted
/// string literals are left untouched. Returns
/// `InvalidRequestException` when the placeholder count doesn't match
/// the parameter count.
fn substitute_execution_parameters(query: &str, params: &[String]) -> Result<String, AwsError> {
    let mut out = String::with_capacity(query.len());
    let mut in_single = false;
    let mut in_double = false;
    let mut iter = params.iter();
    let mut used = 0usize;
    for c in query.chars() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                out.push(c);
            }
            '"' if !in_single => {
                in_double = !in_double;
                out.push(c);
            }
            '?' if !in_single && !in_double => match iter.next() {
                Some(p) => {
                    out.push_str(p);
                    used += 1;
                }
                None => {
                    return Err(AwsError::bad_request(
                        "InvalidRequestException",
                        "Number of ExecutionParameters does not match the number of ? placeholders",
                    ));
                }
            },
            _ => out.push(c),
        }
    }
    if used != params.len() {
        return Err(AwsError::bad_request(
            "InvalidRequestException",
            "Number of ExecutionParameters does not match the number of ? placeholders",
        ));
    }
    Ok(out)
}

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
    let raw_query = input["QueryString"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "QueryString is required")
    })?;

    let params: Vec<String> = input
        .get("ExecutionParameters")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let query_string = substitute_execution_parameters(raw_query, &params)?;

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

    let client_request_token = input.get("ClientRequestToken").and_then(|v| v.as_str());
    let request_hash = client_request_token.map(|_| {
        awsim_core::idempotency::hash_request(&format!(
            "start_query:{query_string}:{workgroup}:{}:{}:{}",
            database.as_deref().unwrap_or(""),
            catalog.as_deref().unwrap_or(""),
            output_location.as_deref().unwrap_or(""),
        ))
    });
    if let (Some(token), Some(hash)) = (client_request_token, request_hash) {
        match state.start_query_idempotency.lookup(token, hash) {
            awsim_core::idempotency::Lookup::Hit(v) => return Ok(v),
            awsim_core::idempotency::Lookup::Mismatch => {
                return Err(AwsError::bad_request(
                    "IdempotentParameterMismatch",
                    format!(
                        "ClientRequestToken `{token}` was already used with different arguments."
                    ),
                ));
            }
            awsim_core::idempotency::Lookup::Miss => {}
        }
    }

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

    let response = json!({ "QueryExecutionId": id });
    if let (Some(token), Some(hash)) = (client_request_token, request_hash) {
        state
            .start_query_idempotency
            .insert(token, hash, response.clone());
    }
    Ok(response)
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

#[cfg(test)]
mod idempotency_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("athena", "us-east-1")
    }

    #[test]
    fn replay_with_same_args_returns_same_query_id() {
        let state = AthenaState::default();
        let input = json!({
            "QueryString": "SELECT 1",
            "ClientRequestToken": "tok-1",
            "WorkGroup": "primary",
        });
        let first = start_query_execution(&state, &input, &ctx()).unwrap();
        let second = start_query_execution(&state, &input, &ctx()).unwrap();
        assert_eq!(first["QueryExecutionId"], second["QueryExecutionId"]);
    }

    #[test]
    fn replay_with_different_args_returns_idempotent_parameter_mismatch() {
        let state = AthenaState::default();
        start_query_execution(
            &state,
            &json!({
                "QueryString": "SELECT 1",
                "ClientRequestToken": "tok-2",
            }),
            &ctx(),
        )
        .unwrap();
        let err = start_query_execution(
            &state,
            &json!({
                "QueryString": "SELECT 2",
                "ClientRequestToken": "tok-2",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "IdempotentParameterMismatch");
    }

    #[test]
    fn omitting_token_runs_fresh_each_time() {
        let state = AthenaState::default();
        let input = json!({ "QueryString": "SELECT 1" });
        let a = start_query_execution(&state, &input, &ctx()).unwrap();
        let b = start_query_execution(&state, &input, &ctx()).unwrap();
        assert_ne!(a["QueryExecutionId"], b["QueryExecutionId"]);
    }
}

#[cfg(test)]
mod execution_parameters_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("athena", "us-east-1")
    }

    #[test]
    fn placeholders_are_substituted_left_to_right() {
        let state = AthenaState::default();
        let id = start_query_execution(
            &state,
            &json!({
                "QueryString": "SELECT * FROM t WHERE a = ? AND b = ?",
                "ExecutionParameters": ["'alice'", "42"],
            }),
            &ctx(),
        )
        .unwrap()["QueryExecutionId"]
            .as_str()
            .unwrap()
            .to_string();
        let qe = state.query_executions.get(&id).unwrap();
        assert_eq!(qe.query, "SELECT * FROM t WHERE a = 'alice' AND b = 42");
    }

    #[test]
    fn question_marks_inside_string_literals_are_not_placeholders() {
        let state = AthenaState::default();
        let id = start_query_execution(
            &state,
            &json!({
                "QueryString": "SELECT 'why?', ? FROM t",
                "ExecutionParameters": ["1"],
            }),
            &ctx(),
        )
        .unwrap()["QueryExecutionId"]
            .as_str()
            .unwrap()
            .to_string();
        let qe = state.query_executions.get(&id).unwrap();
        assert_eq!(qe.query, "SELECT 'why?', 1 FROM t");
    }

    #[test]
    fn mismatched_parameter_count_returns_invalid_request() {
        let state = AthenaState::default();
        let err = start_query_execution(
            &state,
            &json!({
                "QueryString": "SELECT ? , ?",
                "ExecutionParameters": ["1"],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidRequestException");
    }
}
