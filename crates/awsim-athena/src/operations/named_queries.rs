use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{AthenaState, NamedQuery};

// ---------------------------------------------------------------------------
// CreateNamedQuery
// ---------------------------------------------------------------------------

pub fn create_named_query(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "Name is required"))?;
    let database = input["Database"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "Database is required"))?;
    let query_string = input["QueryString"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "QueryString is required")
    })?;

    let workgroup = input["WorkGroup"].as_str().unwrap_or("primary").to_string();
    let description = input["Description"].as_str().map(|s| s.to_string());
    let id = Uuid::new_v4().to_string();

    let nq = NamedQuery {
        id: id.clone(),
        name: name.to_string(),
        database: database.to_string(),
        query_string: query_string.to_string(),
        workgroup,
        description,
    };

    info!(id = %id, name = %name, "Created Athena named query");
    state.named_queries.insert(id.clone(), nq);

    Ok(json!({ "NamedQueryId": id }))
}

// ---------------------------------------------------------------------------
// GetNamedQuery
// ---------------------------------------------------------------------------

pub fn get_named_query(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input["NamedQueryId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "NamedQueryId is required")
    })?;

    let nq = state.named_queries.get(id).ok_or_else(|| {
        AwsError::not_found(
            "InvalidRequestException",
            format!("NamedQuery not found: {id}"),
        )
    })?;

    Ok(json!({
        "NamedQuery": {
            "NamedQueryId": nq.id,
            "Name": nq.name,
            "Database": nq.database,
            "QueryString": nq.query_string,
            "WorkGroup": nq.workgroup,
            "Description": nq.description,
        }
    }))
}

// ---------------------------------------------------------------------------
// ListNamedQueries
// ---------------------------------------------------------------------------

pub fn list_named_queries(
    state: &AthenaState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ids: Vec<Value> = state
        .named_queries
        .iter()
        .map(|e| Value::String(e.key().clone()))
        .collect();

    Ok(json!({ "NamedQueryIds": ids }))
}

// ---------------------------------------------------------------------------
// BatchGetNamedQuery
// ---------------------------------------------------------------------------

pub fn batch_get_named_query(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ids: Vec<&str> = match &input["NamedQueryIds"] {
        Value::Array(arr) => arr.iter().filter_map(|v| v.as_str()).collect(),
        _ => vec![],
    };

    let mut found = Vec::new();
    let mut unprocessed = Vec::new();

    for id in ids {
        if let Some(nq) = state.named_queries.get(id) {
            found.push(json!({
                "NamedQueryId": nq.id,
                "Name": nq.name,
                "Database": nq.database,
                "QueryString": nq.query_string,
                "WorkGroup": nq.workgroup,
                "Description": nq.description,
            }));
        } else {
            unprocessed.push(Value::String(id.to_string()));
        }
    }

    Ok(json!({
        "NamedQueries": found,
        "UnprocessedNamedQueryIds": unprocessed,
    }))
}

// ---------------------------------------------------------------------------
// DeleteNamedQuery
// ---------------------------------------------------------------------------

pub fn delete_named_query(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input["NamedQueryId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "NamedQueryId is required")
    })?;

    state.named_queries.remove(id).ok_or_else(|| {
        AwsError::not_found(
            "InvalidRequestException",
            format!("NamedQuery not found: {id}"),
        )
    })?;

    info!(id = %id, "Deleted Athena named query");
    Ok(json!({}))
}
