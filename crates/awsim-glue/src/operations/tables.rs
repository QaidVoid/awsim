use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{GluePartition, GlueState, GlueTable};

pub fn now_str() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

pub fn table_key(db: &str, table: &str) -> String {
    format!("{db}.{table}")
}

// ---------------------------------------------------------------------------
// CreateTable
// ---------------------------------------------------------------------------

pub fn create_table(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;
    let table_name = input["TableInput"]["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "TableInput.Name is required"))?;

    let key = table_key(db_name, table_name);
    if state.tables.contains_key(&key) {
        return Err(AwsError::conflict(
            "AlreadyExistsException",
            format!("Table already exists: {db_name}.{table_name}"),
        ));
    }

    let now = now_str();
    let storage_descriptor = input["TableInput"]["StorageDescriptor"].clone();

    let table = GlueTable {
        database_name: db_name.to_string(),
        name: table_name.to_string(),
        storage_descriptor: if storage_descriptor.is_null() { None } else { Some(storage_descriptor) },
        description: input["TableInput"]["Description"].as_str().map(|s| s.to_string()),
        created_at: now.clone(),
        updated_at: now,
        partitions: Vec::new(),
    };

    info!(db = %db_name, table = %table_name, "Created Glue table");
    state.tables.insert(key, table);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// GetTable
// ---------------------------------------------------------------------------

pub fn get_table(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;
    let table_name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    let key = table_key(db_name, table_name);
    let table = state.tables.get(&key).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Table not found: {db_name}.{table_name}"),
        )
    })?;

    Ok(json!({ "Table": table_to_value(&table) }))
}

// ---------------------------------------------------------------------------
// GetTables
// ---------------------------------------------------------------------------

pub fn get_tables(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;

    let list: Vec<Value> = state
        .tables
        .iter()
        .filter(|e| e.value().database_name == db_name)
        .map(|e| table_to_value(e.value()))
        .collect();

    Ok(json!({ "TableList": list }))
}

// ---------------------------------------------------------------------------
// DeleteTable
// ---------------------------------------------------------------------------

pub fn delete_table(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;
    let table_name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    let key = table_key(db_name, table_name);
    state.tables.remove(&key).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Table not found: {db_name}.{table_name}"),
        )
    })?;

    info!(db = %db_name, table = %table_name, "Deleted Glue table");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UpdateTable
// ---------------------------------------------------------------------------

pub fn update_table(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;
    let table_name = input["TableInput"]["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "TableInput.Name is required"))?;

    let key = table_key(db_name, table_name);
    let mut table = state.tables.get_mut(&key).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Table not found: {db_name}.{table_name}"),
        )
    })?;

    if let Some(sd) = input["TableInput"].get("StorageDescriptor") {
        if !sd.is_null() {
            table.storage_descriptor = Some(sd.clone());
        }
    }
    if let Some(desc) = input["TableInput"]["Description"].as_str() {
        table.description = Some(desc.to_string());
    }
    table.updated_at = now_str();

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// SearchTables
// ---------------------------------------------------------------------------

pub fn search_tables(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_expr = input["Filters"]
        .as_array()
        .and_then(|f| f.first())
        .and_then(|f| f["Value"].as_str())
        .unwrap_or("")
        .to_lowercase();

    let search_text = input["SearchText"].as_str().unwrap_or("").to_lowercase();
    let query = if !search_text.is_empty() { search_text } else { filter_expr };

    let list: Vec<Value> = state
        .tables
        .iter()
        .filter(|e| {
            if query.is_empty() {
                true
            } else {
                e.value().name.to_lowercase().contains(&query)
                    || e.value().database_name.to_lowercase().contains(&query)
            }
        })
        .map(|e| table_to_value(e.value()))
        .collect();

    Ok(json!({ "TableList": list }))
}

// ---------------------------------------------------------------------------
// GetPartitions
// ---------------------------------------------------------------------------

pub fn get_partitions(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;
    let table_name = input["TableName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "TableName is required"))?;

    let key = table_key(db_name, table_name);
    let table = state.tables.get(&key).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Table not found: {db_name}.{table_name}"),
        )
    })?;

    let partitions: Vec<Value> = table
        .partitions
        .iter()
        .map(partition_to_value)
        .collect();

    Ok(json!({ "Partitions": partitions }))
}

// ---------------------------------------------------------------------------
// CreatePartition
// ---------------------------------------------------------------------------

