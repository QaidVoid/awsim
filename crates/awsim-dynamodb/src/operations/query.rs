use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    expressions::{evaluate_condition, parse_condition, parse_projection, parser::resolve_path},
    keys::storage_value_to_item,
    sqlite_store::SqliteStore,
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

/// Build the LastEvaluatedKey JSON object from an item by extracting just
/// the table's hash + range key attributes.
fn last_evaluated_key(
    item: &DynamoItem,
    hash_key_name: &str,
    range_key_name: Option<&str>,
) -> DynamoItem {
    let mut lek = DynamoItem::new();
    if let Some(hk_val) = item.get(hash_key_name) {
        lek.insert(hash_key_name.to_string(), hk_val.clone());
    }
    if let Some(rk) = range_key_name
        && let Some(sk_val) = item.get(rk)
    {
        lek.insert(rk.to_string(), sk_val.clone());
    }
    lek
}

pub fn query(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    // Schema still comes from the in-memory cache during stage 3 — table
    // metadata moves to SQLite in stage 4.
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

    let projection_paths: Vec<String> = projection_expr.map(parse_projection).unwrap_or_default();

    let hash_key_name = table.hash_key().unwrap_or("").to_string();
    let range_key_name = table.range_key().map(|s| s.to_string());

    // Pull the partition key value out of the KeyConditionExpression so we
    // can push the partition lookup down to SQLite. DynamoDB requires the
    // hash key in every Query, but our parser is conservative — if it
    // can't find one we fall back to a full Scan-style sweep.
    let pk_value = extract_pk_from_condition(
        key_condition_expr,
        &hash_key_name,
        &expr_attr_names,
        &expr_attr_values,
    );

    // Convert ExclusiveStartKey → SQL pagination markers.
    let start_after_sk = exclusive_start_key
        .as_ref()
        .and_then(|esk| range_key_name.as_deref().and_then(|rk| esk.get(rk)))
        .and_then(extract_scalar_str)
        .map(|s| s.to_string());

    let mut scanned_count = 0usize;
    let mut items: Vec<DynamoItem> = Vec::new();
    let mut last_item: Option<DynamoItem> = None;
    let mut hit_limit = false;

    // Drop the table guard before SQLite IO — the dashmap Ref pins a
    // shard, and we don't want to hold it across a blocking read.
    drop(table);

    let mut handle = |item: DynamoItem| -> Result<bool, AwsError> {
        // Key condition over typed attributes (covers sort key range,
        // BEGINS_WITH, BETWEEN, etc.). Items in the partition that fail
        // the condition still count toward ScannedCount, matching real
        // DynamoDB.
        if !evaluate_condition(&key_condition, &item, &expr_attr_names, &expr_attr_values)? {
            scanned_count += 1;
            return Ok(true);
        }
        scanned_count += 1;

        if let Some(ref filter) = filter_condition
            && !evaluate_condition(filter, &item, &expr_attr_names, &expr_attr_values)?
        {
            return Ok(true);
        }

        let projected = if select == "COUNT" {
            DynamoItem::new()
        } else {
            apply_projection_to_item(&item, &projection_paths, &expr_attr_names)
        };
        items.push(projected);
        last_item = Some(item);

        if let Some(lim) = limit
            && items.len() >= lim
        {
            hit_limit = true;
            return Ok(false);
        }
        Ok(true)
    };

    if let Some(ref pk) = pk_value {
        sqlite.query_partition(
            &ctx.account_id,
            &ctx.region,
            table_name,
            pk,
            scan_index_forward,
            start_after_sk.as_deref(),
            |_sk, attrs| {
                let item = storage_value_to_item(attrs)
                    .ok_or_else(|| AwsError::internal("DynamoDB stored attrs is not an object"))?;
                handle(item)
            },
        )?;
    } else {
        // No usable hash-key constraint extracted — fall back to a full
        // table scan (matches the legacy in-memory behaviour).
        let scan_start = exclusive_start_key.as_ref().and_then(|esk| {
            let pk = esk.get(&hash_key_name).and_then(extract_scalar_str)?;
            let sk = range_key_name
                .as_deref()
                .and_then(|rk| esk.get(rk))
                .and_then(extract_scalar_str)
                .unwrap_or("");
            Some((pk.to_string(), sk.to_string()))
        });
        let scan_start_ref = scan_start.as_ref().map(|(p, s)| (p.as_str(), s.as_str()));
        sqlite.scan_table(
            &ctx.account_id,
            &ctx.region,
            table_name,
            scan_start_ref,
            |_pk, _sk, attrs| {
                let item = storage_value_to_item(attrs)
                    .ok_or_else(|| AwsError::internal("DynamoDB stored attrs is not an object"))?;
                handle(item)
            },
        )?;
    }

    let count = items.len();
    let result_items: Vec<Value> = items.into_iter().map(|i| item_to_json(&i)).collect();

    let mut result = json!({
        "Items": result_items,
        "Count": count,
        "ScannedCount": scanned_count,
    });

    if hit_limit && let Some(item) = last_item {
        let lek = last_evaluated_key(&item, &hash_key_name, range_key_name.as_deref());
        result["LastEvaluatedKey"] = item_to_json(&lek);
    }

    Ok(result)
}

