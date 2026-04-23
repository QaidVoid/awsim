use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::operations::tables::{table_key, table_to_value};
use crate::state::{GlueState, Trigger, Workflow};

fn now_str() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

pub fn batch_delete_table(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;

    let names: Vec<String> = input["TablesToDelete"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let mut errors: Vec<Value> = Vec::new();
    for name in names {
        let key = table_key(db_name, &name);
        if state.tables.remove(&key).is_none() {
            errors.push(json!({
                "TableName": name,
                "ErrorDetail": {
                    "ErrorCode": "EntityNotFoundException",
                    "ErrorMessage": format!("Table not found: {}.{}", db_name, name),
                }
            }));
        }
    }

    Ok(json!({ "Errors": errors }))
}

pub fn batch_get_tables(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;

    let names: Vec<String> = input["TablesToGet"]
        .as_array()
        .or_else(|| input["TableNames"].as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let mut found: Vec<Value> = Vec::new();
    let mut missing: Vec<Value> = Vec::new();

    for name in names {
        let key = table_key(db_name, &name);
        match state.tables.get(&key) {
            Some(t) => found.push(table_to_value(&t)),
            None => missing.push(json!({
                "DatabaseName": db_name,
                "Name": name,
            })),
        }
    }

    Ok(json!({
        "Tables": found,
        "TablesNotFound": missing,
    }))
}

pub fn get_partition(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"].as_str().unwrap_or("");
    let table_name = input["TableName"].as_str().unwrap_or("");
    let values: Vec<String> = input["PartitionValues"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let key = table_key(db_name, table_name);
    let table = state.tables.get(&key).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Table not found: {db_name}.{table_name}"),
        )
    })?;

    let values_str = values.join("/");
    let partition = table
        .partitions
        .iter()
        .find(|p| p.values.join("/") == values_str)
        .ok_or_else(|| {
            AwsError::not_found(
                "EntityNotFoundException",
                format!("Partition not found: {values_str}"),
            )
        })?;

    Ok(json!({
        "Partition": {
            "Values": partition.values,
            "DatabaseName": db_name,
            "TableName": table_name,
            "StorageDescriptor": partition.storage_descriptor,
            "CreationTime": partition.created_at,
        }
    }))
}

pub fn batch_get_partition(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"].as_str().unwrap_or("");
    let table_name = input["TableName"].as_str().unwrap_or("");

    let requested: Vec<Vec<String>> = input["PartitionsToGet"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|p| {
                    p["Values"]
                        .as_array()
                        .map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();

    let key = table_key(db_name, table_name);
    let mut found: Vec<Value> = Vec::new();
    let mut missing: Vec<Value> = Vec::new();

    if let Some(table) = state.tables.get(&key) {
        for vals in requested {
            let values_str = vals.join("/");
            match table.partitions.iter().find(|p| p.values.join("/") == values_str) {
                Some(p) => found.push(json!({
                    "Values": p.values,
                    "DatabaseName": db_name,
                    "TableName": table_name,
                    "StorageDescriptor": p.storage_descriptor,
                    "CreationTime": p.created_at,
                })),
                None => missing.push(json!({ "Values": vals })),
            }
        }
    }

    Ok(json!({
        "Partitions": found,
        "UnprocessedKeys": missing,
    }))
}

pub fn update_job(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["JobName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "JobName is required"))?;

    let mut job = state.jobs.get_mut(name).ok_or_else(|| {
        AwsError::not_found("EntityNotFoundException", format!("Job not found: {name}"))
    })?;

    let updates = &input["JobUpdate"];
    if let Some(role) = updates["Role"].as_str() {
        job.role = role.to_string();
    }
    if let Some(cmd) = updates.get("Command") {
        if !cmd.is_null() {
            job.command = Some(cmd.clone());
        }
    }
    if let Some(args) = updates.get("DefaultArguments") {
        if !args.is_null() {
            job.default_arguments = Some(args.clone());
        }
    }

    Ok(json!({ "JobName": name }))
}

pub fn update_connection(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    let mut conn = state.connections.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Connection not found: {name}"),
        )
    })?;

    let conn_input = &input["ConnectionInput"];
    if let Some(t) = conn_input["ConnectionType"].as_str() {
        conn.connection_type = t.to_string();
    }
    if let Some(d) = conn_input["Description"].as_str() {
        conn.description = Some(d.to_string());
    }
    if let Some(props) = conn_input["ConnectionProperties"].as_object() {
        conn.connection_properties = props
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
            .collect();
    }

    Ok(json!({}))
}

