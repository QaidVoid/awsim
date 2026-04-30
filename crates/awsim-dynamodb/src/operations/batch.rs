use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    keys::{extract_item_keys, extract_pk_sk, item_to_storage_value, storage_value_to_item},
    sqlite_store::SqliteStore,
    state::DynamoState,
};

use super::item::{
    ITEM_MAX_BYTES, estimate_item_bytes, estimate_value_bytes, item_to_json, parse_item,
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

    for (table_name, table_request) in request_items {
        let keys = table_request
            .get("Keys")
            .and_then(|v| v.as_array())
            .ok_or_else(|| AwsError::validation(format!("Keys required for table {table_name}")))?;

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
            let Some((pk, sk)) = extract_pk_sk(&table, &parsed) else {
                continue;
            };
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
    let mut cap_reached = false;

    for key in pending {
        if cap_reached {
            unprocessed
                .entry(key.table_name.clone())
                .or_default()
                .push(key.original_key);
            continue;
        }

        let stored = sqlite.get_item(
            &ctx.account_id,
            &ctx.region,
            &key.table_name,
            &key.pk,
            &key.sk,
        )?;
        let Some(stored) = stored else {
            continue;
        };
        let Some(item) = storage_value_to_item(stored) else {
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
        responses.entry(key.table_name).or_default().push(item_json);
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
                if let Some(keys) = extract_item_keys(&table, &item) {
                    let attrs = item_to_storage_value(&item);
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
                    sqlite_ops.push(SqliteOp::Delete {
                        table: table_name.clone(),
                        pk,
                        sk,
                    });
                }
            }
        }
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

    Ok(json!({ "UnprocessedItems": unprocessed_items }))
}
