use awsim_core::{AwsError, InternalEvent, RequestContext};
use serde_json::{Value, json};
use tracing::debug;
use uuid::Uuid;

use crate::{
    expressions::{evaluate_condition, parse_condition},
    keys::{extract_item_keys, extract_pk_sk, item_to_storage_value, storage_value_to_item},
    sqlite_store::SqliteStore,
    state::{DynamoItem, DynamoState, StreamRecord, StreamRecordData},
};

/// Look up an item by `(pk, sk)` in SQLite and decode it back to a
/// `DynamoItem`. Used by the conditional-check + update-expression paths
/// now that items live only in the SQLite mirror.
fn fetch_existing(
    sqlite: &SqliteStore,
    ctx: &RequestContext,
    table_name: &str,
    pk: &str,
    sk: &str,
) -> Result<Option<DynamoItem>, AwsError> {
    let raw = sqlite.get_item(&ctx.account_id, &ctx.region, table_name, pk, sk)?;
    raw.map(|stored| {
        storage_value_to_item(stored)
            .ok_or_else(|| AwsError::internal("DynamoDB stored attrs is not an object"))
    })
    .transpose()
}

use super::{get_expr_attr_names, get_expr_attr_values, opt_str, require_str};

/// Push a stream record into the table's bounded ring-buffer and optionally
/// publish an `InternalEvent` to the event bus so Lambda triggers fire.
fn emit_stream_record(
    state: &DynamoState,
    table_name: &str,
    event_name: &str,
    keys: DynamoItem,
    new_image: Option<DynamoItem>,
    old_image: Option<DynamoItem>,
    ctx: &RequestContext,
) {
    let mut table = match state.tables.get_mut(table_name) {
        Some(t) => t,
        None => return,
    };

    if !table.stream_enabled {
        return;
    }

    let stream_arn = match table.stream_arn.clone() {
        Some(a) => a,
        None => return,
    };

    table.stream_sequence += 1;
    let seq = table.stream_sequence;
    let sequence_number = format!("{:022}", seq);

    let size_bytes: u64 = {
        let mut sz = 0u64;
        for (k, v) in &keys {
            sz += k.len() as u64 + v.to_string().len() as u64;
        }
        if let Some(ref img) = new_image {
            for (k, v) in img {
                sz += k.len() as u64 + v.to_string().len() as u64;
            }
        }
        if let Some(ref img) = old_image {
            for (k, v) in img {
                sz += k.len() as u64 + v.to_string().len() as u64;
            }
        }
        sz
    };

    let view_type = table
        .stream_view_type
        .clone()
        .unwrap_or_else(|| "NEW_AND_OLD_IMAGES".to_string());

    let record = StreamRecord {
        event_id: Uuid::new_v4().to_string(),
        event_name: event_name.to_string(),
        dynamodb: StreamRecordData {
            keys,
            new_image,
            old_image,
            sequence_number: sequence_number.clone(),
            size_bytes,
            stream_view_type: view_type,
        },
        event_source_arn: stream_arn.clone(),
    };

    // Keep last 1 000 records.
    if table.stream_records.len() >= 1000 {
        table.stream_records.remove(0);
    }
    table.stream_records.push(record.clone());

    // Publish to the event bus for Lambda trigger fan-out.
    if let Some(ref bus) = ctx.event_bus {
        let record_json = serde_json::to_value(&record).unwrap_or(json!({}));
        bus.publish(InternalEvent {
            source: "dynamodb".to_string(),
            event_type: "dynamodb:StreamRecord".to_string(),
            region: ctx.region.clone(),
            account_id: ctx.account_id.clone(),
            detail: json!({
                "streamArn": stream_arn,
                "records": [record_json],
            }),
        });
    }
}

/// Parse a DynamoDB item map from JSON value.
pub fn parse_item(val: &Value) -> Option<DynamoItem> {
    val.as_object()
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}

