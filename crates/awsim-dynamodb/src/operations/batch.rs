use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::DynamoState;

use super::item::{item_to_json, parse_item};

pub fn batch_get_item(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
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

        let table = match state.tables.get(table_name) {
            Some(t) => t,
            None => {
                return Err(AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Cannot do operations on a non-existent table: {table_name}"),
                ));
            }
        };

        let mut table_items: Vec<Value> = Vec::new();

        for key_val in keys {
            let key = match parse_item(key_val) {
                Some(k) => k,
                None => continue,
            };

            let composite_key = match table.composite_key(&key) {
                Some(ck) => ck,
                None => continue,
            };

            if let Some(item) = table.items.get(&composite_key) {
                table_items.push(item_to_json(item));
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
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let request_items = input
        .get("RequestItems")
        .and_then(|v| v.as_object())
        .ok_or_else(|| AwsError::validation("RequestItems is required"))?;

    let unprocessed_items = serde_json::Map::new();

    for (table_name, requests) in request_items {
        let requests_arr = requests.as_array().ok_or_else(|| {
            AwsError::validation(format!("Requests for {table_name} must be an array"))
        })?;

        let mut table = match state.tables.get_mut(table_name.as_str()) {
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
                if let Some(ck) = table.composite_key(&item) {
                    table.items.insert(ck, item);
                }
            } else if let Some(delete_req) = req.get("DeleteRequest") {
                let key = match parse_item(&delete_req["Key"]) {
                    Some(k) => k,
                    None => continue,
                };
                if let Some(ck) = table.composite_key(&key) {
                    table.items.remove(&ck);
                }
            }
        }
    }

    Ok(json!({ "UnprocessedItems": unprocessed_items }))
}
