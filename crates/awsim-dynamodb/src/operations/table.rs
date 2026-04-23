use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{
    AttributeDefinition, DynamoState, GlobalSecondaryIndex, KeySchemaElement,
    LocalSecondaryIndex, Projection, Table, TtlSpecification,
};

use super::{opt_str, require_str};

fn now_epoch_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
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

    if table.stream_enabled {
        desc["StreamSpecification"] = json!({
            "StreamEnabled": true,
            "StreamViewType": table.stream_view_type.as_deref().unwrap_or("NEW_AND_OLD_IMAGES"),
        });
        if let Some(ref arn) = table.stream_arn {
            desc["LatestStreamArn"] = json!(arn);
            desc["LatestStreamLabel"] = json!(arn.rsplit('/').next().unwrap_or(""));
        }
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

    // Parse optional StreamSpecification.
    let (stream_enabled, stream_arn, stream_view_type) = {
        if let Some(spec) = input.get("StreamSpecification") {
            let enabled = spec
                .get("StreamEnabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if enabled {
                let view_type = spec
                    .get("StreamViewType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("NEW_AND_OLD_IMAGES")
                    .to_string();
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let stream_arn = format!(
                    "arn:aws:dynamodb:{}:{}:table/{}/stream/{}",
                    ctx.region, ctx.account_id, table_name, timestamp
                );
                (true, Some(stream_arn), Some(view_type))
            } else {
                (false, None, None)
            }
        } else {
            (false, None, None)
        }
    };

    // Parse optional tags from CreateTable input
    let tags = {
        let mut map = std::collections::HashMap::new();
        if let Some(tag_arr) = input.get("Tags").and_then(|v| v.as_array()) {
            for tag in tag_arr {
                if let (Some(k), Some(v)) = (
                    tag.get("Key").and_then(|v| v.as_str()),
                    tag.get("Value").and_then(|v| v.as_str()),
                ) {
                    map.insert(k.to_string(), v.to_string());
                }
            }
        }
        map
    };

    let table = Table {
        name: table_name.to_string(),
        arn,
        key_schema,
        attribute_definitions,
        billing_mode,
        status: "ACTIVE".to_string(),
        created_at: now_epoch_f64(),
        gsi,
        lsi,
        items: BTreeMap::new(),
        stream_enabled,
        stream_arn,
        stream_view_type,
        stream_records: Vec::new(),
        stream_sequence: 0,
        ttl: TtlSpecification::default(),
        tags,
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

    // Update StreamSpecification if provided
    if let Some(spec) = input.get("StreamSpecification") {
        let enabled = spec
            .get("StreamEnabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if enabled && !table.stream_enabled {
            let view_type = spec
                .get("StreamViewType")
                .and_then(|v| v.as_str())
                .unwrap_or("NEW_AND_OLD_IMAGES")
                .to_string();
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            table.stream_arn = Some(format!(
                "arn:aws:dynamodb:{}:table/{}/stream/{}",
                // region/account from table ARN: arn:aws:dynamodb:{region}:{account}:table/{name}
                {
                    let parts: Vec<&str> = table.arn.splitn(6, ':').collect();
                    if parts.len() == 6 {
                        format!("{}:{}", parts[3], parts[4])
                    } else {
                        "us-east-1:000000000000".to_string()
                    }
                },
                table.name,
                timestamp
            ));
            table.stream_view_type = Some(view_type);
            table.stream_enabled = true;
        } else if !enabled {
            table.stream_enabled = false;
        }
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

// ─── DescribeEndpoints ────────────────────────────────────────────────────────

/// DescribeEndpoints — SDK endpoint discovery stub.
/// Returns a single local endpoint so the SDK's endpoint-discovery logic is
/// satisfied without making external calls.
pub fn describe_endpoints(
    _state: &DynamoState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({
        "Endpoints": [
            {
                "Address": "localhost",
                "CachePeriodInMinutes": 1440
            }
        ]
    }))
}

// ─── Time-to-Live ─────────────────────────────────────────────────────────────

/// DescribeTimeToLive — Return current TTL configuration for a table.
pub fn describe_time_to_live(
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

    let (status, attr_name) = if table.ttl.enabled {
        ("ENABLED", Some(table.ttl.attribute_name.clone()))
    } else {
        ("DISABLED", None)
    };

    let mut ttl_desc = json!({ "TimeToLiveStatus": status });
    if let Some(attr) = attr_name {
        ttl_desc["AttributeName"] = json!(attr);
    }

    Ok(json!({ "TimeToLiveDescription": ttl_desc }))
}

/// UpdateTimeToLive — Enable or disable TTL on a table.
pub fn update_time_to_live(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    let spec = input.get("TimeToLiveSpecification").ok_or_else(|| {
        AwsError::bad_request("ValidationException", "TimeToLiveSpecification is required")
    })?;

    let enabled = spec
        .get("Enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let attribute_name = spec
        .get("AttributeName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut table = state.tables.get_mut(table_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    table.ttl = TtlSpecification {
        enabled,
        attribute_name: attribute_name.clone(),
    };

    Ok(json!({
        "TimeToLiveSpecification": {
            "Enabled": enabled,
            "AttributeName": attribute_name
        }
    }))
}

// ─── Continuous Backups ───────────────────────────────────────────────────────

/// DescribeContinuousBackups — Return stub backup configuration for a table.
pub fn describe_continuous_backups(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    // Verify the table exists.
    if !state.tables.contains_key(table_name) {
        return Err(AwsError::not_found(
            "TableNotFoundException",
            format!("Table '{table_name}' not found"),
        ));
    }

    Ok(json!({
        "ContinuousBackupsDescription": {
            "ContinuousBackupsStatus": "ENABLED",
            "PointInTimeRecoveryDescription": {
                "PointInTimeRecoveryStatus": "DISABLED"
            }
        }
    }))
}

// ─── Tagging ─────────────────────────────────────────────────────────────────

/// TagResource — Add or overwrite tags on a table.
pub fn tag_resource(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = require_str(input, "ResourceArn")?;

    // Find the table by ARN.
    let table_name = state
        .tables
        .iter()
        .find(|e| e.value().arn == resource_arn)
        .map(|e| e.key().clone())
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Resource not found: {resource_arn}"),
            )
        })?;

    let tags: std::collections::HashMap<String, String> = input
        .get("Tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|tag| {
                    let k = tag.get("Key")?.as_str()?.to_string();
                    let v = tag.get("Value")?.as_str()?.to_string();
                    Some((k, v))
                })
                .collect()
        })
        .unwrap_or_default();

    if let Some(mut table) = state.tables.get_mut(&table_name) {
        table.tags.extend(tags);
    }

    Ok(json!({}))
}

/// UntagResource — Remove tags from a table.
pub fn untag_resource(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = require_str(input, "ResourceArn")?;

    let table_name = state
        .tables
        .iter()
        .find(|e| e.value().arn == resource_arn)
        .map(|e| e.key().clone())
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Resource not found: {resource_arn}"),
            )
        })?;

    let tag_keys: Vec<String> = input
        .get("TagKeys")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    if let Some(mut table) = state.tables.get_mut(&table_name) {
        for key in &tag_keys {
            table.tags.remove(key);
        }
    }

    Ok(json!({}))
}

/// ListTagsOfResource — List all tags on a table.
pub fn list_tags_of_resource(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = require_str(input, "ResourceArn")?;

    let table = state
        .tables
        .iter()
        .find(|e| e.value().arn == resource_arn)
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Resource not found: {resource_arn}"),
            )
        })?;

    let tags: Vec<Value> = table
        .value()
        .tags
        .iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();

    Ok(json!({ "Tags": tags }))
}