/// Apply projection to an item (return only specified attributes).
fn apply_projection(
    item: &DynamoItem,
    projection_expr: Option<&str>,
    expr_attr_names: &std::collections::HashMap<String, String>,
) -> DynamoItem {
    match projection_expr {
        None => item.clone(),
        Some(expr) => {
            let paths = crate::expressions::parse_projection(expr);
            let mut result = DynamoItem::new();
            for path in paths {
                let resolved = crate::expressions::parser::resolve_path(&path, expr_attr_names);
                if let Some(val) = item.get(&resolved) {
                    result.insert(resolved, val.clone());
                }
            }
            result
        }
    }
}

/// Convert a DynamoItem to a JSON object value.
pub fn item_to_json(item: &DynamoItem) -> Value {
    let mut map = serde_json::Map::new();
    for (k, v) in item {
        map.insert(k.clone(), v.clone());
    }
    Value::Object(map)
}

pub fn put_item(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?.to_string();

    let item = parse_item(&input["Item"])
        .ok_or_else(|| AwsError::validation("Item is required and must be a map"))?;

    // Extracted SQLite keys (pk/sk + per-GSI key columns) computed inside
    // the lock so we get them while we hold the canonical schema view.
    // Pull schema-derived bits up front, then drop the dashmap guard so
    // we never hold it across SQLite IO.
    let (sqlite_keys, keys_item) = {
        let table = state.tables.get(&table_name).ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Cannot do operations on a non-existent table: {table_name}"),
            )
        })?;

        // Validate the hash key attribute is present in the inbound item.
        if let Some(hk) = table.hash_key()
            && !item.contains_key(hk)
        {
            return Err(AwsError::validation(format!(
                "One or more parameter values were invalid: Missing the key {hk} in the item"
            )));
        }

        let sqlite_keys = extract_item_keys(&table, &item)
            .ok_or_else(|| AwsError::validation("Could not extract SQLite keys"))?;

        let mut keys_item = DynamoItem::new();
        for k in table.key_schema.iter().map(|k| k.attribute_name.as_str()) {
            if let Some(v) = item.get(k) {
                keys_item.insert(k.to_string(), v.clone());
            }
        }
        (sqlite_keys, keys_item)
    };

    let expr_attr_names = get_expr_attr_names(input);
    let expr_attr_values = get_expr_attr_values(input);

    // Conditional check + old-image lookup against the SQLite mirror.
    let old_item = fetch_existing(
        sqlite,
        ctx,
        &table_name,
        &sqlite_keys.pk,
        &sqlite_keys.sk,
    )?;

    if let Some(cond_expr) = opt_str(input, "ConditionExpression") {
        let condition = parse_condition(cond_expr)?;
        let empty_item: DynamoItem = DynamoItem::new();
        let check_item = old_item.as_ref().unwrap_or(&empty_item);
        if !evaluate_condition(&condition, check_item, &expr_attr_names, &expr_attr_values)? {
            return Err(AwsError::conflict(
                "ConditionalCheckFailedException",
                "The conditional request failed",
            ));
        }
    }

    let attrs_value = item_to_storage_value(&item);
    sqlite.put_item(
        &ctx.account_id,
        &ctx.region,
        &table_name,
        &sqlite_keys.pk,
        &sqlite_keys.sk,
        &attrs_value,
        &sqlite_keys.gsi,
    )?;

    let return_values = opt_str(input, "ReturnValues").unwrap_or("NONE");

    let event_name = if old_item.is_some() {
        "MODIFY"
    } else {
        "INSERT"
    };
    emit_stream_record(
        state,
        &table_name,
        event_name,
        keys_item,
        Some(item.clone()),
        old_item.clone(),
        ctx,
    );

    let mut result = json!({});
    if return_values == "ALL_OLD"
        && let Some(old) = old_item
    {
        result["Attributes"] = item_to_json(&old);
    }
    Ok(result)
}

