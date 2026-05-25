use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::sqlite_store::SqliteStore;
use crate::state::{
    AttributeDefinition, DynamoState, GlobalSecondaryIndex, KeySchemaElement, LocalSecondaryIndex,
    Projection, SseSpecification, Table, TtlSpecification,
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

/// AWS rejects CreateTable / UpdateTable when a KeySchema names an
/// attribute that isn't present in AttributeDefinitions, with a
/// ValidationException listing the offending names. awsim used to
/// accept those silently and the resulting GSI was just unusable
/// (its key column came back empty for every item).
///
/// `key_schema_owners` is `(label, KeySchema)` per source so the error
/// message can point at the responsible block (the base table itself,
/// or a specific GSI / LSI by name).
fn validate_key_schema_against_attrs(
    attrs: &[AttributeDefinition],
    key_schema_owners: &[(&str, &[KeySchemaElement])],
) -> Result<(), AwsError> {
    let declared: std::collections::HashSet<&str> =
        attrs.iter().map(|a| a.attribute_name.as_str()).collect();
    let mut missing: Vec<String> = Vec::new();
    for (owner, schema) in key_schema_owners {
        for ke in *schema {
            if !declared.contains(ke.attribute_name.as_str()) {
                missing.push(format!("{}.{}", owner, ke.attribute_name));
            }
        }
    }
    if !missing.is_empty() {
        return Err(AwsError::validation(format!(
            "One or more parameter values were invalid: \
             Some index key attributes are not defined in AttributeDefinitions. \
             Missing: [{}]",
            missing.join(", ")
        )));
    }
    Ok(())
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
        "ProvisionedThroughput": {
            "ReadCapacityUnits": table.read_capacity_units,
            "WriteCapacityUnits": table.write_capacity_units,
            "NumberOfDecreasesToday": 0,
        },
        "DeletionProtectionEnabled": table.deletion_protection_enabled,
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

    // AWS only emits SSEDescription when customer-managed SSE is on.
    // Default AWS-owned-key encryption is implicit and silent — match
    // that so SDK round-tripping is faithful.
    if table.sse.enabled {
        let mut sse_desc = json!({
            "Status": "ENABLED",
            "SSEType": if table.sse.sse_type.is_empty() { "KMS" } else { table.sse.sse_type.as_str() },
        });
        if let Some(arn) = table.sse.kms_master_key_arn.as_ref() {
            sse_desc["KMSMasterKeyArn"] = json!(arn);
        }
        desc["SSEDescription"] = sse_desc;
    }

    desc
}

/// Parse a `ProvisionedThroughput` block. Returns `(read, write)`
/// in capacity units. Missing block / fields default to 0 — that
/// matches the AWS shape where PAY_PER_REQUEST tables report 0/0.
fn parse_provisioned_throughput(spec: Option<&Value>) -> (u64, u64) {
    let Some(spec) = spec else { return (0, 0) };
    let r = spec
        .get("ReadCapacityUnits")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let w = spec
        .get("WriteCapacityUnits")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    (r, w)
}

/// Parse an `SSESpecification` block from a CreateTable / UpdateTable
/// request body. Returns the default (disabled / AWS-owned-key) when
/// the block is absent or `Enabled = false`.
fn parse_sse_specification(spec: Option<&Value>) -> SseSpecification {
    let Some(spec) = spec else {
        return SseSpecification::default();
    };
    let enabled = spec
        .get("Enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !enabled {
        return SseSpecification::default();
    }
    // AWS defaults to "KMS" when Enabled is true and SSEType is omitted.
    let sse_type = spec
        .get("SSEType")
        .and_then(Value::as_str)
        .unwrap_or("KMS")
        .to_string();
    let kms_master_key_arn = spec
        .get("KMSMasterKeyId")
        .and_then(Value::as_str)
        .map(String::from);
    SseSpecification {
        enabled: true,
        sse_type,
        kms_master_key_arn,
    }
}

pub fn create_table(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    if state.tables.contains_key(table_name) {
        return Err(AwsError::bad_request(
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

    // Reject CreateTable when an index references an attribute that
    // isn't declared in AttributeDefinitions. AWS surfaces the same
    // check as a ValidationException; we'd previously accept it and
    // the GSI/LSI would just stay empty for every item written.
    let mut owners: Vec<(&str, &[KeySchemaElement])> = Vec::new();
    owners.push(("KeySchema", &key_schema));
    for g in &gsi {
        owners.push((g.index_name.as_str(), &g.key_schema));
    }
    for l in &lsi {
        owners.push((l.index_name.as_str(), &l.key_schema));
    }
    validate_key_schema_against_attrs(&attribute_definitions, &owners)?;

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

    let deletion_protection_enabled = input
        .get("DeletionProtectionEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let sse = parse_sse_specification(input.get("SSESpecification"));

    // Provisioned capacity: only meaningful for the PROVISIONED
    // billing mode, but we honour explicit values regardless so a
    // round-trip preserves them when the user later switches modes.
    let (read_capacity_units, write_capacity_units) =
        parse_provisioned_throughput(input.get("ProvisionedThroughput"));

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
        stream_records: VecDeque::new(),
        stream_sequence: 0,
        ttl: TtlSpecification::default(),
        tags,
        deletion_protection_enabled,
        sse,
        read_capacity_units,
        write_capacity_units,
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

    // Reject the request if deletion protection is on. Mirrors AWS:
    // the table stays put and the caller must flip the flag off via
    // UpdateTable first. Done before `remove` so we don't need to
    // re-insert on rejection.
    if let Some(table_ref) = state.tables.get(table_name)
        && table_ref.value().deletion_protection_enabled
    {
        return Err(AwsError::validation(format!(
            "Table '{table_name}' has deletion protection enabled. Disable it via UpdateTable first."
        )));
    }

    let (_, table) = state.tables.remove(table_name).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    // Drop any per-table token bucket so a recreated table starts
    // with a fresh burst-credit window rather than inheriting the
    // depleted state of its predecessor.
    state.throttle.forget(table_name);

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

    // Update DeletionProtectionEnabled if provided.
    if let Some(flag) = input
        .get("DeletionProtectionEnabled")
        .and_then(Value::as_bool)
    {
        table.deletion_protection_enabled = flag;
    }

    // Update SSE if provided. AWS only allows enabling via UpdateTable;
    // disabling requires re-creating the table. We're a dev tool so
    // we accept either direction for ergonomic use.
    if let Some(sse_spec) = input.get("SSESpecification") {
        table.sse = parse_sse_specification(Some(sse_spec));
    }

    // Update provisioned capacity if a `ProvisionedThroughput` block
    // is supplied. AWS allows this without changing billing mode in
    // the same call, so we apply it independently.
    if let Some(pt) = input.get("ProvisionedThroughput") {
        let (r, w) = parse_provisioned_throughput(Some(pt));
        table.read_capacity_units = r;
        table.write_capacity_units = w;
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
                "arn:aws:dynamodb:{}:{}:table/{}/stream/{}",
                ctx.region, ctx.account_id, table.name, timestamp
            ));
            table.stream_view_type = Some(view_type);
            table.stream_enabled = true;
        } else if !enabled {
            table.stream_enabled = false;
        }
    }

    // Update GSI (add new ones from GlobalSecondaryIndexUpdates).
    // Track whether the index set changed: if so, re-project every
    // existing item's GSI key columns. AWS handles this as an
    // asynchronous backfill (status CREATING -> ACTIVE), but a local
    // emulator can do it synchronously: small enough tables to make
    // it tractable, and tests immediately get queryable results.
    let mut gsi_set_changed = false;
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
                // The new GSI's KeySchema attributes must already be in
                // AttributeDefinitions (which UpdateTable lets you augment
                // via the optional AttributeDefinitions field). Build the
                // effective set: existing definitions plus any new ones
                // the caller is providing in this same call.
                let mut effective_attrs = table.attribute_definitions.clone();
                if let Some(arr) = input.get("AttributeDefinitions").and_then(|v| v.as_array()) {
                    for el in arr {
                        let Some(name) = el.get("AttributeName").and_then(|v| v.as_str()) else {
                            continue;
                        };
                        let Some(ty) = el.get("AttributeType").and_then(|v| v.as_str()) else {
                            continue;
                        };
                        if !effective_attrs.iter().any(|a| a.attribute_name == name) {
                            effective_attrs.push(AttributeDefinition {
                                attribute_name: name.to_string(),
                                attribute_type: ty.to_string(),
                            });
                        }
                    }
                }
                let owners: Vec<(&str, &[KeySchemaElement])> =
                    vec![(index_name.as_str(), &key_schema)];
                validate_key_schema_against_attrs(&effective_attrs, &owners)?;
                table.attribute_definitions = effective_attrs;
                // Real AWS reports CREATING while the backfill is in
                // flight and ACTIVE once the index is queryable. Even
                // though awsim runs the backfill synchronously, we set
                // the status to CREATING here so a concurrent
                // DescribeTable seen between this lock drop and the
                // post-backfill flip below sees an honest snapshot.
                table.gsi.push(GlobalSecondaryIndex {
                    index_name: index_name.clone(),
                    key_schema,
                    projection,
                    status: "CREATING".to_string(),
                });
                gsi_set_changed = true;
            } else if let Some(delete) = update.get("Delete")
                && let Some(index_name) = delete.get("IndexName").and_then(|v| v.as_str())
            {
                table.gsi.retain(|g| g.index_name != index_name);
                gsi_set_changed = true;
            }
        }
    }

    if gsi_set_changed {
        // Snapshot the table schema and drop the dashmap guard so the
        // SQLite scan + reprojection pass doesn't hold the cross-shard
        // lock for the duration.
        let snapshot = table.clone();
        drop(table);
        reproject_gsi_columns(sqlite, ctx, &snapshot)?;
        // Backfill complete: flip every CREATING index to ACTIVE.
        // We grab a fresh mutable borrow so a parallel DescribeTable
        // that came in during the reprojection sees CREATING, then
        // immediately the post-flip ACTIVE on its next call.
        if let Some(mut t) = state.tables.get_mut(table_name) {
            for g in t.gsi.iter_mut() {
                if g.status == "CREATING" || g.status == "UPDATING" {
                    g.status = "ACTIVE".to_string();
                }
            }
        }
        let table = state.tables.get(table_name).ok_or_else(|| {
            AwsError::service_not_found(
                "ResourceNotFoundException",
                format!("Table disappeared during GSI reprojection: {table_name}"),
            )
        })?;
        let count = sqlite
            .count_items(&ctx.account_id, &ctx.region, table_name)
            .unwrap_or(0);
        let desc = table_description(&table, count);
        return Ok(json!({ "TableDescription": desc }));
    }

    let count = sqlite
        .count_items(&ctx.account_id, &ctx.region, table_name)
        .unwrap_or(0);
    let desc = table_description(&table, count);
    Ok(json!({ "TableDescription": desc }))
}

