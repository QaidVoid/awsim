use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{AthenaState, PreparedStatement};

fn now_str() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn stmt_key(workgroup: &str, name: &str) -> String {
    format!("{workgroup}/{name}")
}

fn stmt_to_value(s: &PreparedStatement) -> Value {
    json!({
        "StatementName": s.statement_name,
        "WorkGroupName": s.workgroup,
        "QueryStatement": s.query_statement,
        "Description": s.description,
        "LastModifiedTime": s.last_modified_time,
    })
}

// ---------------------------------------------------------------------------
// CreatePreparedStatement
// ---------------------------------------------------------------------------

pub fn create_prepared_statement(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["StatementName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "StatementName is required")
    })?;
    let workgroup = input["WorkGroup"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "WorkGroup is required"))?;
    let query_statement = input["QueryStatement"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "QueryStatement is required")
    })?;

    let key = stmt_key(workgroup, name);
    if state.prepared_statements.contains_key(&key) {
        return Err(AwsError::conflict(
            "InvalidRequestException",
            format!("PreparedStatement '{name}' already exists in workgroup '{workgroup}'"),
        ));
    }

    let description = input["Description"].as_str().map(|s| s.to_string());
    let stmt = PreparedStatement {
        statement_name: name.to_string(),
        workgroup: workgroup.to_string(),
        query_statement: query_statement.to_string(),
        description,
        last_modified_time: now_str(),
    };

    info!(name = %name, workgroup = %workgroup, "Created Athena prepared statement");
    state.prepared_statements.insert(key, stmt);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// GetPreparedStatement
// ---------------------------------------------------------------------------

pub fn get_prepared_statement(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["StatementName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "StatementName is required")
    })?;
    let workgroup = input["WorkGroup"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "WorkGroup is required"))?;

    let key = stmt_key(workgroup, name);
    let stmt = state.prepared_statements.get(&key).ok_or_else(|| {
        AwsError::not_found(
            "InvalidRequestException",
            format!("PreparedStatement '{name}' not found in workgroup '{workgroup}'"),
        )
    })?;

    Ok(json!({ "PreparedStatement": stmt_to_value(&stmt) }))
}

// ---------------------------------------------------------------------------
// ListPreparedStatements
// ---------------------------------------------------------------------------

pub fn list_prepared_statements(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let workgroup = input["WorkGroup"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "WorkGroup is required"))?;

    let stmts: Vec<Value> = state
        .prepared_statements
        .iter()
        .filter(|e| e.value().workgroup == workgroup)
        .map(|e| {
            json!({
                "StatementName": e.value().statement_name,
                "LastModifiedTime": e.value().last_modified_time,
            })
        })
        .collect();

    Ok(json!({ "PreparedStatements": stmts }))
}

// ---------------------------------------------------------------------------
// DeletePreparedStatement
// ---------------------------------------------------------------------------

pub fn delete_prepared_statement(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["StatementName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "StatementName is required")
    })?;
    let workgroup = input["WorkGroup"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "WorkGroup is required"))?;

    let key = stmt_key(workgroup, name);
    state.prepared_statements.remove(&key).ok_or_else(|| {
        AwsError::not_found(
            "InvalidRequestException",
            format!("PreparedStatement '{name}' not found in workgroup '{workgroup}'"),
        )
    })?;

    info!(name = %name, workgroup = %workgroup, "Deleted Athena prepared statement");
    Ok(json!({}))
}