pub fn scan(
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
    let projection_paths: Vec<String> = projection_expr.map(parse_projection).unwrap_or_default();

    let hash_key_name = table.hash_key().unwrap_or("").to_string();
    let range_key_name = table.range_key().map(|s| s.to_string());

    drop(table);

    // Translate ExclusiveStartKey → (pk, sk) tuple SQLite uses for
    // resume. Tables with no sort key encode sk as the empty string.
    let scan_start = exclusive_start_key.as_ref().and_then(|esk| {
        let pk = esk.get(&hash_key_name).and_then(extract_scalar_str)?;
        let sk = range_key_name
            .as_deref()
            .and_then(|rk| esk.get(rk))
            .and_then(extract_scalar_str)
            .unwrap_or("");
        Some((pk.to_string(), sk.to_string()))
    });

    let mut scanned_count = 0usize;
    let mut items: Vec<DynamoItem> = Vec::new();
    let mut last_item: Option<DynamoItem> = None;
    let mut hit_limit = false;

    let scan_start_ref = scan_start.as_ref().map(|(p, s)| (p.as_str(), s.as_str()));
    sqlite.scan_table(
        &ctx.account_id,
        &ctx.region,
        table_name,
        scan_start_ref,
        |_pk, _sk, attrs| {
            let item = storage_value_to_item(attrs)
                .ok_or_else(|| AwsError::internal("DynamoDB stored attrs is not an object"))?;

            scanned_count += 1;

            if let Some(ref filter) = filter_condition
                && !evaluate_condition(filter, &item, &expr_attr_names, &expr_attr_values)?
            {
                return Ok(true);
            }

            let projected = if select == "COUNT" {
                DynamoItem::new()
            } else {
                apply_projection_to_item(&item, &projection_paths, &expr_attr_names)
            };
            items.push(projected);
            last_item = Some(item);

            if let Some(lim) = limit
                && items.len() >= lim
            {
                hit_limit = true;
                return Ok(false);
            }
            Ok(true)
        },
    )?;

    let count = items.len();
    let result_items: Vec<Value> = items.into_iter().map(|i| item_to_json(&i)).collect();

    let mut result = json!({
        "Items": result_items,
        "Count": count,
        "ScannedCount": scanned_count,
    });

    if hit_limit && let Some(item) = last_item {
        let lek = last_evaluated_key(&item, &hash_key_name, range_key_name.as_deref());
        result["LastEvaluatedKey"] = item_to_json(&lek);
    }

    Ok(result)
}

/// Try to extract the partition key value from a KeyConditionExpression.
/// This enables a single-partition lookup against SQLite instead of a
/// full table scan.
/// Supports: "pk = :val", "pk = :val AND sk <op> :sk_val", etc.
fn extract_pk_from_condition(
    expr: &str,
    hash_key_name: &str,
    expr_attr_names: &std::collections::HashMap<String, String>,
    expr_attr_values: &serde_json::Map<String, Value>,
) -> Option<String> {
    // Simple heuristic: look for "hash_key = :placeholder" pattern.
    let upper = expr.to_uppercase();
    let hash_upper = hash_key_name.to_uppercase();

    if !upper.contains(&hash_upper) && !expr.contains('#') {
        return None;
    }

    for part in expr.split("AND") {
        let part = part.trim();
        if let Some(eq_pos) = part.find('=') {
            let left = part[..eq_pos].trim();
            let right = part[eq_pos + 1..].trim();

            let resolved_left = if let Some(stripped) = left.strip_prefix('#') {
                expr_attr_names
                    .get(&format!("#{stripped}"))
                    .map(|s| s.as_str())
                    .unwrap_or(left)
            } else {
                left
            };

            if resolved_left == hash_key_name
                && let Some(placeholder) = right.strip_prefix(':')
            {
                let key = format!(":{placeholder}");
                if let Some(val) = expr_attr_values.get(&key) {
                    return val
                        .get("S")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| val.get("N").and_then(|v| v.as_str()).map(|s| s.to_string()));
                }
            }
        }
    }
    None
}