pub fn get_connection(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    let conn = state.connections.get(name).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Connection not found: {name}"),
        )
    })?;

    let props: serde_json::Map<String, Value> = conn
        .connection_properties
        .iter()
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect();

    Ok(json!({
        "Connection": {
            "Name": conn.name,
            "ConnectionType": conn.connection_type,
            "ConnectionProperties": props,
            "Description": conn.description,
            "CreationTime": conn.created_at,
        }
    }))
}

pub fn create_trigger(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;
    let trigger_type = input["Type"].as_str().unwrap_or("ON_DEMAND").to_string();

    if state.triggers.contains_key(name) {
        return Err(AwsError::conflict(
            "AlreadyExistsException",
            format!("Trigger already exists: {name}"),
        ));
    }

    let trigger = Trigger {
        name: name.to_string(),
        trigger_type,
        state: "CREATED".to_string(),
        schedule: input["Schedule"].as_str().map(String::from),
        actions: input.get("Actions").cloned().unwrap_or(json!([])),
        workflow_name: input["WorkflowName"].as_str().map(String::from),
        created_at: now_str(),
    };

    state.triggers.insert(name.to_string(), trigger);
    Ok(json!({ "Name": name }))
}

pub fn get_trigger(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    let trigger = state.triggers.get(name).ok_or_else(|| {
        AwsError::not_found("EntityNotFoundException", format!("Trigger not found: {name}"))
    })?;

    Ok(json!({ "Trigger": trigger_to_value(&trigger) }))
}

pub fn get_triggers(
    state: &GlueState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let triggers: Vec<Value> = state
        .triggers
        .iter()
        .map(|e| trigger_to_value(e.value()))
        .collect();

    Ok(json!({ "Triggers": triggers }))
}

pub fn delete_trigger(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    state.triggers.remove(name).ok_or_else(|| {
        AwsError::not_found("EntityNotFoundException", format!("Trigger not found: {name}"))
    })?;

    Ok(json!({ "Name": name }))
}

pub fn create_workflow(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    if state.workflows.contains_key(name) {
        return Err(AwsError::conflict(
            "AlreadyExistsException",
            format!("Workflow already exists: {name}"),
        ));
    }

    let workflow = Workflow {
        name: name.to_string(),
        description: input["Description"].as_str().map(String::from),
        default_run_properties: input
            .get("DefaultRunProperties")
            .cloned()
            .unwrap_or(json!({})),
        created_at: now_str(),
    };

    state.workflows.insert(name.to_string(), workflow);
    Ok(json!({ "Name": name }))
}

pub fn get_workflow(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    let workflow = state.workflows.get(name).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Workflow not found: {name}"),
        )
    })?;

    Ok(json!({ "Workflow": workflow_to_value(&workflow) }))
}

pub fn list_workflows(
    state: &GlueState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let names: Vec<Value> = state
        .workflows
        .iter()
        .map(|e| json!(e.value().name))
        .collect();

    Ok(json!({ "Workflows": names }))
}

pub fn delete_workflow(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    state.workflows.remove(name).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Workflow not found: {name}"),
        )
    })?;

    Ok(json!({ "Name": name }))
}

fn trigger_to_value(t: &Trigger) -> Value {
    json!({
        "Name": t.name,
        "Type": t.trigger_type,
        "State": t.state,
        "Schedule": t.schedule,
        "Actions": t.actions,
        "WorkflowName": t.workflow_name,
        "CreatedOn": t.created_at,
    })
}

fn workflow_to_value(w: &Workflow) -> Value {
    json!({
        "Name": w.name,
        "Description": w.description,
        "DefaultRunProperties": w.default_run_properties,
        "CreatedOn": w.created_at,
    })
}
