use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::AthenaState;

pub fn tag_resource(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["ResourceARN"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "ResourceARN is required")
    })?;

    let mut entry = state.resource_tags.entry(arn.to_string()).or_default();

    if let Some(arr) = input["Tags"].as_array() {
        for tag in arr {
            if let (Some(k), Some(v)) = (tag["Key"].as_str(), tag["Value"].as_str()) {
                entry.insert(k.to_string(), v.to_string());
            }
        }
    }

    Ok(json!({}))
}

pub fn untag_resource(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["ResourceARN"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "ResourceARN is required")
    })?;

    let keys: Vec<String> = input["TagKeys"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if let Some(mut entry) = state.resource_tags.get_mut(arn) {
        for k in &keys {
            entry.remove(k);
        }
    }

    Ok(json!({}))
}

pub fn list_tags_for_resource(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["ResourceARN"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "ResourceARN is required")
    })?;

    let tags: Vec<Value> = state
        .resource_tags
        .get(arn)
        .map(|t| {
            t.iter()
                .map(|(k, v)| json!({ "Key": k, "Value": v }))
                .collect()
        })
        .unwrap_or_default();

    Ok(json!({ "Tags": tags, "NextToken": Value::Null }))
}

pub fn list_engine_versions(
    _state: &AthenaState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({
        "EngineVersions": [
            {
                "SelectedEngineVersion": "AUTO",
                "EffectiveEngineVersion": "Athena engine version 3"
            },
            {
                "SelectedEngineVersion": "Athena engine version 2",
                "EffectiveEngineVersion": "Athena engine version 2"
            },
            {
                "SelectedEngineVersion": "Athena engine version 3",
                "EffectiveEngineVersion": "Athena engine version 3"
            }
        ]
    }))
}

pub fn list_application_dpu_sizes(
    _state: &AthenaState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({
        "ApplicationDPUSizes": [
            {
                "ApplicationRuntimeId": "Athena notebook version 1",
                "SupportedDPUSizes": [1, 2, 4, 8, 16]
            }
        ]
    }))
}

pub fn get_query_runtime_statistics(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let qid = input["QueryExecutionId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "QueryExecutionId is required")
    })?;

    let _ = state.query_executions.get(qid).ok_or_else(|| {
        AwsError::not_found(
            "InvalidRequestException",
            format!("QueryExecution not found: {qid}"),
        )
    })?;

    Ok(json!({
        "QueryRuntimeStatistics": {
            "Timeline": {
                "QueryQueueTimeInMillis": 10,
                "ServicePreProcessingTimeInMillis": 5,
                "QueryPlanningTimeInMillis": 20,
                "EngineExecutionTimeInMillis": 100,
                "ServiceProcessingTimeInMillis": 5,
                "TotalExecutionTimeInMillis": 140
            },
            "Rows": {
                "InputRows": 1,
                "InputBytes": 16,
                "OutputBytes": 16,
                "OutputRows": 1
            },
            "OutputStage": null
        }
    }))
}

pub fn update_data_catalog(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "Name is required"))?;

    let mut catalog = state.data_catalogs.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "InvalidRequestException",
            format!("DataCatalog not found: {name}"),
        )
    })?;

    if let Some(t) = input["Type"].as_str() {
        catalog.catalog_type = t.to_string();
    }
    if let Some(d) = input["Description"].as_str() {
        catalog.description = Some(d.to_string());
    }
    if let Some(p) = input.get("Parameters") {
        catalog.parameters = p.clone();
    }

    Ok(json!({}))
}

pub fn batch_get_prepared_statement(
    state: &AthenaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let workgroup = input["WorkGroup"].as_str().unwrap_or("primary");
    let names: Vec<String> = input["PreparedStatementNames"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let mut found: Vec<Value> = Vec::new();
    let mut errors: Vec<Value> = Vec::new();

    for name in names {
        let key = format!("{}/{}", workgroup, name);
        if let Some(s) = state.prepared_statements.get(&key) {
            found.push(json!({
                "StatementName": s.statement_name,
                "WorkGroupName": s.workgroup,
                "QueryStatement": s.query_statement,
                "Description": s.description,
                "LastModifiedTime": s.last_modified_time,
            }));
        } else {
            errors.push(json!({
                "StatementName": name,
                "ErrorCode": "ResourceNotFoundException",
                "ErrorMessage": format!("PreparedStatement not found: {}", name),
            }));
        }
    }

    Ok(json!({
        "PreparedStatements": found,
        "UnprocessedPreparedStatementNames": errors,
    }))
}
