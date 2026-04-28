use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::sqlite_store::SqliteStore;
use crate::state::{
    AttributeDefinition, DynamoState, GlobalSecondaryIndex, KeySchemaElement, LocalSecondaryIndex,
    Projection, Table, TtlSpecification,
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
                    let projection = parse_projection(el.get("Projection").unwrap_or(&json!({})));
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
                    let projection = parse_projection(el.get("Projection").unwrap_or(&json!({})));
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

/// Build a DynamoDB `TableDescription` JSON shape.
///
/// `item_count` is the live row count from the SQLite store. We pass it
/// explicitly instead of querying inside `table_description` so the caller
/// can choose whether to pay the count query (it's cheap — covered by the
/// PRIMARY KEY index — but `ListTables` etc. don't need it).
pub fn table_description(table: &Table, item_count: u64) -> Value {
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
        "ItemCount": item_count,
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
    sqlite: &SqliteStore,
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
        .map(parse_gsi)
        .unwrap_or_default();

    let lsi = input
        .get("LocalSecondaryIndexes")
        .map(parse_lsi)
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
        stream_enabled,
        stream_arn,
        stream_view_type,
        stream_records: Vec::new(),
        stream_sequence: 0,
        ttl: TtlSpecification::default(),
        tags,
    };

    // Brand new table — item count is always 0, no need to query SQLite.
    let desc = table_description(&table, 0);

    // Mirror the schema to SQLite so future reads (and a process restart
    // after stage 4) can repopulate the in-memory state.
    let schema_value = serde_json::to_value(&table)
        .map_err(|e| AwsError::internal(format!("DynamoDB schema serialize failed: {e}")))?;
    sqlite.put_table_schema(&ctx.account_id, &ctx.region, table_name, &schema_value)?;

    state.tables.insert(table_name.to_string(), table);
    info!(table = %table_name, "Created DynamoDB table");

    Ok(json!({ "TableDescription": desc }))
}

/// `TruncateTable` — awsim-only op. Wipes every item in a table while
/// keeping the schema, GSIs, and stream config intact. Useful for the
/// UI's "reset between tests" workflow; no equivalent in real DynamoDB
/// (you'd have to DeleteTable + CreateTable, which loses streams).
pub fn truncate_table(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    if !state.tables.contains_key(table_name) {
        return Err(AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        ));
    }

    let removed = sqlite.truncate_table(&ctx.account_id, &ctx.region, table_name)?;
    info!(table = %table_name, removed, "Truncated DynamoDB table");
    Ok(json!({
        "TableName": table_name,
        "DeletedItemCount": removed,
    }))
}

pub fn delete_table(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    let (_, table) = state.tables.remove(table_name).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    // Capture the row count BEFORE dropping the SQLite mirror so the
    // returned TableDescription reports the right ItemCount.
    let count = sqlite
        .count_items(&ctx.account_id, &ctx.region, table_name)
        .unwrap_or(0);
    let _ = sqlite.drop_table(&ctx.account_id, &ctx.region, table_name)?;

    let desc = table_description(&table, count);
    info!(table = %table_name, "Deleted DynamoDB table");
    Ok(json!({ "TableDescription": desc }))
}

pub fn describe_table(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    let table = state.tables.get(table_name).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    let count = sqlite
        .count_items(&ctx.account_id, &ctx.region, table_name)
        .unwrap_or(0);
    Ok(json!({ "Table": table_description(&table, count) }))
}

