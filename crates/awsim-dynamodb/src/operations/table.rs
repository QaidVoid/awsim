use std::collections::BTreeMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{
    AttributeDefinition, DynamoState, GlobalSecondaryIndex, KeySchemaElement,
    LocalSecondaryIndex, Projection, Table,
};

use super::{opt_str, require_str};

fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d, h, min, s) = unix_to_ymd_hms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{min:02}:{s:02}Z")
}

fn unix_to_ymd_hms(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let s = secs % 60;
    let mins = secs / 60;
    let min = mins % 60;
    let hours = mins / 60;
    let h = hours % 24;
    let days = hours / 24;
    let (y, doy) = days_to_year(days);
    let (mo, d) = doy_to_month_day(doy, is_leap(y));
    (y, mo, d, h, min, s)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

fn days_to_year(mut days: u64) -> (u64, u64) {
    let mut y = 1970u64;
    loop {
        let dy = if is_leap(y) { 366 } else { 365 };
        if days < dy {
            return (y, days);
        }
        days -= dy;
        y += 1;
    }
}

fn doy_to_month_day(doy: u64, leap: bool) -> (u64, u64) {
    let months: &[u64] = if leap {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut rem = doy;
    for (i, &days) in months.iter().enumerate() {
        if rem < days {
            return ((i + 1) as u64, rem + 1);
        }
        rem -= days;
    }
    (12, 31)
}

fn parse_key_schema(input: &Value) -> Vec<KeySchemaElement> {
    input
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|el| {
                    let attr_name = el.get("AttributeName")?.as_str()?.to_string();
                    let key_type = el.get("KeyType")?.as_str()?.to_string();
                    Some(KeySchemaElement {
                        attribute_name: attr_name,
                        key_type,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_attribute_definitions(input: &Value) -> Vec<AttributeDefinition> {
    input
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|el| {
                    let attr_name = el.get("AttributeName")?.as_str()?.to_string();
                    let attr_type = el.get("AttributeType")?.as_str()?.to_string();
                    Some(AttributeDefinition {
                        attribute_name: attr_name,
                        attribute_type: attr_type,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_projection(input: &Value) -> Projection {
    let projection_type = input
        .get("ProjectionType")
        .and_then(|v| v.as_str())
        .unwrap_or("ALL")
        .to_string();
    let non_key_attributes = input
        .get("NonKeyAttributes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Projection {
        projection_type,
        non_key_attributes,
    }
}

fn parse_gsi(input: &Value) -> Vec<GlobalSecondaryIndex> {
    input
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|el| {
                    let index_name = el.get("IndexName")?.as_str()?.to_string();
                    let key_schema = parse_key_schema(&el["KeySchema"]);
                    let projection = parse_projection(&el.get("Projection").unwrap_or(&json!({})));
                    Some(GlobalSecondaryIndex {
                        index_name,
                        key_schema,
                        projection,
                        status: "ACTIVE".to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_lsi(input: &Value) -> Vec<LocalSecondaryIndex> {
    input
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|el| {
                    let index_name = el.get("IndexName")?.as_str()?.to_string();
                    let key_schema = parse_key_schema(&el["KeySchema"]);
                    let projection = parse_projection(&el.get("Projection").unwrap_or(&json!({})));
                    Some(LocalSecondaryIndex {
                        index_name,
                        key_schema,
                        projection,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn table_description(table: &Table) -> Value {
    let key_schema: Vec<Value> = table
        .key_schema
        .iter()
        .map(|k| json!({ "AttributeName": k.attribute_name, "KeyType": k.key_type }))
        .collect();

    let attr_defs: Vec<Value> = table
        .attribute_definitions
        .iter()
        .map(|a| json!({ "AttributeName": a.attribute_name, "AttributeType": a.attribute_type }))
        .collect();

    let gsi: Vec<Value> = table
        .gsi
        .iter()
        .map(|g| {
            let ks: Vec<Value> = g
                .key_schema
                .iter()
                .map(|k| json!({ "AttributeName": k.attribute_name, "KeyType": k.key_type }))
                .collect();
            json!({
                "IndexName": g.index_name,
                "KeySchema": ks,
                "Projection": {
                    "ProjectionType": g.projection.projection_type,
                    "NonKeyAttributes": g.projection.non_key_attributes
                },
                "IndexStatus": g.status,
                "ProvisionedThroughput": { "ReadCapacityUnits": 0, "WriteCapacityUnits": 0 }
            })
        })
        .collect();

    let lsi: Vec<Value> = table
        .lsi
        .iter()
        .map(|l| {
            let ks: Vec<Value> = l
                .key_schema
                .iter()
                .map(|k| json!({ "AttributeName": k.attribute_name, "KeyType": k.key_type }))
                .collect();
            json!({
                "IndexName": l.index_name,
                "KeySchema": ks,
                "Projection": {
                    "ProjectionType": l.projection.projection_type,
                    "NonKeyAttributes": l.projection.non_key_attributes
                }
            })
        })
        .collect();

    let mut desc = json!({
        "TableName": table.name,
        "TableArn": table.arn,
        "TableStatus": table.status,
        "CreationDateTime": table.created_at,
        "KeySchema": key_schema,
        "AttributeDefinitions": attr_defs,
        "BillingModeSummary": { "BillingMode": table.billing_mode },
        "ItemCount": table.item_count(),
        "TableSizeBytes": 0,
        "ProvisionedThroughput": { "ReadCapacityUnits": 0, "WriteCapacityUnits": 0 }
    });

    if !gsi.is_empty() {
        desc["GlobalSecondaryIndexes"] = json!(gsi);
    }
    if !lsi.is_empty() {
        desc["LocalSecondaryIndexes"] = json!(lsi);
    }

    desc
}

pub fn create_table(
    state: &DynamoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    if state.tables.contains_key(table_name) {
        return Err(AwsError::conflict(
            "ResourceInUseException",
            format!("Table already exists: {table_name}"),
        ));
    }

    let key_schema = parse_key_schema(&input["KeySchema"]);
    if key_schema.is_empty() {
        return Err(AwsError::validation("KeySchema is required"));
    }

    let attribute_definitions = parse_attribute_definitions(&input["AttributeDefinitions"]);
    let billing_mode = opt_str(input, "BillingMode")
        .unwrap_or("PAY_PER_REQUEST")
        .to_string();

    let gsi = input
        .get("GlobalSecondaryIndexes")
        .map(|v| parse_gsi(v))
        .unwrap_or_default();

    let lsi = input
        .get("LocalSecondaryIndexes")
        .map(|v| parse_lsi(v))
        .unwrap_or_default();

    let arn = format!(
        "arn:aws:dynamodb:{}:{}:table/{}",
        ctx.region, ctx.account_id, table_name
    );

    let table = Table {
        name: table_name.to_string(),
        arn,
        key_schema,
        attribute_definitions,
        billing_mode,
        status: "ACTIVE".to_string(),
        created_at: now_iso8601(),
        gsi,
        lsi,
        items: BTreeMap::new(),
    };

    let desc = table_description(&table);
    state.tables.insert(table_name.to_string(), table);
    info!(table = %table_name, "Created DynamoDB table");

    Ok(json!({ "TableDescription": desc }))
}

pub fn delete_table(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    let (_, table) = state.tables.remove(table_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    let desc = table_description(&table);
    info!(table = %table_name, "Deleted DynamoDB table");
    Ok(json!({ "TableDescription": desc }))
}

pub fn describe_table(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    let table = state.tables.get(table_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    Ok(json!({ "Table": table_description(&table) }))
}

pub fn list_tables(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let limit = input
        .get("Limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(100) as usize;
    let exclusive_start = opt_str(input, "ExclusiveStartTableName");

    // Collect sorted table names
    let mut names: Vec<String> = state
        .tables
        .iter()
        .map(|entry| entry.key().clone())
        .collect();
    names.sort();

    // Apply pagination
    let start_idx = if let Some(start) = exclusive_start {
        names.iter().position(|n| n == start).map(|i| i + 1).unwrap_or(0)
    } else {
        0
    };

    let page: Vec<String> = names[start_idx..].iter().take(limit).cloned().collect();
    let last = if page.len() == limit && start_idx + limit < names.len() {
        page.last().cloned()
    } else {
        None
    };

    let mut result = json!({ "TableNames": page });
    if let Some(last_name) = last {
        result["LastEvaluatedTableName"] = json!(last_name);
    }
    Ok(result)
}

pub fn update_table(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    let mut table = state.tables.get_mut(table_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    // Update billing mode if provided
    if let Some(billing_mode) = opt_str(input, "BillingMode") {
        table.billing_mode = billing_mode.to_string();
    }

    // Update GSI (add new ones from GlobalSecondaryIndexUpdates)
    if let Some(gsi_updates) = input.get("GlobalSecondaryIndexUpdates").and_then(|v| v.as_array()) {
        for update in gsi_updates {
            if let Some(create) = update.get("Create") {
                let index_name = create
                    .get("IndexName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let key_schema = parse_key_schema(&create["KeySchema"]);
                let projection =
                    parse_projection(&create.get("Projection").unwrap_or(&json!({})));
                table.gsi.push(GlobalSecondaryIndex {
                    index_name: index_name.clone(),
                    key_schema,
                    projection,
                    status: "ACTIVE".to_string(),
                });
            } else if let Some(delete) = update.get("Delete") {
                if let Some(index_name) = delete.get("IndexName").and_then(|v| v.as_str()) {
                    table.gsi.retain(|g| g.index_name != index_name);
                }
            }
        }
    }

    let desc = table_description(&table);
    Ok(json!({ "TableDescription": desc }))
}