/// Walk every item in `table` and rewrite its `gsi{N}_pk` / `gsi{N}_sk`
/// columns to match the current `table.gsi` set. Used after a GSI is
/// added or removed so existing items are visible (or no longer
/// visible) to queries against the affected index.
///
/// This is the synchronous local-emulator equivalent of AWS's async
/// GSI backfill, where new items would land in CREATING state and
/// get backfilled in the background. We pull the full row set into
/// memory first to avoid holding both a read cursor and a write
/// connection at the same time; tables in awsim are intentionally
/// small enough that this is fine, and a `--data-dir` deployment
/// with very large tables can still use the operation - it just
/// takes proportional time.
fn reproject_gsi_columns(
    sqlite: &SqliteStore,
    ctx: &RequestContext,
    table: &crate::state::Table,
) -> Result<(), AwsError> {
    let mut rows: Vec<(String, String, serde_json::Value)> = Vec::new();
    sqlite.scan_table(
        &ctx.account_id,
        &ctx.region,
        &table.name,
        None,
        |pk, sk, attrs| {
            rows.push((pk.to_string(), sk.to_string(), attrs));
            Ok(true)
        },
    )?;
    for (pk, sk, attrs) in rows {
        let item = match crate::keys::storage_value_to_item(attrs.clone()) {
            Some(i) => i,
            None => continue,
        };
        let keys = match crate::keys::extract_item_keys(table, &item) {
            Some(k) => k,
            None => continue,
        };
        sqlite.put_item(
            &ctx.account_id,
            &ctx.region,
            &table.name,
            &pk,
            &sk,
            &attrs,
            &keys.gsi,
        )?;
    }
    Ok(())
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

// ─── Global Tables ────────────────────────────────────────────────────────────
//
// AWSim doesn't perform cross-region replication. The global-table object is
// metadata only — the source-of-truth for "does my Terraform / CDK think this
// global table exists" — but reads and writes to the underlying tables stay
// per-region. That matches what `awslocal` style emulators do.

/// CreateGlobalTable — register a logical Global Table over per-region
/// replicas. Each named replica region must already host a table with the
/// same name (real DynamoDB requires this), but for emulator ergonomics we
/// only fail when the source region's table is missing.
pub fn create_global_table(
    state: &DynamoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    use crate::state::{GlobalTable, GlobalTableReplica};

    let name = require_str(input, "GlobalTableName")?;
    if state.global_tables.contains_key(name) {
        return Err(AwsError::bad_request(
            "GlobalTableAlreadyExistsException",
            format!("Global table '{name}' already exists"),
        ));
    }
    if !state.tables.contains_key(name) {
        return Err(AwsError::service_not_found(
            "TableNotFoundException",
            format!(
                "Cannot create global table '{name}': no table with that name exists in this region"
            ),
        ));
    }

    let replicas: Vec<GlobalTableReplica> = input
        .get("ReplicationGroup")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.get("RegionName")
                        .and_then(Value::as_str)
                        .map(|r| GlobalTableReplica {
                            region_name: r.to_string(),
                            replica_status: "ACTIVE".to_string(),
                        })
                })
                .collect()
        })
        .unwrap_or_else(|| {
            // Default to a single replica in the request region.
            vec![GlobalTableReplica {
                region_name: ctx.region.clone(),
                replica_status: "ACTIVE".to_string(),
            }]
        });

    let arn = format!("arn:aws:dynamodb::{}:global-table/{}", ctx.account_id, name);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();

    let global_table = GlobalTable {
        global_table_name: name.to_string(),
        global_table_arn: arn.clone(),
        creation_date: now,
        global_table_status: "ACTIVE".to_string(),
        replication_group: replicas.clone(),
    };
    state.global_tables.insert(name.to_string(), global_table);

    Ok(json!({
        "GlobalTableDescription": global_table_to_json(&state.global_tables.get(name).unwrap()),
    }))
}