pub fn list_tables(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let limit = input.get("Limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
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
        names
            .iter()
            .position(|n| n == start)
            .map(|i| i + 1)
            .unwrap_or(0)
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
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    let mut table = state.tables.get_mut(table_name).ok_or_else(|| {
        AwsError::service_not_found(
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
    if let Some(gsi_updates) = input
        .get("GlobalSecondaryIndexUpdates")
        .and_then(|v| v.as_array())
    {
        for update in gsi_updates {
            if let Some(create) = update.get("Create") {
                let index_name = create
                    .get("IndexName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let key_schema = parse_key_schema(&create["KeySchema"]);
                let projection = parse_projection(create.get("Projection").unwrap_or(&json!({})));
                table.gsi.push(GlobalSecondaryIndex {
                    index_name: index_name.clone(),
                    key_schema,
                    projection,
                    status: "ACTIVE".to_string(),
                });
            } else if let Some(delete) = update.get("Delete")
                && let Some(index_name) = delete.get("IndexName").and_then(|v| v.as_str())
            {
                table.gsi.retain(|g| g.index_name != index_name);
            }
        }
    }

    let count = sqlite
        .count_items(&ctx.account_id, &ctx.region, table_name)
        .unwrap_or(0);
    let desc = table_description(&table, count);
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
        AwsError::service_not_found(
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
        AwsError::service_not_found(
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
        return Err(AwsError::service_not_found(
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
            AwsError::service_not_found(
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
            AwsError::service_not_found(
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
            AwsError::service_not_found(
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

// ─── DescribeLimits ───────────────────────────────────────────────────────────

/// DescribeLimits — Return default account-level DynamoDB limits.
/// Terraform calls this on every plan to check provisioned-throughput limits.
pub fn describe_limits(
    _state: &DynamoState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({
        "AccountMaxReadCapacityUnits": 80000,
        "AccountMaxWriteCapacityUnits": 80000,
        "TableMaxReadCapacityUnits": 40000,
        "TableMaxWriteCapacityUnits": 40000
    }))
}

// ─── Global Table stubs ───────────────────────────────────────────────────────

/// DescribeGlobalTable — Always returns not-found.
/// Terraform checks for global table existence before creating.
pub fn describe_global_table(
    _state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "GlobalTableName")?;
    Err(AwsError::service_not_found(
        "GlobalTableNotFoundException",
        format!("Global table '{name}' not found"),
    ))
}

/// ListGlobalTables — Return an empty list.
pub fn list_global_tables(
    _state: &DynamoState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "GlobalTables": [], "LastEvaluatedGlobalTableName": null }))
}

// ─── Export ───────────────────────────────────────────────────────────────────

pub fn describe_export(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let export_arn = require_str(input, "ExportArn")?;
    let record = state.exports.get(export_arn).ok_or_else(|| {
        AwsError::service_not_found(
            "ExportNotFoundException",
            format!("Export not found: {export_arn}"),
        )
    })?;

    Ok(json!({
        "ExportDescription": {
            "ExportArn": record.export_arn,
            "ExportStatus": record.export_status,
            "TableArn": record.table_arn,
            "ExportFormat": record.export_format,
            "S3Bucket": record.s3_bucket,
            "S3Prefix": record.s3_prefix,
            "StartTime": record.start_time,
            "EndTime": record.end_time
        }
    }))
}

pub fn export_table_to_point_in_time(
    state: &DynamoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_arn = require_str(input, "TableArn")?;
    let exists = state.tables.iter().any(|e| e.value().arn == table_arn);
    if !exists {
        return Err(AwsError::service_not_found(
            "TableNotFoundException",
            format!("Table not found: {table_arn}"),
        ));
    }

    let s3_bucket = require_str(input, "S3Bucket")?.to_string();
    let s3_prefix = opt_str(input, "S3Prefix").map(|s| s.to_string());
    let format = opt_str(input, "ExportFormat")
        .unwrap_or("DYNAMODB_JSON")
        .to_string();

    let now = now_epoch_f64();
    let export_arn = format!("{}/export/{:016.0}", table_arn, now);
    let _ = ctx;

    let record = crate::state::ExportRecord {
        export_arn: export_arn.clone(),
        table_arn: table_arn.to_string(),
        export_status: "COMPLETED".to_string(),
        export_format: format.clone(),
        s3_bucket: s3_bucket.clone(),
        s3_prefix: s3_prefix.clone(),
        start_time: now,
        end_time: Some(now),
    };

    state.exports.insert(export_arn.clone(), record);

    Ok(json!({
        "ExportDescription": {
            "ExportArn": export_arn,
            "ExportStatus": "IN_PROGRESS",
            "TableArn": table_arn,
            "ExportFormat": format,
            "S3Bucket": s3_bucket,
            "S3Prefix": s3_prefix,
            "StartTime": now
        }
    }))
}

pub fn list_exports(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_arn_filter = opt_str(input, "TableArn");

    let summaries: Vec<Value> = state
        .exports
        .iter()
        .filter(|e| {
            table_arn_filter
                .map(|arn| e.value().table_arn == arn)
                .unwrap_or(true)
        })
        .map(|e| {
            json!({
                "ExportArn": e.value().export_arn,
                "ExportStatus": e.value().export_status,
                "ExportType": "FULL_EXPORT"
            })
        })
        .collect();

    Ok(json!({ "ExportSummaries": summaries }))
}

// ─── Import ───────────────────────────────────────────────────────────────────

pub fn describe_import(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let import_arn = require_str(input, "ImportArn")?;
    let record = state.imports.get(import_arn).ok_or_else(|| {
        AwsError::service_not_found(
            "ImportNotFoundException",
            format!("Import not found: {import_arn}"),
        )
    })?;

    Ok(json!({
        "ImportTableDescription": {
            "ImportArn": record.import_arn,
            "ImportStatus": record.import_status,
            "TableArn": record.table_arn,
            "TableId": "00000000-0000-0000-0000-000000000000",
            "InputFormat": record.input_format,
            "S3BucketSource": {
                "S3Bucket": record.s3_bucket
            },
            "StartTime": record.start_time,
            "EndTime": record.end_time
        }
    }))
}

pub fn import_table(
    state: &DynamoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let params = input.get("TableCreationParameters").ok_or_else(|| {
        AwsError::bad_request("ValidationException", "TableCreationParameters is required")
    })?;
    let table_name = params
        .get("TableName")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("ValidationException", "TableName is required"))?;

    let s3_source = input.get("S3BucketSource").ok_or_else(|| {
        AwsError::bad_request("ValidationException", "S3BucketSource is required")
    })?;
    let s3_bucket = s3_source
        .get("S3Bucket")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let input_format = opt_str(input, "InputFormat")
        .unwrap_or("DYNAMODB_JSON")
        .to_string();

    let now = now_epoch_f64();
    let table_arn = format!(
        "arn:aws:dynamodb:{}:{}:table/{}",
        ctx.region, ctx.account_id, table_name
    );
    let import_arn = format!(
        "arn:aws:dynamodb:{}:{}:table/{}/import/{:016.0}",
        ctx.region, ctx.account_id, table_name, now
    );

    let record = crate::state::ImportRecord {
        import_arn: import_arn.clone(),
        table_arn: table_arn.clone(),
        table_name: table_name.to_string(),
        import_status: "IN_PROGRESS".to_string(),
        input_format: input_format.clone(),
        s3_bucket: s3_bucket.clone(),
        start_time: now,
        end_time: None,
    };

    state.imports.insert(import_arn.clone(), record);

    Ok(json!({
        "ImportTableDescription": {
            "ImportArn": import_arn,
            "ImportStatus": "IN_PROGRESS",
            "TableArn": table_arn,
            "TableId": "00000000-0000-0000-0000-000000000000",
            "InputFormat": input_format,
            "S3BucketSource": {
                "S3Bucket": s3_bucket
            },
            "StartTime": now
        }
    }))
}

pub fn list_imports(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_arn_filter = opt_str(input, "TableArn");

    let summaries: Vec<Value> = state
        .imports
        .iter()
        .filter(|e| {
            table_arn_filter
                .map(|arn| e.value().table_arn == arn)
                .unwrap_or(true)
        })
        .map(|e| {
            json!({
                "ImportArn": e.value().import_arn,
                "ImportStatus": e.value().import_status,
                "TableArn": e.value().table_arn,
                "S3BucketSource": {
                    "S3Bucket": e.value().s3_bucket
                },
                "InputFormat": e.value().input_format,
                "StartTime": e.value().start_time,
                "EndTime": e.value().end_time
            })
        })
        .collect();

    Ok(json!({ "ImportSummaryList": summaries }))
}

// ─── Contributor Insights stubs ───────────────────────────────────────────────

/// DescribeContributorInsights — Return DISABLED status for any table.
pub fn describe_contributor_insights(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    if !state.tables.contains_key(table_name) {
        return Err(AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Table '{table_name}' not found"),
        ));
    }

    Ok(json!({
        "TableName": table_name,
        "ContributorInsightsStatus": "DISABLED",
        "ContributorInsightsRuleList": [],
        "FailureException": null
    }))
}

/// UpdateContributorInsights — Stub: acknowledge and return DISABLED.
pub fn update_contributor_insights(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    if !state.tables.contains_key(table_name) {
        return Err(AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Table '{table_name}' not found"),
        ));
    }

    let status = input
        .get("ContributorInsightsAction")
        .and_then(|v| v.as_str())
        .unwrap_or("DISABLE");

    let new_status = if status == "ENABLE" {
        "ENABLING"
    } else {
        "DISABLING"
    };

    Ok(json!({
        "TableName": table_name,
        "ContributorInsightsStatus": new_status
    }))
}

/// ListContributorInsights — Return an empty list.
pub fn list_contributor_insights(
    _state: &DynamoState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "ContributorInsightsSummaries": [] }))
}
