use awsim_core::{AwsError, InternalEvent, RequestContext};
use serde_json::{Value, json};
use tracing::debug;
use uuid::Uuid;

use crate::{
    expressions::{evaluate_condition, parse_condition},
    keys::{extract_item_keys, extract_pk_sk, item_to_storage_value, storage_value_to_item},
    sqlite_store::SqliteStore,
    state::{DynamoItem, DynamoState, StreamRecord, StreamRecordData},
    throttle::BucketKind,
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

use super::reject_attrs_to_get_with_projection;
use super::{
    build_consumed_capacity, get_expr_attr_names, get_expr_attr_values, opt_str,
    read_capacity_units, require_str, validate_expr_attr_values, write_capacity_units,
};

/// Build a `ConditionalCheckFailedException` matching the real DynamoDB shape:
/// HTTP 400, the standard message, and (when the caller asked for `ALL_OLD`
/// via `ReturnValuesOnConditionCheckFailure`) the existing item attached as
/// the `Item` extra so SDKs can read it from the typed exception.
fn conditional_check_failed(input: &Value, existing: Option<&DynamoItem>) -> AwsError {
    let mut err = AwsError::bad_request(
        "ConditionalCheckFailedException",
        "The conditional request failed",
    );
    if matches!(
        opt_str(input, "ReturnValuesOnConditionCheckFailure"),
        Some("ALL_OLD")
    ) && let Some(item) = existing
    {
        err = err.with_extra("Item", item_to_json(item));
    }
    err
}

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
        table.stream_records.pop_front();
    }
    table.stream_records.push_back(record.clone());

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

/// Validate the AWS rules for typed-set attributes (SS / NS / BS):
///   * each set must be non-empty,
///   * elements within a set must be unique.
///
/// AWS rejects violations with `ValidationException` ("One or more
/// parameter values were invalid"). Walks `Value` recursively so sets
/// nested inside lists/maps are caught too.
pub(crate) fn validate_sets(name: &str, value: &Value) -> Result<(), AwsError> {
    let Some(obj) = value.as_object() else {
        return Ok(());
    };
    for tag in ["SS", "NS", "BS"] {
        if let Some(arr) = obj.get(tag).and_then(Value::as_array) {
            if arr.is_empty() {
                return Err(AwsError::validation(format!(
                    "One or more parameter values were invalid: \
                     An {tag} attribute must contain at least one element \
                     (attribute: {name})"
                )));
            }
            let mut seen: std::collections::HashSet<&str> =
                std::collections::HashSet::with_capacity(arr.len());
            for elem in arr {
                let Some(s) = elem.as_str() else {
                    return Err(AwsError::validation(format!(
                        "One or more parameter values were invalid: \
                         {tag} elements must be strings (attribute: {name})"
                    )));
                };
                if !seen.insert(s) {
                    return Err(AwsError::validation(format!(
                        "One or more parameter values were invalid: \
                         {tag} contains duplicates (attribute: {name})"
                    )));
                }
            }
        }
    }
    // Recurse into list / map elements so a set buried inside an L or M
    // attribute is also caught.
    if let Some(arr) = obj.get("L").and_then(Value::as_array) {
        for (i, v) in arr.iter().enumerate() {
            validate_sets(&format!("{name}[{i}]"), v)?;
        }
    }
    if let Some(map) = obj.get("M").and_then(Value::as_object) {
        for (k, v) in map {
            validate_sets(&format!("{name}.{k}"), v)?;
        }
    }
    Ok(())
}