pub fn create_partition(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;
    let table_name = input["TableName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "TableName is required"))?;

    let values: Vec<String> = input["PartitionInput"]["Values"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "PartitionInput.Values is required"))?
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    let storage_descriptor = input["PartitionInput"]["StorageDescriptor"].clone();

    let key = table_key(db_name, table_name);
    let mut table = state.tables.get_mut(&key).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Table not found: {db_name}.{table_name}"),
        )
    })?;

    // Check for duplicate partition
    let values_str = values.join("/");
    if table
        .partitions
        .iter()
        .any(|p| p.values.join("/") == values_str)
    {
        return Err(AwsError::conflict(
            "AlreadyExistsException",
            format!("Partition already exists: {values_str}"),
        ));
    }

    let partition = GluePartition {
        values,
        storage_descriptor: if storage_descriptor.is_null() {
            None
        } else {
            Some(storage_descriptor)
        },
        created_at: now_str(),
    };

    info!(db = %db_name, table = %table_name, "Created Glue partition");
    table.partitions.push(partition);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DeletePartition
// ---------------------------------------------------------------------------

pub fn delete_partition(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;
    let table_name = input["TableName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "TableName is required"))?;

    let values: Vec<String> = input["PartitionValues"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "PartitionValues is required"))?
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    let values_str = values.join("/");
    let key = table_key(db_name, table_name);
    let mut table = state.tables.get_mut(&key).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Table not found: {db_name}.{table_name}"),
        )
    })?;

    let before = table.partitions.len();
    table.partitions.retain(|p| p.values.join("/") != values_str);
    if table.partitions.len() == before {
        return Err(AwsError::not_found(
            "EntityNotFoundException",
            format!("Partition not found: {values_str}"),
        ));
    }

    info!(db = %db_name, table = %table_name, "Deleted Glue partition");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// BatchCreatePartition
// ---------------------------------------------------------------------------

pub fn batch_create_partition(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;
    let table_name = input["TableName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "TableName is required"))?;

    let partition_inputs = input["PartitionInputList"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "PartitionInputList is required"))?;

    let key = table_key(db_name, table_name);
    let mut table = state.tables.get_mut(&key).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Table not found: {db_name}.{table_name}"),
        )
    })?;

    let mut errors: Vec<Value> = Vec::new();

    for partition_input in partition_inputs {
        let values: Vec<String> = partition_input["Values"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let values_str = values.join("/");

        if table
            .partitions
            .iter()
            .any(|p| p.values.join("/") == values_str)
        {
            errors.push(json!({
                "PartitionValues": values,
                "ErrorDetail": {
                    "ErrorCode": "AlreadyExistsException",
                    "ErrorMessage": format!("Partition already exists: {values_str}"),
                }
            }));
            continue;
        }

        let storage_descriptor = partition_input["StorageDescriptor"].clone();
        let partition = GluePartition {
            values,
            storage_descriptor: if storage_descriptor.is_null() {
                None
            } else {
                Some(storage_descriptor)
            },
            created_at: now_str(),
        };
        table.partitions.push(partition);
    }

    info!(db = %db_name, table = %table_name, "BatchCreatePartition");
    Ok(json!({ "Errors": errors }))
}

// ---------------------------------------------------------------------------
// BatchDeletePartition
// ---------------------------------------------------------------------------

pub fn batch_delete_partition(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let db_name = input["DatabaseName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "DatabaseName is required"))?;
    let table_name = input["TableName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "TableName is required"))?;

    let partition_value_list = input["PartitionsToDelete"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "PartitionsToDelete is required"))?;

    let key = table_key(db_name, table_name);
    let mut table = state.tables.get_mut(&key).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Table not found: {db_name}.{table_name}"),
        )
    })?;

    let mut errors: Vec<Value> = Vec::new();

    for pv_entry in partition_value_list {
        let values: Vec<String> = pv_entry["Values"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let values_str = values.join("/");
        let before = table.partitions.len();
        table.partitions.retain(|p| p.values.join("/") != values_str);
        if table.partitions.len() == before {
            errors.push(json!({
                "PartitionValues": values,
                "ErrorDetail": {
                    "ErrorCode": "EntityNotFoundException",
                    "ErrorMessage": format!("Partition not found: {values_str}"),
                }
            }));
        }
    }

    info!(db = %db_name, table = %table_name, "BatchDeletePartition");
    Ok(json!({ "Errors": errors }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn table_to_value(t: &GlueTable) -> Value {
    json!({
        "DatabaseName": t.database_name,
        "Name": t.name,
        "StorageDescriptor": t.storage_descriptor,
        "Description": t.description,
        "CreateTime": t.created_at,
        "UpdateTime": t.updated_at,
    })
}

fn partition_to_value(p: &GluePartition) -> Value {
    json!({
        "Values": p.values,
        "StorageDescriptor": p.storage_descriptor,
        "CreationTime": p.created_at,
    })
}
