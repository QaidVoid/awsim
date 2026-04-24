use awsim_core::{AwsError, InternalEvent, RequestContext};
use serde_json::{Value, json};
use tracing::debug;
use uuid::Uuid;

use crate::{
    expressions::{evaluate_condition, parse_condition},
    state::{DynamoItem, DynamoState, StreamRecord, StreamRecordData},
};

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
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?.to_string();

    let item = parse_item(&input["Item"])
        .ok_or_else(|| AwsError::validation("Item is required and must be a map"))?;

    let (composite_key, old_item, keys_item, hash_key_name) = {
        let mut table = state.tables.get_mut(&table_name).ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Cannot do operations on a non-existent table: {table_name}"),
            )
        })?;

        // Validate key attributes exist
        let hash_key = table.hash_key().map(|s| s.to_string());
        if let Some(ref hk) = hash_key {
            if !item.contains_key(hk) {
                return Err(AwsError::validation(format!(
                    "One or more parameter values were invalid: Missing the key {hk} in the item"
                )));
            }
        }

        let expr_attr_names = get_expr_attr_names(input);
        let expr_attr_values = get_expr_attr_values(input);

        // Evaluate condition expression if present
        if let Some(cond_expr) = opt_str(input, "ConditionExpression") {
            let condition = parse_condition(cond_expr)?;
            let composite_key = table.composite_key(&item);

            let existing = composite_key.as_deref().and_then(|ck| table.items.get(ck));

            let empty_item: DynamoItem = DynamoItem::new();
            let check_item = existing.unwrap_or(&empty_item);

            if !evaluate_condition(&condition, check_item, &expr_attr_names, &expr_attr_values)? {
                return Err(AwsError::conflict(
                    "ConditionalCheckFailedException",
                    "The conditional request failed",
                ));
            }
        }

        let composite_key = table
            .composite_key(&item)
            .ok_or_else(|| AwsError::validation("Could not construct item key"))?;

        // Build keys sub-item for the stream record.
        let mut keys_item = DynamoItem::new();
        for k in table.key_schema.iter().map(|k| k.attribute_name.as_str()) {
            if let Some(v) = item.get(k) {
                keys_item.insert(k.to_string(), v.clone());
            }
        }

        let old_item = table.items.remove(&composite_key);
        table.items.insert(composite_key.clone(), item.clone());

        (composite_key, old_item, keys_item, hash_key)
    };

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
        Some(item.clone()),
        old_item.clone(),
        ctx,
    );

    let _ = (composite_key, hash_key_name); // suppress unused warnings

    let mut result = json!({});
    if return_values == "ALL_OLD" {
        if let Some(old) = old_item {
            result["Attributes"] = item_to_json(&old);
        }
    }
    Ok(result)
}

pub fn get_item(
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

    let key = parse_item(&input["Key"]).ok_or_else(|| AwsError::validation("Key is required"))?;

    let composite_key = table
        .composite_key(&key)
        .ok_or_else(|| AwsError::validation("Could not construct item key"))?;

    let expr_attr_names = get_expr_attr_names(input);
    let projection_expr = opt_str(input, "ProjectionExpression");

    match table.items.get(&composite_key) {
        None => Ok(json!({})),
        Some(item) => {
            let projected = apply_projection(item, projection_expr, &expr_attr_names);
            Ok(json!({ "Item": item_to_json(&projected) }))
        }
    }
}

pub fn delete_item(
    state: &DynamoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?.to_string();

    let key = parse_item(&input["Key"]).ok_or_else(|| AwsError::validation("Key is required"))?;

    let (old_item, keys_item) = {
        let mut table = state.tables.get_mut(&table_name).ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Cannot do operations on a non-existent table: {table_name}"),
            )
        })?;

        let composite_key = table
            .composite_key(&key)
            .ok_or_else(|| AwsError::validation("Could not construct item key"))?;

        let expr_attr_names = get_expr_attr_names(input);
        let expr_attr_values = get_expr_attr_values(input);

        // Evaluate condition expression if present
        if let Some(cond_expr) = opt_str(input, "ConditionExpression") {
            let condition = parse_condition(cond_expr)?;
            let empty_item: DynamoItem = DynamoItem::new();
            let existing = table.items.get(&composite_key).unwrap_or(&empty_item);
            if !evaluate_condition(&condition, existing, &expr_attr_names, &expr_attr_values)? {
                return Err(AwsError::conflict(
                    "ConditionalCheckFailedException",
                    "The conditional request failed",
                ));
            }
        }

        // Collect key attributes for the stream record.
        let mut keys_item = DynamoItem::new();
        for k in table.key_schema.iter().map(|k| k.attribute_name.as_str()) {
            if let Some(v) = key.get(k) {
                keys_item.insert(k.to_string(), v.clone());
            }
        }

        let old_item = table.items.remove(&composite_key);
        (old_item, keys_item)
    };

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
    if return_values == "ALL_OLD" {
        if let Some(old) = old_item {
            result["Attributes"] = item_to_json(&old);
        }
    }
    Ok(result)
}

pub fn update_item(
    state: &DynamoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?.to_string();

    let key = parse_item(&input["Key"]).ok_or_else(|| AwsError::validation("Key is required"))?;

    let (old_item, new_item, keys_item) = {
        let mut table = state.tables.get_mut(&table_name).ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Cannot do operations on a non-existent table: {table_name}"),
            )
        })?;

        let composite_key = table
            .composite_key(&key)
            .ok_or_else(|| AwsError::validation("Could not construct item key"))?;

        let expr_attr_names = get_expr_attr_names(input);
        let expr_attr_values = get_expr_attr_values(input);

        // Evaluate condition expression if present
        if let Some(cond_expr) = opt_str(input, "ConditionExpression") {
            let condition = parse_condition(cond_expr)?;
            let empty_item: DynamoItem = DynamoItem::new();
            let existing = table.items.get(&composite_key).unwrap_or(&empty_item);
            if !evaluate_condition(&condition, existing, &expr_attr_names, &expr_attr_values)? {
                return Err(AwsError::conflict(
                    "ConditionalCheckFailedException",
                    "The conditional request failed",
                ));
            }
        }

        // Get or create the item (upsert semantics)
        let old_item = table.items.get(&composite_key).cloned();
        let mut item = old_item.clone().unwrap_or_else(|| key.clone());

        // Apply UpdateExpression
        if let Some(update_expr) = opt_str(input, "UpdateExpression") {
            crate::expressions::apply_update_expression(
                &mut item,
                update_expr,
                &expr_attr_names,
                &expr_attr_values,
            )?;
        }

        // Ensure key attributes are preserved
        for (k, v) in &key {
            item.insert(k.clone(), v.clone());
        }

        // Collect key attributes for the stream record.
        let mut keys_item = DynamoItem::new();
        for k in table.key_schema.iter().map(|k| k.attribute_name.as_str()) {
            if let Some(v) = key.get(k) {
                keys_item.insert(k.to_string(), v.clone());
            }
        }

        let new_item = item.clone();
        table.items.insert(composite_key, item);

        (old_item, new_item, keys_item)
    };

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