/// Run [`validate_sets`] over every attribute of an item. Used as the
/// boundary check on PutItem / UpdateItem before the value lands in
/// storage.
pub(crate) fn validate_item_sets(item: &DynamoItem) -> Result<(), AwsError> {
    for (name, value) in item {
        validate_sets(name, value)?;
    }
    Ok(())
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

/// Approximate the on-the-wire bytes a JSON value will contribute to
/// a response. Walks the tree summing string lengths plus small
/// constants for structural overhead — a couple orders of magnitude
/// faster than `serde_json::to_string`. Used to enforce the
/// AWS-defined response caps on Query/Scan/BatchGetItem/TransactGetItems
/// without paying serialization cost twice.
pub(crate) fn estimate_value_bytes(v: &Value) -> usize {
    match v {
        Value::Null => 1,
        Value::Bool(_) => 1,
        Value::Number(n) => n.to_string().len(),
        Value::String(s) => s.len() + 2,
        Value::Array(arr) => 2 + arr.iter().map(estimate_value_bytes).sum::<usize>() + arr.len(),
        Value::Object(map) => {
            2 + map
                .iter()
                .map(|(k, vv)| k.len() + 2 + estimate_value_bytes(vv) + 2)
                .sum::<usize>()
        }
    }
}

/// Estimate bytes for a typed DynamoItem (sum of attribute names +
/// each AttributeValue subtree + small per-attribute overhead).
pub(crate) fn estimate_item_bytes(item: &DynamoItem) -> usize {
    let mut total = 0usize;
    for (name, value) in item {
        total += name.len();
        total += estimate_value_bytes(value);
    }
    total + item.len() * 4 + 2
}

/// AWS caps every persisted DynamoDB item at 400 KB — applies to
/// PutItem, UpdateItem, BatchWriteItem.PutRequest, and TransactWriteItems
/// Put / Update. Shared via this constant so all writers reject the
/// same threshold.
pub(crate) const ITEM_MAX_BYTES: usize = 400 * 1024;

pub fn put_item(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?.to_string();
    validate_expr_attr_values(input)?;

    let item = parse_item(&input["Item"])
        .ok_or_else(|| AwsError::validation("Item is required and must be a map"))?;
    validate_item_sets(&item)?;

    let item_bytes = estimate_item_bytes(&item);
    if item_bytes > ITEM_MAX_BYTES {
        return Err(AwsError::validation(format!(
            "Item size {item_bytes} bytes exceeds the {ITEM_MAX_BYTES}-byte (400 KB) per-item cap"
        )));
    }

    // Extracted SQLite keys (pk/sk + per-GSI key columns) computed inside
    // the lock so we get them while we hold the canonical schema view.
    // Pull schema-derived bits up front, then drop the dashmap guard so
    // we never hold it across SQLite IO.
    let (sqlite_keys, keys_item) = {
        let table = state.tables.get(&table_name).ok_or_else(|| {
            AwsError::service_not_found(
                "ResourceNotFoundException",
                format!("Cannot do operations on a non-existent table: {table_name}"),
            )
        })?;

        // Validate that BOTH key attributes are present. Only checking the
        // hash key let through items that omit the sort key; AWS requires
        // every key in KeySchema to be supplied or returns ValidationException.
        if let Some(hk) = table.hash_key()
            && !item.contains_key(hk)
        {
            return Err(AwsError::validation(format!(
                "One or more parameter values were invalid: Missing the key {hk} in the item"
            )));
        }
        if let Some(rk) = table.range_key()
            && !item.contains_key(rk)
        {
            return Err(AwsError::validation(format!(
                "One or more parameter values were invalid: Missing the key {rk} in the item"
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
    let old_item = fetch_existing(sqlite, ctx, &table_name, &sqlite_keys.pk, &sqlite_keys.sk)?;

    if let Some(cond_expr) = opt_str(input, "ConditionExpression") {
        let condition = parse_condition(cond_expr)?;
        let empty_item: DynamoItem = DynamoItem::new();
        let check_item = old_item.as_ref().unwrap_or(&empty_item);
        if !evaluate_condition(&condition, check_item, &expr_attr_names, &expr_attr_values)? {
            return Err(conditional_check_failed(input, old_item.as_ref()));
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
    let write_units = write_capacity_units(item_bytes, false);
    state.enforce_throughput(&table_name, BucketKind::Write, write_units)?;
    if let Some(cc) = build_consumed_capacity(input, &table_name, 0.0, write_units) {
        result["ConsumedCapacity"] = cc;
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
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    let key = parse_item(&input["Key"]).ok_or_else(|| AwsError::validation("Key is required"))?;

    let (pk, sk) = extract_pk_sk(&table, &key)
        .ok_or_else(|| AwsError::validation("Could not construct item key"))?;

    let expr_attr_names = get_expr_attr_names(input);
    let projection_expr = opt_str(input, "ProjectionExpression");
    reject_attrs_to_get_with_projection(input, projection_expr)?;

    // Drop the dashmap guard before SQLite IO to avoid pinning the shard.
    drop(table);

    let consistent_read = input
        .get("ConsistentRead")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let raw = sqlite.get_item(&ctx.account_id, &ctx.region, table_name, &pk, &sk)?;
    let (mut response, bytes) = match raw {
        None => (json!({}), 0usize),
        Some(stored) => {
            let item = crate::keys::storage_value_to_item(stored)
                .ok_or_else(|| AwsError::internal("DynamoDB stored attrs is not an object"))?;
            let bytes = estimate_item_bytes(&item);
            let projected = apply_projection(&item, projection_expr, &expr_attr_names);
            (json!({ "Item": item_to_json(&projected) }), bytes)
        }
    };
    let read_units = read_capacity_units(bytes, consistent_read, false);
    state.enforce_throughput(table_name, BucketKind::Read, read_units)?;
    if let Some(cc) = build_consumed_capacity(input, table_name, read_units, 0.0) {
        response["ConsumedCapacity"] = cc;
    }
    Ok(response)
}

pub fn delete_item(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?.to_string();
    validate_expr_attr_values(input)?;

    let key = parse_item(&input["Key"]).ok_or_else(|| AwsError::validation("Key is required"))?;

    let (sqlite_pk_sk, keys_item) = {
        let table = state.tables.get(&table_name).ok_or_else(|| {
            AwsError::service_not_found(
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
    let old_item = fetch_existing(sqlite, ctx, &table_name, &sqlite_pk_sk.0, &sqlite_pk_sk.1)?;

    if let Some(cond_expr) = opt_str(input, "ConditionExpression") {
        let condition = parse_condition(cond_expr)?;
        let empty_item: DynamoItem = DynamoItem::new();
        let existing = old_item.as_ref().unwrap_or(&empty_item);
        if !evaluate_condition(&condition, existing, &expr_attr_names, &expr_attr_values)? {
            return Err(conditional_check_failed(input, old_item.as_ref()));
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
    let old_bytes = old_item.as_ref().map(estimate_item_bytes).unwrap_or(0);
    if return_values == "ALL_OLD"
        && let Some(old) = old_item
    {
        result["Attributes"] = item_to_json(&old);
    }
    let write_units = write_capacity_units(old_bytes, false);
    state.enforce_throughput(&table_name, BucketKind::Write, write_units)?;
    if let Some(cc) = build_consumed_capacity(input, &table_name, 0.0, write_units) {
        result["ConsumedCapacity"] = cc;
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
    validate_expr_attr_values(input)?;

    let key = parse_item(&input["Key"]).ok_or_else(|| AwsError::validation("Key is required"))?;

    let (sqlite_pk_sk, keys_item) = {
        let table = state.tables.get(&table_name).ok_or_else(|| {
            AwsError::service_not_found(
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
    let old_item = fetch_existing(sqlite, ctx, &table_name, &sqlite_pk_sk.0, &sqlite_pk_sk.1)?;

    if let Some(cond_expr) = opt_str(input, "ConditionExpression") {
        let condition = parse_condition(cond_expr)?;
        let empty_item: DynamoItem = DynamoItem::new();
        let existing = old_item.as_ref().unwrap_or(&empty_item);
        if !evaluate_condition(&condition, existing, &expr_attr_names, &expr_attr_values)? {
            return Err(conditional_check_failed(input, old_item.as_ref()));
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

    // The UpdateExpression may have left a set in an invalid state (empty
    // after DELETE, duplicates after ADD, etc.). Re-validate the merged
    // item so we don't persist something the AWS API would have rejected.
    validate_item_sets(&new_item)?;

    // Re-extract SQLite keys from the merged item — UpdateExpression may
    // have introduced or changed GSI key attributes.
    let sqlite_keys = {
        let table = state.tables.get(&table_name).ok_or_else(|| {
            AwsError::service_not_found(
                "ResourceNotFoundException",
                format!("Cannot do operations on a non-existent table: {table_name}"),
            )
        })?;
        extract_item_keys(&table, &new_item)
            .ok_or_else(|| AwsError::validation("Could not extract SQLite keys"))?
    };

    let new_item_bytes = estimate_item_bytes(&new_item);
    if new_item_bytes > ITEM_MAX_BYTES {
        return Err(AwsError::validation(format!(
            "Updated item size {new_item_bytes} bytes exceeds the {ITEM_MAX_BYTES}-byte (400 KB) per-item cap"
        )));
    }

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
            // The diff of the old item against the new: attributes that
            // were modified or removed by the update, in their pre-update
            // form. Key attributes never participate.
            let key_names: std::collections::HashSet<&String> = key.keys().collect();
            if let Some(old) = old_item {
                let mut diff = DynamoItem::new();
                for (k, v) in old.iter() {
                    if key_names.contains(k) {
                        continue;
                    }
                    if new_item.get(k) != Some(v) {
                        diff.insert(k.clone(), v.clone());
                    }
                }
                if !diff.is_empty() {
                    result["Attributes"] = item_to_json(&diff);
                }
            }
        }
        "UPDATED_NEW" => {
            // The diff of the new item against the old: attributes added
            // or changed by the update, in their post-update form.
            let key_names: std::collections::HashSet<&String> = key.keys().collect();
            let mut diff = DynamoItem::new();
            for (k, v) in new_item.iter() {
                if key_names.contains(k) {
                    continue;
                }
                if old_item.as_ref().and_then(|o| o.get(k)) != Some(v) {
                    diff.insert(k.clone(), v.clone());
                }
            }
            if !diff.is_empty() {
                result["Attributes"] = item_to_json(&diff);
            }
        }
        _ => {}
    }

    let write_units = write_capacity_units(new_item_bytes, false);
    state.enforce_throughput(&table_name, BucketKind::Write, write_units)?;
    if let Some(cc) = build_consumed_capacity(input, &table_name, 0.0, write_units) {
        result["ConsumedCapacity"] = cc;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{KeySchemaElement, Table};
    use serde_json::json;
    use std::collections::VecDeque;

    #[test]
    fn validate_sets_rejects_empty_string_set() {
        let v = json!({ "SS": [] });
        assert!(validate_sets("attr", &v).is_err());
    }

    #[test]
    fn validate_sets_rejects_duplicate_string_set_elements() {
        let v = json!({ "SS": ["a", "a"] });
        let err = validate_sets("attr", &v).unwrap_err();
        assert!(err.message.contains("duplicates"));
    }

    #[test]
    fn validate_sets_accepts_unique_non_empty_set() {
        let v = json!({ "SS": ["a", "b"] });
        assert!(validate_sets("attr", &v).is_ok());
    }

    #[test]
    fn validate_sets_recurses_into_lists_and_maps() {
        // Set buried inside a list element must still be caught.
        let v = json!({ "L": [ { "M": { "tags": { "NS": [] } } } ] });
        assert!(validate_sets("attr", &v).is_err());
    }

    #[test]
    fn validate_sets_handles_each_typed_set_kind() {
        for tag in ["SS", "NS", "BS"] {
            let dup = json!({ tag: ["x", "x"] });
            assert!(
                validate_sets("attr", &dup).is_err(),
                "{tag} duplicates must reject"
            );
            let empty = json!({ tag: [] });
            assert!(
                validate_sets("attr", &empty).is_err(),
                "{tag} empty must reject"
            );
        }
    }

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
        };
        state.tables.insert("t".into(), table);
        state
    }

    #[test]
    fn put_item_does_not_grow_in_memory_store() {
        // Bulk insert proves the memory-pressure regression that motivated
        // this whole refactor: 1k rows in, sqlite carries them all and
        // there's no in-memory items map left to grow.
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

        assert_eq!(
            sqlite
                .count_items(&ctx.account_id, &ctx.region, "t")
                .unwrap(),
            1000
        );
    }

    #[test]
    fn put_item_returns_consumed_capacity_only_when_requested() {
        let state = make_state_with_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        let item = json!({
            "TableName": "t",
            "Item": { "pk": {"S": "p"}, "sk": {"S": "s"}, "n": {"N": "1"} },
        });
        let resp = put_item(&state, &sqlite, &item, &c).unwrap();
        assert!(resp.get("ConsumedCapacity").is_none());

        let mut item_with = item.clone();
        item_with["ReturnConsumedCapacity"] = json!("TOTAL");
        let resp = put_item(&state, &sqlite, &item_with, &c).unwrap();
        let cc = resp.get("ConsumedCapacity").unwrap();
        assert_eq!(cc["TableName"], json!("t"));
        // Tiny item costs at least 1 WCU.
        assert!(cc["WriteCapacityUnits"].as_f64().unwrap() >= 1.0);
        assert!(cc["CapacityUnits"].as_f64().unwrap() >= 1.0);
    }

    #[test]
    fn get_item_returns_consumed_capacity_with_read_units() {
        let state = make_state_with_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": { "pk": {"S": "p"}, "sk": {"S": "s"} },
            }),
            &c,
        )
        .unwrap();

        let resp = get_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Key": { "pk": {"S": "p"}, "sk": {"S": "s"} },
                "ReturnConsumedCapacity": "TOTAL",
                "ConsistentRead": true,
            }),
            &c,
        )
        .unwrap();
        let cc = resp.get("ConsumedCapacity").unwrap();
        // Strongly consistent, tiny item → exactly 1 RCU.
        assert_eq!(cc["ReadCapacityUnits"].as_f64().unwrap(), 1.0);
        assert_eq!(cc["CapacityUnits"].as_f64().unwrap(), 1.0);
    }

    #[test]
    fn put_item_rejects_missing_sort_key() {
        let state = make_state_with_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let ctx = ctx();
        let err = put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": { "pk": {"S": "tenant"} },  // sk omitted
            }),
            &ctx,
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("sk"));
    }

    #[test]
    fn put_item_rejects_missing_hash_key() {
        let state = make_state_with_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let ctx = ctx();
        let err = put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": { "sk": {"S": "row"} },  // pk omitted
            }),
            &ctx,
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("pk"));
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
    fn put_item_conditional_failure_returns_existing_item_when_requested() {
        let state = make_state_with_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let ctx = ctx();

        // Seed an item that the next put will collide with.
        let seed = json!({
            "TableName": "t",
            "Item": {"pk": {"S": "p"}, "sk": {"S": "s"}, "v": {"N": "1"}}
        });
        put_item(&state, &sqlite, &seed, &ctx).unwrap();

        // Conditional put that fails because the item already exists.
        let conflicting = json!({
            "TableName": "t",
            "Item": {"pk": {"S": "p"}, "sk": {"S": "s"}, "v": {"N": "2"}},
            "ConditionExpression": "attribute_not_exists(pk)",
            "ReturnValuesOnConditionCheckFailure": "ALL_OLD",
        });
        let err = put_item(&state, &sqlite, &conflicting, &ctx).unwrap_err();
        assert_eq!(err.code, "ConditionalCheckFailedException");
        assert_eq!(err.status.as_u16(), 400);
        let extras = err.extras.as_ref().expect("extras populated");
        let item = extras.get("Item").expect("Item attached on failure");
        assert_eq!(item["v"], json!({"N": "1"}));

        // Without the opt-in flag, no Item is attached.
        let no_flag = json!({
            "TableName": "t",
            "Item": {"pk": {"S": "p"}, "sk": {"S": "s"}, "v": {"N": "3"}},
            "ConditionExpression": "attribute_not_exists(pk)",
        });
        let err = put_item(&state, &sqlite, &no_flag, &ctx).unwrap_err();
        assert!(err.extras.is_none(), "Item should be opt-in");
    }

    #[test]
    fn update_item_returns_only_modified_attrs_for_updated_new() {
        let state = make_state_with_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let ctx = ctx();
        // Seed with three attributes.
        put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": {
                    "pk": {"S": "p"}, "sk": {"S": "s"},
                    "a": {"N": "1"}, "b": {"S": "old"}, "c": {"S": "stays"},
                },
            }),
            &ctx,
        )
        .unwrap();

        let resp = update_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Key": { "pk": {"S": "p"}, "sk": {"S": "s"} },
                "UpdateExpression": "SET a = :a, b = :b",
                "ExpressionAttributeValues": { ":a": {"N": "2"}, ":b": {"S": "new"} },
                "ReturnValues": "UPDATED_NEW",
            }),
            &ctx,
        )
        .unwrap();
        let attrs = &resp["Attributes"];
        // Only a and b changed; c is unchanged so must NOT appear.
        assert_eq!(attrs["a"]["N"], json!("2"));
        assert_eq!(attrs["b"]["S"], json!("new"));
        assert!(
            attrs.get("c").is_none(),
            "unchanged attribute must be filtered"
        );
        // Key attributes never participate in UPDATED_*.
        assert!(attrs.get("pk").is_none());
        assert!(attrs.get("sk").is_none());
    }

    #[test]
    fn update_item_returns_pre_update_values_for_updated_old() {
        let state = make_state_with_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let ctx = ctx();
        put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": {
                    "pk": {"S": "p"}, "sk": {"S": "s"},
                    "a": {"N": "1"}, "c": {"S": "stays"},
                },
            }),
            &ctx,
        )
        .unwrap();

        let resp = update_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Key": { "pk": {"S": "p"}, "sk": {"S": "s"} },
                "UpdateExpression": "SET a = :a",
                "ExpressionAttributeValues": { ":a": {"N": "99"} },
                "ReturnValues": "UPDATED_OLD",
            }),
            &ctx,
        )
        .unwrap();
        let attrs = &resp["Attributes"];
        assert_eq!(attrs["a"]["N"], json!("1"));
        assert!(attrs.get("c").is_none());
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

    /// Spin up a PROVISIONED 1-WCU table with a tiny burst window
    /// (1 WCU * 300 s = 300 burst tokens) and PutItem until the
    /// bucket runs dry. Once exhausted every further write should
    /// surface `ProvisionedThroughputExceededException`.
    #[test]
    fn put_item_throttles_when_wcu_exhausted_on_provisioned_table() {
        let state = make_state_with_table();
        // Flip the table from PAY_PER_REQUEST to PROVISIONED + 1 WCU.
        state.tables.alter("t", |_, mut t| {
            t.billing_mode = "PROVISIONED".into();
            t.write_capacity_units = 1;
            t
        });
        let sqlite = SqliteStore::in_memory().unwrap();
        let ctx = ctx();

        // Each item is well under 1 KiB, so each PutItem charges 1
        // WCU. 300 succeed, 301st throttles.
        for i in 0..300 {
            let input = json!({
                "TableName": "t",
                "Item": { "pk": {"S": "p"}, "sk": {"S": format!("{i}")} }
            });
            put_item(&state, &sqlite, &input, &ctx)
                .unwrap_or_else(|e| panic!("put {i} failed: {}", e.message));
        }
        let input = json!({
            "TableName": "t",
            "Item": { "pk": {"S": "p"}, "sk": {"S": "exhausted"} }
        });
        let err = put_item(&state, &sqlite, &input, &ctx).unwrap_err();
        assert_eq!(err.code, "ProvisionedThroughputExceededException");
    }

    /// PAY_PER_REQUEST is the documented "no throttling" path: the
    /// emulator must never reject a write on capacity grounds even
    /// after firing thousands of operations in a row.
    #[test]
    fn pay_per_request_table_is_never_throttled() {
        let state = make_state_with_table(); // defaults to PAY_PER_REQUEST
        let sqlite = SqliteStore::in_memory().unwrap();
        let ctx = ctx();

        for i in 0..5_000 {
            let input = json!({
                "TableName": "t",
                "Item": { "pk": {"S": "p"}, "sk": {"S": format!("{i}")} }
            });
            put_item(&state, &sqlite, &input, &ctx).unwrap();
        }
    }
}
