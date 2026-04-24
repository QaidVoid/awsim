use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    expressions::{evaluate_condition, parse_condition, parse_projection, parser::resolve_path},
    state::{DynamoItem, DynamoState, extract_scalar_str},
};

use super::{get_expr_attr_names, get_expr_attr_values, opt_str, require_str};
use crate::operations::item::item_to_json;

fn apply_projection_to_item(
    item: &DynamoItem,
    paths: &[String],
    expr_attr_names: &std::collections::HashMap<String, String>,
) -> DynamoItem {
    if paths.is_empty() {
        return item.clone();
    }
    let mut result = DynamoItem::new();
    for path in paths {
        let resolved = resolve_path(path, expr_attr_names);
        if let Some(val) = item.get(&resolved) {
            result.insert(resolved, val.clone());
        }
    }
    result
}

pub fn query(state: &DynamoState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    let table = state.tables.get(table_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    let expr_attr_names = get_expr_attr_names(input);
    let expr_attr_values = get_expr_attr_values(input);
    let projection_expr = opt_str(input, "ProjectionExpression");
    let filter_expr = opt_str(input, "FilterExpression");
    let key_condition_expr = opt_str(input, "KeyConditionExpression")
        .ok_or_else(|| AwsError::validation("KeyConditionExpression is required for Query"))?;
    let limit = input
        .get("Limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    let scan_index_forward = input
        .get("ScanIndexForward")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let select = opt_str(input, "Select").unwrap_or("ALL_ATTRIBUTES");
    let exclusive_start_key = input
        .get("ExclusiveStartKey")
        .and_then(|v| v.as_object())
        .cloned();

    let key_condition = parse_condition(key_condition_expr)?;
    let filter_condition = filter_expr.map(parse_condition).transpose()?;

    let projection_paths: Vec<String> = projection_expr
        .map(|e| parse_projection(e))
        .unwrap_or_default();

    // Extract partition key value from key condition for efficient lookup
    let hash_key_name = table.hash_key().unwrap_or("").to_string();
    let range_key_name = table.range_key().map(|s| s.to_string());

    // Determine scan range: if we know the partition key prefix, use BTreeMap range
    let pk_prefix = extract_pk_from_condition(
        key_condition_expr,
        &hash_key_name,
        &expr_attr_names,
        &expr_attr_values,
    );

    // Collect all items in range
    let all_items: Vec<(String, DynamoItem)> = if let Some(ref pk) = pk_prefix {
        // Efficient: scan only items with this partition key prefix
        let prefix = format!("{pk}\0");
        let exact = pk.clone();

        if range_key_name.is_some() {
            table
                .items
                .range(prefix.clone()..)
                .take_while(|(k, _)| k.starts_with(&prefix))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        } else {
            // No range key: only one item per partition key
            table
                .items
                .get(&exact)
                .map(|v| vec![(exact.clone(), v.clone())])
                .unwrap_or_default()
        }
    } else {
        table
            .items
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };

    // Apply pagination (ExclusiveStartKey)
    let start_after = exclusive_start_key.as_ref().and_then(|esk| {
        let hk_val = esk
            .get(&hash_key_name)
            .and_then(|v| extract_scalar_str(v))
            .map(|s| s.to_string())?;
        let sk_val = range_key_name
            .as_deref()
            .and_then(|rk| esk.get(rk))
            .and_then(|v| extract_scalar_str(v))
            .map(|s| s.to_string());
        Some(if let Some(sv) = sk_val {
            format!("{hk_val}\0{sv}")
        } else {
            hk_val
        })
    });

    let mut scanned_count = 0usize;
    let mut items: Vec<DynamoItem> = Vec::new();
    let mut last_evaluated_key: Option<DynamoItem> = None;
    let mut past_start = start_after.is_none();

    // Sort order
    let ordered: Vec<(String, DynamoItem)> = if scan_index_forward {
        all_items
    } else {
        let mut v = all_items;
        v.reverse();
        v
    };

    for (composite_key, item) in ordered {
        // Skip until past the exclusive start key
        if !past_start {
            if let Some(ref start) = start_after {
                if &composite_key == start {
                    past_start = true;
                }
            }
            continue;
        }

        // Evaluate key condition
        if !evaluate_condition(&key_condition, &item, &expr_attr_names, &expr_attr_values)? {
            scanned_count += 1;
            continue;
        }

        scanned_count += 1;

        // Apply filter expression
        if let Some(ref filter) = filter_condition {
            if !evaluate_condition(filter, &item, &expr_attr_names, &expr_attr_values)? {
                continue;
            }
        }

        // Apply projection
        let projected = if select == "COUNT" {
            DynamoItem::new()
        } else {
            apply_projection_to_item(&item, &projection_paths, &expr_attr_names)
        };

        items.push(projected);

        // Check limit
        if let Some(lim) = limit {
            if items.len() >= lim {
                // Build LastEvaluatedKey
                let mut lek = DynamoItem::new();
                if let Some(hk_val) = item.get(&hash_key_name) {
                    lek.insert(hash_key_name.clone(), hk_val.clone());
                }
                if let Some(ref rk) = range_key_name {
                    if let Some(sk_val) = item.get(rk) {
                        lek.insert(rk.clone(), sk_val.clone());
                    }
                }
                last_evaluated_key = Some(lek);
                break;
            }
        }
    }

    let count = if select == "COUNT" {
        items.len()
    } else {
        items.len()
    };

    let result_items: Vec<Value> = items.into_iter().map(|i| item_to_json(&i)).collect();

    let mut result = json!({
        "Items": result_items,
        "Count": count,
        "ScannedCount": scanned_count
    });

    if let Some(lek) = last_evaluated_key {
        result["LastEvaluatedKey"] = item_to_json(&lek);
    }

    Ok(result)
}

pub fn scan(state: &DynamoState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    let table = state.tables.get(table_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    let expr_attr_names = get_expr_attr_names(input);
    let expr_attr_values = get_expr_attr_values(input);
    let projection_expr = opt_str(input, "ProjectionExpression");
    let filter_expr = opt_str(input, "FilterExpression");
    let limit = input
        .get("Limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    let select = opt_str(input, "Select").unwrap_or("ALL_ATTRIBUTES");
    let exclusive_start_key = input
        .get("ExclusiveStartKey")
        .and_then(|v| v.as_object())
        .cloned();

    let filter_condition = filter_expr.map(parse_condition).transpose()?;

    let projection_paths: Vec<String> = projection_expr
        .map(|e| parse_projection(e))
        .unwrap_or_default();

    let hash_key_name = table.hash_key().unwrap_or("").to_string();
    let range_key_name = table.range_key().map(|s| s.to_string());

    // Pagination start
    let start_after = exclusive_start_key.as_ref().and_then(|esk| {
        let hk_val = esk
            .get(&hash_key_name)
            .and_then(|v| extract_scalar_str(v))
            .map(|s| s.to_string())?;
        let sk_val = range_key_name
            .as_deref()
            .and_then(|rk| esk.get(rk))
            .and_then(|v| extract_scalar_str(v))
            .map(|s| s.to_string());
        Some(if let Some(sv) = sk_val {
            format!("{hk_val}\0{sv}")
        } else {
            hk_val
        })
    });

    let mut scanned_count = 0usize;
    let mut items: Vec<DynamoItem> = Vec::new();
    let mut last_evaluated_key: Option<DynamoItem> = None;
    let mut past_start = start_after.is_none();

    for (composite_key, item) in table.items.iter() {
        if !past_start {
            if let Some(ref start) = start_after {
                if composite_key == start {
                    past_start = true;
                }
            }
            continue;
        }

        scanned_count += 1;

        // Apply filter expression
        if let Some(ref filter) = filter_condition {
            if !evaluate_condition(filter, item, &expr_attr_names, &expr_attr_values)? {
                continue;
            }
        }

        let projected = if select == "COUNT" {
            DynamoItem::new()
        } else {
            apply_projection_to_item(item, &projection_paths, &expr_attr_names)
        };

        items.push(projected);

        if let Some(lim) = limit {
            if items.len() >= lim {
                let mut lek = DynamoItem::new();
                if let Some(hk_val) = item.get(&hash_key_name) {
                    lek.insert(hash_key_name.clone(), hk_val.clone());
                }
                if let Some(ref rk) = range_key_name {
                    if let Some(sk_val) = item.get(rk) {
                        lek.insert(rk.clone(), sk_val.clone());
                    }
                }
                last_evaluated_key = Some(lek);
                break;
            }
        }
    }

    let count = items.len();
    let result_items: Vec<Value> = items.into_iter().map(|i| item_to_json(&i)).collect();

    let mut result = json!({
        "Items": result_items,
        "Count": count,
        "ScannedCount": scanned_count
    });

    if let Some(lek) = last_evaluated_key {
        result["LastEvaluatedKey"] = item_to_json(&lek);
    }

    Ok(result)
}

/// Try to extract the partition key value from a KeyConditionExpression.
/// This enables efficient BTreeMap range queries.
/// Supports: "pk = :val", "pk = :val AND sk <op> :sk_val", etc.
fn extract_pk_from_condition(
    expr: &str,
    hash_key_name: &str,
    expr_attr_names: &std::collections::HashMap<String, String>,
    expr_attr_values: &serde_json::Map<String, Value>,
) -> Option<String> {
    // Simple heuristic: look for "hash_key = :placeholder" pattern
    // We tokenize and look for equality on the hash key
    let upper = expr.to_uppercase();
    let hash_upper = hash_key_name.to_uppercase();

    // Check if the expression directly references the hash key
    if !upper.contains(&hash_upper) && !expr.contains('#') {
        return None;
    }

    // Try to find "key_name = :placeholder" or "#alias = :placeholder"
    // Look for patterns like: pk = :pk
    for part in expr.split("AND") {
        let part = part.trim();
        if let Some(eq_pos) = part.find('=') {
            let left = part[..eq_pos].trim();
            let right = part[eq_pos + 1..].trim();

            // Resolve left side (attribute name or alias)
            let resolved_left = if let Some(stripped) = left.strip_prefix('#') {
                expr_attr_names
                    .get(&format!("#{stripped}"))
                    .map(|s| s.as_str())
                    .unwrap_or(left)
            } else {
                left
            };

            if resolved_left == hash_key_name {
                // Get value from expression attribute values
                if let Some(placeholder) = right.strip_prefix(':') {
                    let key = format!(":{placeholder}");
                    if let Some(val) = expr_attr_values.get(&key) {
                        return val
                            .get("S")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .or_else(|| {
                                val.get("N").and_then(|v| v.as_str()).map(|s| s.to_string())
                            });
                    }
                }
            }
        }
    }
    None
}
