use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::debug;

use crate::{
    expressions::{evaluate_condition, parse_condition},
    state::{DynamoItem, DynamoState},
};

use super::{get_expr_attr_names, get_expr_attr_values, opt_str, require_str};

/// Parse a DynamoDB item map from JSON value.
pub fn parse_item(val: &Value) -> Option<DynamoItem> {
    val.as_object().map(|obj| {
        obj.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    })
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
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    let mut table = state.tables.get_mut(table_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    let item = parse_item(&input["Item"])
        .ok_or_else(|| AwsError::validation("Item is required and must be a map"))?;

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
    let return_values = opt_str(input, "ReturnValues").unwrap_or("NONE");

    // Evaluate condition expression if present
    if let Some(cond_expr) = opt_str(input, "ConditionExpression") {
        let condition = parse_condition(cond_expr)?;
        let composite_key = table.composite_key(&item);

        let existing = composite_key
            .as_deref()
            .and_then(|ck| table.items.get(ck));

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

    let old_item = table.items.remove(&composite_key);
    table.items.insert(composite_key, item);

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

    let key = parse_item(&input["Key"])
        .ok_or_else(|| AwsError::validation("Key is required"))?;

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
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    let mut table = state.tables.get_mut(table_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    let key = parse_item(&input["Key"])
        .ok_or_else(|| AwsError::validation("Key is required"))?;

    let composite_key = table
        .composite_key(&key)
        .ok_or_else(|| AwsError::validation("Could not construct item key"))?;

    let expr_attr_names = get_expr_attr_names(input);
    let expr_attr_values = get_expr_attr_values(input);
    let return_values = opt_str(input, "ReturnValues").unwrap_or("NONE");

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

    let old_item = table.items.remove(&composite_key);

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
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    let mut table = state.tables.get_mut(table_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    let key = parse_item(&input["Key"])
        .ok_or_else(|| AwsError::validation("Key is required"))?;

    let composite_key = table
        .composite_key(&key)
        .ok_or_else(|| AwsError::validation("Could not construct item key"))?;

    let expr_attr_names = get_expr_attr_names(input);
    let expr_attr_values = get_expr_attr_values(input);
    let return_values = opt_str(input, "ReturnValues").unwrap_or("NONE");

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

    let new_item = item.clone();
    table.items.insert(composite_key, item);

    debug!(table = %table_name, "Updated item");

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
