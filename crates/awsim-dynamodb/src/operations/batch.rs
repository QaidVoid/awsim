use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    keys::{
        extract_item_keys, extract_pk_sk, item_to_storage_value, resolve_key, storage_value_to_item,
    },
    sqlite_store::SqliteStore,
    state::DynamoState,
    throttle::BucketKind,
};

use super::item::{
    ITEM_MAX_BYTES, estimate_item_bytes, estimate_value_bytes, item_to_json, parse_item,
    reject_empty_key_values, validate_item,
};
use super::{
    item_collection_metrics, push_item_collection, read_capacity_units, write_capacity_units,
};

/// AWS BatchGetItem caps a single call at 100 keys total across all
/// tables, and at 16 MB of response payload. Items beyond the byte
/// cap aren't returned — their keys are echoed back in
/// `UnprocessedKeys` so the client can retry. Without these caps
/// awsim materialises arbitrarily large responses in memory.
const BATCH_GET_MAX_KEYS: usize = 100;
const BATCH_GET_MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// AWS BatchWriteItem caps a single call at 25 PutRequest /
/// DeleteRequest entries total. The 400 KB per-item cap is shared
/// with PutItem via `ITEM_MAX_BYTES`.
const BATCH_WRITE_MAX_ITEMS: usize = 25;

pub fn batch_get_item(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let request_items = input
        .get("RequestItems")
        .and_then(|v| v.as_object())
        .ok_or_else(|| AwsError::validation("RequestItems is required"))?;

    // Resolve every key up front so we can bail on the 100-key limit
    // before touching SQLite, and so each key carries its original
    // JSON value for echoing in UnprocessedKeys.
    struct PendingKey {
        table_name: String,
        original_key: Value,
        pk: String,
        sk: String,
    }
    let mut pending: Vec<PendingKey> = Vec::new();
    // Tables whose request entry asks for strongly consistent reads.
    // ConsistentRead is a per-table flag in BatchGetItem, and it
    // doubles that table's RCU charge below.
    let mut consistent_tables: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (table_name, table_request) in request_items {
        let keys = table_request
            .get("Keys")
            .and_then(|v| v.as_array())
            .ok_or_else(|| AwsError::validation(format!("Keys required for table {table_name}")))?;

        if table_request
            .get("ConsistentRead")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            consistent_tables.insert(table_name.clone());
        }

        let table = match state.tables.get(table_name) {
            Some(t) => t,
            None => {
                return Err(AwsError::service_not_found(
                    "ResourceNotFoundException",
                    format!("Cannot do operations on a non-existent table: {table_name}"),
                ));
            }
        };
        for key_val in keys {
            let Some(parsed) = parse_item(key_val) else {
                continue;
            };
            let (pk, sk) = resolve_key(&table, &parsed).map_err(AwsError::validation)?;
            pending.push(PendingKey {
                table_name: table_name.clone(),
                original_key: key_val.clone(),
                pk,
                sk,
            });
        }
    }

    if pending.len() > BATCH_GET_MAX_KEYS {
        return Err(AwsError::validation(format!(
            "BatchGetItem cannot process more than {BATCH_GET_MAX_KEYS} keys per call ({} supplied)",
            pending.len()
        )));
    }

    let mut responses: std::collections::HashMap<String, Vec<Value>> =
        std::collections::HashMap::new();
    let mut unprocessed: std::collections::HashMap<String, Vec<Value>> =
        std::collections::HashMap::new();
    let mut response_bytes = 0usize;
    let mut per_table_units: std::collections::HashMap<String, f64> =
        std::collections::HashMap::new();
    let mut cap_reached = false;

    for key in pending {
        if cap_reached {
            unprocessed
                .entry(key.table_name.clone())
                .or_default()
                .push(key.original_key);
            continue;
        }
        let consistent = consistent_tables.contains(&key.table_name);

        let stored = sqlite.get_item(
            &ctx.account_id,
            &ctx.region,
            &key.table_name,
            &key.pk,
            &key.sk,
        )?;
        let item = stored.and_then(storage_value_to_item);
        let Some(item) = item else {
            // A miss still consumes the minimum read capacity, exactly
            // like a standalone GetItem on a nonexistent key.
            *per_table_units.entry(key.table_name).or_default() +=
                read_capacity_units(0, consistent, false);
            continue;
        };
        let item_json = item_to_json(&item);
        let item_bytes = estimate_value_bytes(&item_json);

        // If this single item would push us past the cap and we've
        // already returned at least one item for this call, defer it —
        // matches AWS behaviour where a partial response + an
        // UnprocessedKeys entry beats a hard error.
        if response_bytes > 0 && response_bytes + item_bytes > BATCH_GET_MAX_RESPONSE_BYTES {
            cap_reached = true;
            unprocessed
                .entry(key.table_name.clone())
                .or_default()
                .push(key.original_key);
            continue;
        }

        response_bytes += item_bytes;
        *per_table_units.entry(key.table_name.clone()).or_default() +=
            read_capacity_units(item_bytes, consistent, false);
        responses.entry(key.table_name).or_default().push(item_json);
    }

    // Charge each touched table's read bucket. AWS sizes BatchGetItem
    // per item: each item rounds up to 4 KiB chunks on its own (0.5
    // RCU per chunk eventually consistent, 1 RCU with ConsistentRead)
    // rather than summing bytes across the batch first.
    for (table, units) in &per_table_units {
        state.charge_throughput(ctx, table, BucketKind::Read, *units)?;
    }

    let responses_json: serde_json::Map<String, Value> = responses
        .into_iter()
        .map(|(table, items)| (table, Value::Array(items)))
        .collect();
    let unprocessed_json: serde_json::Map<String, Value> = unprocessed
        .into_iter()
        .map(|(table, keys)| (table, json!({ "Keys": keys })))
        .collect();

    Ok(json!({
        "Responses": responses_json,
        "UnprocessedKeys": unprocessed_json
    }))
}