/// UpdateGlobalTable — apply Create / Delete replica updates in a single
/// request, mirroring the AWS shape where the caller posts a list of
/// `ReplicaUpdates` containing `{Create: {RegionName}}` and / or
/// `{Delete: {RegionName}}` entries.
pub fn update_global_table(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    use crate::state::GlobalTableReplica;

    let name = require_str(input, "GlobalTableName")?;
    let mut global = state.global_tables.get_mut(name).ok_or_else(|| {
        AwsError::service_not_found(
            "GlobalTableNotFoundException",
            format!("Global table '{name}' not found"),
        )
    })?;

    let updates = input
        .get("ReplicaUpdates")
        .and_then(Value::as_array)
        .ok_or_else(|| AwsError::validation("ReplicaUpdates is required"))?;

    for update in updates {
        if let Some(region) = update
            .get("Create")
            .and_then(|c| c.get("RegionName"))
            .and_then(Value::as_str)
        {
            if global
                .replication_group
                .iter()
                .any(|r| r.region_name == region)
            {
                continue;
            }
            global.replication_group.push(GlobalTableReplica {
                region_name: region.to_string(),
                replica_status: "ACTIVE".to_string(),
            });
        } else if let Some(region) = update
            .get("Delete")
            .and_then(|d| d.get("RegionName"))
            .and_then(Value::as_str)
        {
            global.replication_group.retain(|r| r.region_name != region);
        }
    }

    let snapshot = global.clone();
    drop(global);
    Ok(json!({
        "GlobalTableDescription": global_table_to_json(&snapshot),
    }))
}