pub fn get_item(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    let table = state.tables.get(table_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    let key = parse_item(&input["Key"]).ok_or_else(|| AwsError::validation("Key is required"))?;

    let (pk, sk) = extract_pk_sk(&table, &key)
        .ok_or_else(|| AwsError::validation("Could not construct item key"))?;

    let expr_attr_names = get_expr_attr_names(input);
    let projection_expr = opt_str(input, "ProjectionExpression");

    // Drop the dashmap guard before SQLite IO to avoid pinning the shard.
    drop(table);

    let raw = sqlite.get_item(&ctx.account_id, &ctx.region, table_name, &pk, &sk)?;
    match raw {
        None => Ok(json!({})),
        Some(stored) => {
            let item = crate::keys::storage_value_to_item(stored)
                .ok_or_else(|| AwsError::internal("DynamoDB stored attrs is not an object"))?;
            let projected = apply_projection(&item, projection_expr, &expr_attr_names);
            Ok(json!({ "Item": item_to_json(&projected) }))
        }
    }
}

pub fn delete_item(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?.to_string();

    let key = parse_item(&input["Key"]).ok_or_else(|| AwsError::validation("Key is required"))?;

    let (sqlite_pk_sk, keys_item) = {
        let table = state.tables.get(&table_name).ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Cannot do operations on a non-existent table: {table_name}"),
            )
        })?;

        let sqlite_pk_sk = extract_pk_sk(&table, &key)
            .ok_or_else(|| AwsError::validation("Could not extract SQLite keys"))?;

        let mut keys_item = DynamoItem::new();
        for k in table.key_schema.iter().map(|k| k.attribute_name.as_str()) {
            if let Some(v) = key.get(k) {
                keys_item.insert(k.to_string(), v.clone());
            }
        }
        (sqlite_pk_sk, keys_item)
    };

    let expr_attr_names = get_expr_attr_names(input);
    let expr_attr_values = get_expr_attr_values(input);

    // Snapshot the existing item before delete — needed for both
    // ConditionExpression evaluation and the REMOVE stream record.
    let old_item = fetch_existing(
        sqlite,
        ctx,
        &table_name,
        &sqlite_pk_sk.0,
        &sqlite_pk_sk.1,
    )?;

    if let Some(cond_expr) = opt_str(input, "ConditionExpression") {
        let condition = parse_condition(cond_expr)?;
        let empty_item: DynamoItem = DynamoItem::new();
        let existing = old_item.as_ref().unwrap_or(&empty_item);
        if !evaluate_condition(&condition, existing, &expr_attr_names, &expr_attr_values)? {
            return Err(AwsError::conflict(
                "ConditionalCheckFailedException",
                "The conditional request failed",
            ));
        }
    }

    let _ = sqlite.delete_item(
        &ctx.account_id,
        &ctx.region,
        &table_name,
        &sqlite_pk_sk.0,
        &sqlite_pk_sk.1,
    )?;

    let return_values = opt_str(input, "ReturnValues").unwrap_or("NONE");

    // Emit stream record only when an item was actually removed.
    if old_item.is_some() {
        emit_stream_record(
            state,
            &table_name,
            "REMOVE",
            keys_item,
            None,
            old_item.clone(),
            ctx,
        );
    }

    let mut result = json!({});
    if return_values == "ALL_OLD"
        && let Some(old) = old_item
    {
        result["Attributes"] = item_to_json(&old);
    }
    Ok(result)
}