pub fn batch_write_item(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let request_items = input
        .get("RequestItems")
        .and_then(|v| v.as_object())
        .ok_or_else(|| AwsError::validation("RequestItems is required"))?;

    let total_requests: usize = request_items
        .values()
        .filter_map(|v| v.as_array())
        .map(|a| a.len())
        .sum();
    if total_requests > BATCH_WRITE_MAX_ITEMS {
        return Err(AwsError::validation(format!(
            "BatchWriteItem cannot process more than {BATCH_WRITE_MAX_ITEMS} requests per call ({total_requests} supplied)"
        )));
    }

    let unprocessed_items = serde_json::Map::new();

    // Collect SQLite mirror operations while we hold each table lock,
    // then apply them after the lock is released so we don't hold the
    // DashMap entry across blocking sqlite IO.
    //
    // The gsi array (5 × Option<String> pairs) is boxed to keep the
    // SqliteOp enum size in check — without it Put dwarfs Delete by
    // ~500 bytes, which clippy (rightly) flags for `Vec<SqliteOp>`.
    enum SqliteOp {
        Put {
            table: String,
            pk: String,
            sk: String,
            attrs: Value,
            gsi: Box<[(Option<String>, Option<String>); crate::sqlite_store::MAX_GSI_SLOTS]>,
        },
        Delete {
            table: String,
            pk: String,
            sk: String,
        },
    }
    let mut sqlite_ops: Vec<SqliteOp> = Vec::new();
    // Per-table WCU totals. AWS charges each PutRequest/DeleteRequest
    // individually (1 WCU per 1 KiB chunk, per item), so units are
    // accumulated per request rather than summing bytes per table.
    let mut write_units_by_table: std::collections::HashMap<String, f64> =
        std::collections::HashMap::new();
    // ItemCollectionMetrics, when requested, is a per-table array of one
    // entry per affected item in a table that has an LSI.
    let mut item_collections: serde_json::Map<String, Value> = serde_json::Map::new();

    for (table_name, requests) in request_items {
        let requests_arr = requests.as_array().ok_or_else(|| {
            AwsError::validation(format!("Requests for {table_name} must be an array"))
        })?;

        // Hold the read guard only long enough to extract storage keys —
        // the write path is sqlite-only after stage 4.
        let table = match state.tables.get(table_name.as_str()) {
            Some(t) => t,
            None => {
                return Err(AwsError::service_not_found(
                    "ResourceNotFoundException",
                    format!("Cannot do operations on a non-existent table: {table_name}"),
                ));
            }
        };

        for req in requests_arr {
            if let Some(put_req) = req.get("PutRequest") {
                let item = match parse_item(&put_req["Item"]) {
                    Some(i) => i,
                    None => continue,
                };
                let item_bytes = estimate_item_bytes(&item);
                if item_bytes > ITEM_MAX_BYTES {
                    return Err(AwsError::validation(format!(
                        "Item size {item_bytes} bytes exceeds the {ITEM_MAX_BYTES}-byte (400 KB) per-item cap in table {table_name}"
                    )));
                }
                validate_item(&item)?;
                reject_empty_key_values(&table, &item)?;
                if let Some(keys) = extract_item_keys(&table, &item) {
                    let attrs = item_to_storage_value(&item);
                    *write_units_by_table.entry(table_name.clone()).or_default() +=
                        write_capacity_units(item_bytes, false);
                    if let Some(icm) = item_collection_metrics(input, &table, &item) {
                        push_item_collection(&mut item_collections, table_name, icm);
                    }
                    sqlite_ops.push(SqliteOp::Put {
                        table: table_name.clone(),
                        pk: keys.pk,
                        sk: keys.sk,
                        attrs,
                        gsi: Box::new(keys.gsi),
                    });
                }
            } else if let Some(delete_req) = req.get("DeleteRequest") {
                let key = match parse_item(&delete_req["Key"]) {
                    Some(k) => k,
                    None => continue,
                };
                if let Some((pk, sk)) = extract_pk_sk(&table, &key) {
                    if let Some(icm) = item_collection_metrics(input, &table, &key) {
                        push_item_collection(&mut item_collections, table_name, icm);
                    }
                    sqlite_ops.push(SqliteOp::Delete {
                        table: table_name.clone(),
                        pk,
                        sk,
                    });
                }
            }
        }
    }

    // DeleteRequest charges by the size of the item it removes (1 WCU
    // minimum when the key doesn't exist), which takes a read of the
    // current row. Done here, after the table guards are released and
    // before anything mutates.
    for op in &sqlite_ops {
        let SqliteOp::Delete { table, pk, sk } = op else {
            continue;
        };
        let old_bytes = sqlite
            .get_item(&ctx.account_id, &ctx.region, table, pk, sk)?
            .and_then(storage_value_to_item)
            .map(|i| estimate_item_bytes(&i))
            .unwrap_or(0);
        *write_units_by_table.entry(table.clone()).or_default() +=
            write_capacity_units(old_bytes, false);
    }

    // Charge each touched table's write bucket *before* mutating
    // SQLite. If a table is throttled, none of its writes (and no
    // other table's writes either) land. Matches what the SDK
    // expects for a batch op that hits a capacity wall.
    for (table, units) in &write_units_by_table {
        state.charge_throughput(ctx, table, BucketKind::Write, *units)?;
    }

    for op in sqlite_ops {
        match op {
            SqliteOp::Put {
                table,
                pk,
                sk,
                attrs,
                gsi,
            } => {
                sqlite.put_item(&ctx.account_id, &ctx.region, &table, &pk, &sk, &attrs, &gsi)?;
            }
            SqliteOp::Delete { table, pk, sk } => {
                sqlite.delete_item(&ctx.account_id, &ctx.region, &table, &pk, &sk)?;
            }
        }
    }

    let mut result = json!({ "UnprocessedItems": unprocessed_items });
    if !item_collections.is_empty() {
        result["ItemCollectionMetrics"] = Value::Object(item_collections);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{KeySchemaElement, LocalSecondaryIndex, Projection, Table};
    use std::collections::VecDeque;

    fn ctx() -> RequestContext {
        RequestContext::new("dynamodb", "us-east-1")
    }

    fn state_with_lsi_table() -> DynamoState {
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
            lsi: vec![LocalSecondaryIndex {
                index_name: "byLsi".into(),
                key_schema: vec![KeySchemaElement {
                    attribute_name: "pk".into(),
                    key_type: "HASH".into(),
                }],
                projection: Projection {
                    projection_type: "ALL".into(),
                    non_key_attributes: vec![],
                },
            }],
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
    fn batch_write_returns_item_collection_metrics_for_lsi_table() {
        let state = state_with_lsi_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let res = batch_write_item(
            &state,
            &sqlite,
            &json!({
                "ReturnItemCollectionMetrics": "SIZE",
                "RequestItems": {
                    "t": [
                        {"PutRequest": {"Item": {"pk": {"S": "a"}, "sk": {"S": "1"}}}},
                        {"DeleteRequest": {"Key": {"pk": {"S": "b"}, "sk": {"S": "2"}}}},
                    ]
                }
            }),
            &ctx(),
        )
        .unwrap();

        // Per-table map keyed by table name, one entry per affected item.
        let entries = res["ItemCollectionMetrics"]["t"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["ItemCollectionKey"], json!({"pk": {"S": "a"}}));
    }

    /// ConsistentRead=true doubles a table's RCU charge: a ~5 KiB item
    /// spans two 4 KiB chunks, costing 1 RCU eventually consistent but
    /// 2 RCU strongly consistent. With the read bucket drained to one
    /// token, the consistent read throttles and the eventual one passes.
    #[test]
    fn batch_get_charges_double_rcu_for_consistent_read() {
        let state = state_with_lsi_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let big = "x".repeat(5000);
        batch_write_item(
            &state,
            &sqlite,
            &json!({
                "RequestItems": {
                    "t": [{"PutRequest": {"Item": {
                        "pk": {"S": "a"}, "sk": {"S": "1"}, "blob": {"S": big}
                    }}}]
                }
            }),
            &ctx(),
        )
        .unwrap();

        // 1 RCU -> 300 burst read tokens; leave exactly one.
        state.tables.alter("t", |_, mut t| {
            t.billing_mode = "PROVISIONED".into();
            t.read_capacity_units = 1;
            t
        });
        state
            .enforce_throughput("t", BucketKind::Read, 299.0)
            .unwrap();

        let get_input = |consistent: bool| {
            json!({
                "RequestItems": {
                    "t": {
                        "Keys": [{"pk": {"S": "a"}, "sk": {"S": "1"}}],
                        "ConsistentRead": consistent,
                    }
                }
            })
        };

        let err = batch_get_item(&state, &sqlite, &get_input(true), &ctx()).unwrap_err();
        assert_eq!(err.code, "ProvisionedThroughputExceededException");

        // A failed charge doesn't drain the bucket, so the remaining
        // token still covers the eventually-consistent read.
        let res = batch_get_item(&state, &sqlite, &get_input(false), &ctx()).unwrap();
        assert_eq!(res["Responses"]["t"].as_array().unwrap().len(), 1);
    }

    /// RCU is charged per item, not on pooled table bytes: two ~5 KiB
    /// items cost 1 RCU each eventually consistent (2 total), where
    /// pooling 10 KiB would round to 1.5 RCU. With 1.5 tokens left the
    /// batch must throttle.
    #[test]
    fn batch_get_charges_rcu_per_item() {
        let state = state_with_lsi_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let blob = "x".repeat(5000);
        batch_write_item(
            &state,
            &sqlite,
            &json!({
                "RequestItems": {
                    "t": [
                        {"PutRequest": {"Item": {
                            "pk": {"S": "a"}, "sk": {"S": "1"}, "blob": {"S": blob}
                        }}},
                        {"PutRequest": {"Item": {
                            "pk": {"S": "a"}, "sk": {"S": "2"}, "blob": {"S": blob}
                        }}},
                    ]
                }
            }),
            &ctx(),
        )
        .unwrap();

        state.tables.alter("t", |_, mut t| {
            t.billing_mode = "PROVISIONED".into();
            t.read_capacity_units = 1;
            t
        });
        state
            .enforce_throughput("t", BucketKind::Read, 298.5)
            .unwrap();

        let err = batch_get_item(
            &state,
            &sqlite,
            &json!({
                "RequestItems": {
                    "t": {
                        "Keys": [
                            {"pk": {"S": "a"}, "sk": {"S": "1"}},
                            {"pk": {"S": "a"}, "sk": {"S": "2"}},
                        ]
                    }
                }
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ProvisionedThroughputExceededException");
    }

    /// A DeleteRequest charges by the size of the item it removes:
    /// deleting a ~5 KiB item costs 5 WCU, not the 1 WCU minimum, and
    /// deleting a missing key still charges only the minimum.
    #[test]
    fn batch_write_charges_delete_by_removed_item_size() {
        let state = state_with_lsi_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        batch_write_item(
            &state,
            &sqlite,
            &json!({
                "RequestItems": {
                    "t": [{"PutRequest": {"Item": {
                        "pk": {"S": "a"}, "sk": {"S": "1"}, "blob": {"S": "x".repeat(5000)}
                    }}}]
                }
            }),
            &ctx(),
        )
        .unwrap();

        state.tables.alter("t", |_, mut t| {
            t.billing_mode = "PROVISIONED".into();
            t.write_capacity_units = 1;
            t
        });
        state
            .enforce_throughput("t", BucketKind::Write, 299.0)
            .unwrap();

        let err = batch_write_item(
            &state,
            &sqlite,
            &json!({
                "RequestItems": {
                    "t": [{"DeleteRequest": {"Key": {"pk": {"S": "a"}, "sk": {"S": "1"}}}}]
                }
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ProvisionedThroughputExceededException");

        batch_write_item(
            &state,
            &sqlite,
            &json!({
                "RequestItems": {
                    "t": [{"DeleteRequest": {"Key": {"pk": {"S": "zzz"}, "sk": {"S": "9"}}}}]
                }
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn batch_write_omits_item_collection_metrics_without_lsi() {
        let state = state_with_lsi_table();
        // Strip the LSI so the table no longer has item collections.
        state.tables.get_mut("t").unwrap().lsi.clear();
        let sqlite = SqliteStore::in_memory().unwrap();
        let res = batch_write_item(
            &state,
            &sqlite,
            &json!({
                "ReturnItemCollectionMetrics": "SIZE",
                "RequestItems": {
                    "t": [{"PutRequest": {"Item": {"pk": {"S": "a"}, "sk": {"S": "1"}}}}]
                }
            }),
            &ctx(),
        )
        .unwrap();
        assert!(res.get("ItemCollectionMetrics").is_none());
    }
}