/// DescribeGlobalTable — return the stored metadata or
/// GlobalTableNotFoundException.
pub fn describe_global_table(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "GlobalTableName")?;
    let global = state.global_tables.get(name).ok_or_else(|| {
        AwsError::service_not_found(
            "GlobalTableNotFoundException",
            format!("Global table '{name}' not found"),
        )
    })?;
    Ok(json!({
        "GlobalTableDescription": global_table_to_json(&global),
    }))
}

/// ListGlobalTables — paginated by GlobalTableName, optionally filtered by
/// region (emulator returns only globals that include the given RegionName).
pub fn list_global_tables(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let limit = input
        .get("Limit")
        .and_then(Value::as_u64)
        .map(|n| n.clamp(1, 100) as usize)
        .unwrap_or(100);
    let region_filter = input.get("RegionName").and_then(Value::as_str);
    let exclusive = input
        .get("ExclusiveStartGlobalTableName")
        .and_then(Value::as_str);

    let mut names: Vec<String> = state
        .global_tables
        .iter()
        .filter(|e| match region_filter {
            None => true,
            Some(region) => e
                .value()
                .replication_group
                .iter()
                .any(|r| r.region_name == region),
        })
        .map(|e| e.key().clone())
        .collect();
    names.sort();

    let after_idx = exclusive
        .and_then(|name| names.iter().position(|n| n.as_str() == name))
        .map(|i| i + 1)
        .unwrap_or(0);
    let page: Vec<&String> = names.iter().skip(after_idx).take(limit).collect();
    let last = if names.len() > after_idx + page.len() {
        page.last().map(|s| s.to_string())
    } else {
        None
    };

    let global_tables: Vec<Value> = page
        .iter()
        .filter_map(|name| state.global_tables.get(name.as_str()))
        .map(|g| {
            json!({
                "GlobalTableName": g.global_table_name,
                "ReplicationGroup": g.replication_group.iter().map(|r| json!({
                    "RegionName": r.region_name,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();

    Ok(json!({
        "GlobalTables": global_tables,
        "LastEvaluatedGlobalTableName": last,
    }))
}

fn global_table_to_json(g: &crate::state::GlobalTable) -> Value {
    json!({
        "GlobalTableName": g.global_table_name,
        "GlobalTableArn": g.global_table_arn,
        "GlobalTableStatus": g.global_table_status,
        "CreationDateTime": g.creation_date,
        "ReplicationGroup": g.replication_group.iter().map(|r| json!({
            "RegionName": r.region_name,
            "ReplicaStatus": r.replica_status,
        })).collect::<Vec<_>>(),
    })
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

pub fn describe_table_replica_auto_scaling(
    state: &DynamoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;
    let _table = state.tables.get(table_name).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    Ok(json!({
        "TableAutoScalingDescription": {
            "TableName": table_name,
            "TableStatus": "ACTIVE",
            "Replicas": [{
                "RegionName": ctx.region,
                "GlobalSecondaryIndexes": [],
                "ReplicaProvisionedReadCapacityAutoScalingSettings": {
                    "MinimumUnits": 5,
                    "MaximumUnits": 100,
                    "ProvisionedReadCapacityUnits": 5,
                    "TargetTrackingScalingPolicyConfiguration": {
                        "TargetValue": 70.0
                    },
                    "AutoScalingDisabled": false
                },
                "ReplicaProvisionedWriteCapacityAutoScalingSettings": {
                    "MinimumUnits": 5,
                    "MaximumUnits": 100,
                    "ProvisionedWriteCapacityUnits": 5,
                    "TargetTrackingScalingPolicyConfiguration": {
                        "TargetValue": 70.0
                    },
                    "AutoScalingDisabled": false
                },
                "ReplicaStatus": "ACTIVE"
            }]
        }
    }))
}

pub fn update_table_replica_auto_scaling(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;
    let _table = state.tables.get(table_name).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    Ok(json!({}))
}

pub fn describe_global_table_settings(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "GlobalTableName")?;
    let _table = state.tables.get(table_name).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    Ok(json!({
        "GlobalTableSettingsDescription": {
            "GlobalTableName": table_name,
            "ReplicaSettings": [{
                "RegionName": _ctx.region,
                "ReplicaStatus": "ACTIVE",
                "ReplicaProvisionedReadCapacityUnits": 5,
                "ReplicaProvisionedWriteCapacityUnits": 5,
            }]
        }
    }))
}

pub fn update_global_table_settings(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "GlobalTableName")?;
    let _table = state.tables.get(table_name).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    Ok(json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{KeySchemaElement, Table};

    fn ctx() -> RequestContext {
        RequestContext::new("dynamodb", "us-east-1")
    }

    fn state_with_table(name: &str) -> DynamoState {
        let state = DynamoState::default();
        state.tables.insert(
            name.to_string(),
            Table {
                name: name.to_string(),
                arn: format!("arn:aws:dynamodb:us-east-1:000000000000:table/{name}"),
                key_schema: vec![KeySchemaElement {
                    attribute_name: "pk".into(),
                    key_type: "HASH".into(),
                }],
                attribute_definitions: vec![],
                billing_mode: "PAY_PER_REQUEST".into(),
                status: "ACTIVE".into(),
                created_at: 0.0,
                gsi: vec![],
                lsi: vec![],
                stream_enabled: false,
                stream_arn: None,
                stream_view_type: None,
                stream_records: VecDeque::new(),
                stream_sequence: 0,
                ttl: Default::default(),
                tags: Default::default(),
                deletion_protection_enabled: false,
                sse: Default::default(),
                read_capacity_units: 0,
                write_capacity_units: 0,
            },
        );
        state
    }

    #[test]
    fn create_describe_round_trip_with_default_replica() {
        let state = state_with_table("orders");
        create_global_table(&state, &json!({ "GlobalTableName": "orders" }), &ctx()).unwrap();
        let desc =
            describe_global_table(&state, &json!({ "GlobalTableName": "orders" }), &ctx()).unwrap();
        let regions: Vec<String> = desc["GlobalTableDescription"]["ReplicationGroup"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v["RegionName"].as_str().map(String::from))
            .collect();
        assert_eq!(regions, vec!["us-east-1".to_string()]);
    }

    #[test]
    fn update_global_table_creates_and_deletes_replicas() {
        let state = state_with_table("inventory");
        create_global_table(
            &state,
            &json!({
                "GlobalTableName": "inventory",
                "ReplicationGroup": [{ "RegionName": "us-east-1" }],
            }),
            &ctx(),
        )
        .unwrap();

        update_global_table(
            &state,
            &json!({
                "GlobalTableName": "inventory",
                "ReplicaUpdates": [
                    { "Create": { "RegionName": "eu-west-1" } },
                    { "Create": { "RegionName": "ap-south-1" } },
                ],
            }),
            &ctx(),
        )
        .unwrap();

        let desc =
            describe_global_table(&state, &json!({ "GlobalTableName": "inventory" }), &ctx())
                .unwrap();
        let mut regions: Vec<String> = desc["GlobalTableDescription"]["ReplicationGroup"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v["RegionName"].as_str().map(String::from))
            .collect();
        regions.sort();
        assert_eq!(
            regions,
            vec![
                "ap-south-1".to_string(),
                "eu-west-1".to_string(),
                "us-east-1".to_string(),
            ]
        );

        update_global_table(
            &state,
            &json!({
                "GlobalTableName": "inventory",
                "ReplicaUpdates": [
                    { "Delete": { "RegionName": "ap-south-1" } },
                ],
            }),
            &ctx(),
        )
        .unwrap();

        let desc =
            describe_global_table(&state, &json!({ "GlobalTableName": "inventory" }), &ctx())
                .unwrap();
        let mut regions: Vec<String> = desc["GlobalTableDescription"]["ReplicationGroup"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v["RegionName"].as_str().map(String::from))
            .collect();
        regions.sort();
        assert_eq!(
            regions,
            vec!["eu-west-1".to_string(), "us-east-1".to_string()]
        );
    }

    #[test]
    fn create_global_table_requires_underlying_table() {
        let state = DynamoState::default();
        let err = create_global_table(&state, &json!({ "GlobalTableName": "ghost" }), &ctx())
            .unwrap_err();
        assert_eq!(err.code, "TableNotFoundException");
    }

    #[test]
    fn list_global_tables_filters_by_region() {
        let state = state_with_table("a");
        state.tables.insert(
            "b".to_string(),
            Table {
                name: "b".into(),
                arn: "arn:aws:dynamodb:us-east-1:000000000000:table/b".into(),
                key_schema: vec![KeySchemaElement {
                    attribute_name: "pk".into(),
                    key_type: "HASH".into(),
                }],
                attribute_definitions: vec![],
                billing_mode: "PAY_PER_REQUEST".into(),
                status: "ACTIVE".into(),
                created_at: 0.0,
                gsi: vec![],
                lsi: vec![],
                stream_enabled: false,
                stream_arn: None,
                stream_view_type: None,
                stream_records: VecDeque::new(),
                stream_sequence: 0,
                ttl: Default::default(),
                tags: Default::default(),
                deletion_protection_enabled: false,
                sse: Default::default(),
                read_capacity_units: 0,
                write_capacity_units: 0,
            },
        );
        create_global_table(
            &state,
            &json!({
                "GlobalTableName": "a",
                "ReplicationGroup": [{ "RegionName": "us-east-1" }],
            }),
            &ctx(),
        )
        .unwrap();
        create_global_table(
            &state,
            &json!({
                "GlobalTableName": "b",
                "ReplicationGroup": [{ "RegionName": "eu-west-1" }],
            }),
            &ctx(),
        )
        .unwrap();
        let resp =
            list_global_tables(&state, &json!({ "RegionName": "eu-west-1" }), &ctx()).unwrap();
        let names: Vec<String> = resp["GlobalTables"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v["GlobalTableName"].as_str().map(String::from))
            .collect();
        assert_eq!(names, vec!["b".to_string()]);
    }

    #[test]
    fn update_table_add_gsi_backfills_existing_items() {
        use crate::operations::item::put_item;
        use crate::operations::query::query;
        use crate::sqlite_store::SqliteStore;

        let state = state_with_table("t");
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();

        // Seed three items before the GSI exists. They carry the future
        // GSI key attribute already; the index-maintenance step on
        // PutItem just doesn't have a GSI to project them into yet.
        for i in 0..3 {
            put_item(
                &state,
                &sqlite,
                &json!({
                    "TableName": "t",
                    "Item": {
                        "pk":  { "S": format!("p-{i}") },
                        "tag": { "S": "shared" },
                    },
                }),
                &c,
            )
            .unwrap();
        }

        // Querying the GSI before it exists must fail with the
        // 'no such index' validation error.
        let err = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "IndexName": "byTag",
                "KeyConditionExpression": "tag = :t",
                "ExpressionAttributeValues": { ":t": { "S": "shared" } },
            }),
            &c,
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");

        // Add the GSI through UpdateTable. This is where backfill must
        // happen: the three pre-existing items need their gsi1_pk
        // column set so the subsequent query finds them.
        update_table(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "AttributeDefinitions": [
                    { "AttributeName": "tag", "AttributeType": "S" }
                ],
                "GlobalSecondaryIndexUpdates": [{
                    "Create": {
                        "IndexName": "byTag",
                        "KeySchema": [{ "AttributeName": "tag", "KeyType": "HASH" }],
                        "Projection": { "ProjectionType": "ALL" }
                    }
                }]
            }),
            &c,
        )
        .unwrap();

        let resp = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "IndexName": "byTag",
                "KeyConditionExpression": "tag = :t",
                "ExpressionAttributeValues": { ":t": { "S": "shared" } },
            }),
            &c,
        )
        .unwrap();
        // All three pre-existing items must surface from the new index.
        assert_eq!(resp["Count"], json!(3));
    }

    #[test]
    fn update_table_drop_gsi_clears_columns() {
        use crate::operations::item::put_item;
        use crate::operations::query::query;
        use crate::sqlite_store::SqliteStore;
        use crate::state::{GlobalSecondaryIndex, Projection};

        // Start with a GSI in place, seed an item, then drop the GSI.
        // After the drop, querying it should fail with ValidationException
        // because the index no longer exists.
        let state = DynamoState::default();
        state.tables.insert(
            "t".to_string(),
            Table {
                name: "t".into(),
                arn: "arn:aws:dynamodb:us-east-1:000000000000:table/t".into(),
                key_schema: vec![KeySchemaElement {
                    attribute_name: "pk".into(),
                    key_type: "HASH".into(),
                }],
                attribute_definitions: vec![],
                billing_mode: "PAY_PER_REQUEST".into(),
                status: "ACTIVE".into(),
                created_at: 0.0,
                gsi: vec![GlobalSecondaryIndex {
                    index_name: "byTag".into(),
                    key_schema: vec![KeySchemaElement {
                        attribute_name: "tag".into(),
                        key_type: "HASH".into(),
                    }],
                    projection: Projection {
                        projection_type: "ALL".into(),
                        non_key_attributes: vec![],
                    },
                    status: "ACTIVE".into(),
                }],
                lsi: vec![],
                stream_enabled: false,
                stream_arn: None,
                stream_view_type: None,
                stream_records: VecDeque::new(),
                stream_sequence: 0,
                ttl: Default::default(),
                tags: Default::default(),
                deletion_protection_enabled: false,
                sse: Default::default(),
                read_capacity_units: 0,
                write_capacity_units: 0,
            },
        );

        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": { "pk": { "S": "p1" }, "tag": { "S": "shared" } },
            }),
            &c,
        )
        .unwrap();

        update_table(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "GlobalSecondaryIndexUpdates": [{
                    "Delete": { "IndexName": "byTag" }
                }]
            }),
            &c,
        )
        .unwrap();

        let err = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "IndexName": "byTag",
                "KeyConditionExpression": "tag = :t",
                "ExpressionAttributeValues": { ":t": { "S": "shared" } },
            }),
            &c,
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn create_table_rejects_gsi_keying_on_undeclared_attribute() {
        use crate::sqlite_store::SqliteStore;
        let state = DynamoState::default();
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = create_table(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
                "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
                "BillingMode": "PAY_PER_REQUEST",
                "GlobalSecondaryIndexes": [{
                    "IndexName": "byTag",
                    "KeySchema": [{ "AttributeName": "tag", "KeyType": "HASH" }],
                    "Projection": { "ProjectionType": "ALL" }
                }]
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("byTag.tag"));
    }

    #[test]
    fn create_table_accepts_gsi_with_declared_attribute() {
        use crate::sqlite_store::SqliteStore;
        let state = DynamoState::default();
        let sqlite = SqliteStore::in_memory().unwrap();
        create_table(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
                "AttributeDefinitions": [
                    { "AttributeName": "pk",  "AttributeType": "S" },
                    { "AttributeName": "tag", "AttributeType": "S" }
                ],
                "BillingMode": "PAY_PER_REQUEST",
                "GlobalSecondaryIndexes": [{
                    "IndexName": "byTag",
                    "KeySchema": [{ "AttributeName": "tag", "KeyType": "HASH" }],
                    "Projection": { "ProjectionType": "ALL" }
                }]
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn update_table_returns_active_status_on_added_gsi() {
        // The synchronous in-line backfill flips the index from
        // CREATING (its initial state) to ACTIVE before update_table
        // returns, so apps that DescribeTable straight after see a
        // queryable index without a polling loop.
        use crate::sqlite_store::SqliteStore;
        let state = state_with_table("t");
        let sqlite = SqliteStore::in_memory().unwrap();
        let resp = update_table(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "AttributeDefinitions": [
                    { "AttributeName": "tag", "AttributeType": "S" }
                ],
                "GlobalSecondaryIndexUpdates": [{
                    "Create": {
                        "IndexName": "byTag",
                        "KeySchema": [{ "AttributeName": "tag", "KeyType": "HASH" }],
                        "Projection": { "ProjectionType": "ALL" }
                    }
                }]
            }),
            &ctx(),
        )
        .unwrap();
        let gsis = resp["TableDescription"]["GlobalSecondaryIndexes"]
            .as_array()
            .expect("GSI list present");
        assert_eq!(gsis.len(), 1);
        assert_eq!(gsis[0]["IndexStatus"].as_str(), Some("ACTIVE"));
    }

    #[test]
    fn update_table_rejects_added_gsi_with_undeclared_attribute() {
        use crate::sqlite_store::SqliteStore;
        let state = state_with_table("t");
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = update_table(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "GlobalSecondaryIndexUpdates": [{
                    "Create": {
                        "IndexName": "missing",
                        "KeySchema": [{ "AttributeName": "ghost", "KeyType": "HASH" }],
                        "Projection": { "ProjectionType": "ALL" }
                    }
                }]
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("missing.ghost"));
    }
}
