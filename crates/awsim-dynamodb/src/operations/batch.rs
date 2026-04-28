use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    keys::{extract_item_keys, extract_pk_sk, item_to_storage_value, storage_value_to_item},
    sqlite_store::SqliteStore,
    state::DynamoState,
};

use super::item::{item_to_json, parse_item};

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

    let mut responses = serde_json::Map::new();
    let unprocessed_keys = serde_json::Map::new();

    for (table_name, table_request) in request_items {
        let keys = table_request
            .get("Keys")
            .and_then(|v| v.as_array())
            .ok_or_else(|| AwsError::validation(format!("Keys required for table {table_name}")))?;

        // Resolve the composite (pk, sk) pairs while holding the schema
        // guard, then drop it before SQLite IO.
        let pk_sk_pairs: Vec<(String, String)> = {
            let table = match state.tables.get(table_name) {
                Some(t) => t,
                None => {
                    return Err(AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Cannot do operations on a non-existent table: {table_name}"),
                    ));
                }
            };
            keys.iter()
                .filter_map(|key_val| {
                    let key = parse_item(key_val)?;
                    extract_pk_sk(&table, &key)
                })
                .collect()
        };

        let mut table_items: Vec<Value> = Vec::new();
        for (pk, sk) in pk_sk_pairs {
            if let Some(stored) =
                sqlite.get_item(&ctx.account_id, &ctx.region, table_name, &pk, &sk)?
                && let Some(item) = storage_value_to_item(stored)
            {
                table_items.push(item_to_json(&item));
            }
        }

        responses.insert(table_name.clone(), json!(table_items));
    }

    Ok(json!({
        "Responses": responses,
        "UnprocessedKeys": unprocessed_keys
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

    let unprocessed_items = serde_json::Map::new();

    // Collect SQLite mirror operations while we hold each table lock,
    // then apply them after the lock is released so we don't hold the
    // DashMap entry across blocking sqlite IO.
    enum SqliteOp {
        Put {
            table: String,
            pk: String,
            sk: String,
            attrs: Value,
            gsi: [(Option<String>, Option<String>); crate::sqlite_store::MAX_GSI_SLOTS],
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
                return Err(AwsError::not_found(
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
                if let Some(keys) = extract_item_keys(&table, &item) {
                    let attrs = item_to_storage_value(&item);
                    sqlite_ops.push(SqliteOp::Put {
                        table: table_name.clone(),
                        pk: keys.pk,
                        sk: keys.sk,
                        attrs,
                        gsi: keys.gsi,
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
                sqlite.put_item(
                    &ctx.account_id,
                    &ctx.region,
                    &table,
                    &pk,
                    &sk,
                    &attrs,
                    &gsi,
                )?;
            }
            SqliteOp::Delete { table, pk, sk } => {
                sqlite.delete_item(&ctx.account_id, &ctx.region, &table, &pk, &sk)?;
            }
        }
    }

    Ok(json!({ "UnprocessedItems": unprocessed_items }))
}