pub fn update_item(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?.to_string();

    let key = parse_item(&input["Key"]).ok_or_else(|| AwsError::validation("Key is required"))?;

    let (sqlite_pk_sk, keys_item) = {
        let table = state.tables.get(&table_name).ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Cannot do operations on a non-existent table: {table_name}"),
            )
        })?;

        let sqlite_pk_sk = extract_pk_sk(&table, &key)
            .ok_or_else(|| AwsError::validation("Could not extract SQLite keys"))?;

        let mut keys_item = DynamoItem::new();
        for k in table.key_schema.iter().map(|k| k.attribute_name.as_str()) {
            if let Some(v) = key.get(k) {
                keys_item.insert(k.to_string(), v.clone());
            }
        }
        (sqlite_pk_sk, keys_item)
    };

    let expr_attr_names = get_expr_attr_names(input);
    let expr_attr_values = get_expr_attr_values(input);

    // Load the existing item (upsert semantics — Update creates the row
    // when it doesn't yet exist, with just the key attributes populated).
    let old_item = fetch_existing(
        sqlite,
        ctx,
        &table_name,
        &sqlite_pk_sk.0,
        &sqlite_pk_sk.1,
    )?;

    if let Some(cond_expr) = opt_str(input, "ConditionExpression") {
        let condition = parse_condition(cond_expr)?;
        let empty_item: DynamoItem = DynamoItem::new();
        let existing = old_item.as_ref().unwrap_or(&empty_item);
        if !evaluate_condition(&condition, existing, &expr_attr_names, &expr_attr_values)? {
            return Err(AwsError::conflict(
                "ConditionalCheckFailedException",
                "The conditional request failed",
            ));
        }
    }

    let mut new_item = old_item.clone().unwrap_or_else(|| key.clone());

    if let Some(update_expr) = opt_str(input, "UpdateExpression") {
        crate::expressions::apply_update_expression(
            &mut new_item,
            update_expr,
            &expr_attr_names,
            &expr_attr_values,
        )?;
    }

    // Key attributes always survive an UpdateExpression (DynamoDB semantics).
    for (k, v) in &key {
        new_item.insert(k.clone(), v.clone());
    }

    // Re-extract SQLite keys from the merged item — UpdateExpression may
    // have introduced or changed GSI key attributes.
    let sqlite_keys = {
        let table = state.tables.get(&table_name).ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Cannot do operations on a non-existent table: {table_name}"),
            )
        })?;
        extract_item_keys(&table, &new_item)
            .ok_or_else(|| AwsError::validation("Could not extract SQLite keys"))?
    };

    let attrs_value = item_to_storage_value(&new_item);
    sqlite.put_item(
        &ctx.account_id,
        &ctx.region,
        &table_name,
        &sqlite_keys.pk,
        &sqlite_keys.sk,
        &attrs_value,
        &sqlite_keys.gsi,
    )?;

    debug!(table = %table_name, "Updated item");

    let return_values = opt_str(input, "ReturnValues").unwrap_or("NONE");

    // Emit stream record after lock is released.
    let event_name = if old_item.is_some() {
        "MODIFY"
    } else {
        "INSERT"
    };
    emit_stream_record(
        state,
        &table_name,
        event_name,
        keys_item,
        Some(new_item.clone()),
        old_item.clone(),
        ctx,
    );

    let mut result = json!({});
    match return_values {
        "ALL_OLD" => {
            if let Some(old) = old_item {
                result["Attributes"] = item_to_json(&old);
            }
        }
        "ALL_NEW" => {
            result["Attributes"] = item_to_json(&new_item);
        }
        "UPDATED_OLD" => {
            // In full fidelity we'd track which attrs changed; for now return old
            if let Some(old) = old_item {
                result["Attributes"] = item_to_json(&old);
            }
        }
        "UPDATED_NEW" => {
            result["Attributes"] = item_to_json(&new_item);
        }
        _ => {}
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{KeySchemaElement, Table};
    use serde_json::json;

    fn ctx() -> RequestContext {
        RequestContext::new("dynamodb", "us-east-1")
    }

    fn make_state_with_table() -> DynamoState {
        let state = DynamoState::default();
        let table = Table {
            name: "t".into(),
            arn: "arn:aws:dynamodb:us-east-1:000000000000:table/t".into(),
            key_schema: vec![
                KeySchemaElement {
                    attribute_name: "pk".into(),
                    key_type: "HASH".into(),
                },
                KeySchemaElement {
                    attribute_name: "sk".into(),
                    key_type: "RANGE".into(),
                },
            ],
            attribute_definitions: vec![],
            billing_mode: "PAY_PER_REQUEST".into(),
            status: "ACTIVE".into(),
            created_at: 0.0,
            gsi: vec![],
            lsi: vec![],
            items: std::collections::BTreeMap::new(),
            stream_enabled: false,
            stream_arn: None,
            stream_view_type: None,
            stream_records: Vec::new(),
            stream_sequence: 0,
            ttl: Default::default(),
            tags: Default::default(),
        };
        state.tables.insert("t".into(), table);
        state
    }

    #[test]
    fn put_item_does_not_grow_in_memory_store() {
        // Bulk insert proves the memory-pressure regression that motivated
        // this whole refactor: 1k rows in, in-memory items.len() stays 0.
        let state = make_state_with_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let ctx = ctx();

        for i in 0..1000 {
            let input = json!({
                "TableName": "t",
                "Item": {
                    "pk": {"S": "tenant"},
                    "sk": {"S": format!("row-{i:04}")},
                    "n": {"N": i.to_string()},
                }
            });
            put_item(&state, &sqlite, &input, &ctx).unwrap();
        }

        // The whole point of stage 4: items live exclusively in SQLite.
        assert_eq!(state.tables.get("t").unwrap().items.len(), 0);
        assert_eq!(sqlite.count_items(&ctx.account_id, &ctx.region, "t").unwrap(), 1000);
    }

    #[test]
    fn put_item_writes_only_to_sqlite() {
        let state = make_state_with_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let ctx = ctx();

        let input = json!({
            "TableName": "t",
            "Item": {
                "pk": {"S": "user-1"},
                "sk": {"S": "profile"},
                "name": {"S": "Alice"},
            }
        });

        put_item(&state, &sqlite, &input, &ctx).unwrap();

        // After stage 4 items live only in SQLite — the in-memory map
        // stays empty.
        assert_eq!(state.tables.get("t").unwrap().items.len(), 0);

        let stored = sqlite
            .get_item(&ctx.account_id, &ctx.region, "t", "user-1", "profile")
            .unwrap()
            .expect("sqlite store");
        assert_eq!(stored["name"], json!({"S": "Alice"}));
    }

    #[test]
    fn delete_item_writes_only_to_sqlite() {
        let state = make_state_with_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let ctx = ctx();

        let put_input = json!({
            "TableName": "t",
            "Item": {"pk": {"S": "x"}, "sk": {"S": "y"}, "v": {"N": "1"}}
        });
        put_item(&state, &sqlite, &put_input, &ctx).unwrap();

        let del_input = json!({
            "TableName": "t",
            "Key": {"pk": {"S": "x"}, "sk": {"S": "y"}}
        });
        delete_item(&state, &sqlite, &del_input, &ctx).unwrap();

        // Items live only in SQLite — verify the row is gone there.
        assert_eq!(
            sqlite
                .get_item(&ctx.account_id, &ctx.region, "t", "x", "y")
                .unwrap(),
            None
        );
    }

    #[test]
    fn get_item_reads_from_sqlite_after_put() {
        let state = make_state_with_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let ctx = ctx();

        // Seed via put_item (dual-writes), then read via get_item
        // (sqlite-only) — proves the read path picks up the mirror.
        let put = json!({
            "TableName": "t",
            "Item": {"pk": {"S": "u"}, "sk": {"S": "p"}, "n": {"S": "Bob"}}
        });
        put_item(&state, &sqlite, &put, &ctx).unwrap();

        let get = json!({
            "TableName": "t",
            "Key": {"pk": {"S": "u"}, "sk": {"S": "p"}}
        });
        let res = get_item(&state, &sqlite, &get, &ctx).unwrap();
        assert_eq!(res["Item"]["n"], json!({"S": "Bob"}));
    }

    #[test]
    fn update_item_writes_only_to_sqlite() {
        let state = make_state_with_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let ctx = ctx();

        let input = json!({
            "TableName": "t",
            "Key": {"pk": {"S": "p"}, "sk": {"S": "s"}},
            "UpdateExpression": "SET #v = :v",
            "ExpressionAttributeNames": {"#v": "value"},
            "ExpressionAttributeValues": {":v": {"S": "hello"}}
        });

        update_item(&state, &sqlite, &input, &ctx).unwrap();

        let stored = sqlite
            .get_item(&ctx.account_id, &ctx.region, "t", "p", "s")
            .unwrap()
            .expect("sqlite mirror");
        assert_eq!(stored["value"], json!({"S": "hello"}));
    }
}
