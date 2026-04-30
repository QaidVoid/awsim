use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    expressions::{apply_update_expression, evaluate_condition, parse_condition},
    keys::{extract_item_keys, extract_pk_sk, item_to_storage_value, storage_value_to_item},
    sqlite_store::{MAX_GSI_SLOTS, ReadTx, SqliteStore, WriteTx},
    state::{DynamoItem, DynamoState},
};

use super::{
    get_expr_attr_names, get_expr_attr_values,
    item::{estimate_value_bytes, item_to_json, parse_item},
    opt_str,
};

/// Decode a stored sqlite row into a `DynamoItem`. Returns `None` when
/// the row doesn't exist; surfaces an internal error if the row exists
/// but isn't a JSON object (shouldn't happen — we only ever write
/// objects).
fn decode_existing(stored: Option<Value>) -> Result<Option<DynamoItem>, AwsError> {
    stored
        .map(|s| {
            storage_value_to_item(s)
                .ok_or_else(|| AwsError::internal("DynamoDB stored attrs is not an object"))
        })
        .transpose()
}

/// Build a `TransactionCanceledException` with a `CancellationReasons` array
/// shaped like the real DynamoDB response: one entry per TransactItem in
/// request order, where the failed item carries `Code` (and a short `Message`)
/// and every other item is marked `{"Code": "None"}`.
fn transaction_canceled(total: usize, failed_idx: usize, failed_code: &str) -> AwsError {
    let reasons: Vec<Value> = (0..total)
        .map(|i| {
            if i == failed_idx {
                json!({
                    "Code": failed_code,
                    "Message": "The conditional request failed",
                })
            } else {
                json!({ "Code": "None" })
            }
        })
        .collect();

    let summary: Vec<&str> = reasons
        .iter()
        .filter_map(|r| r.get("Code").and_then(Value::as_str))
        .collect();
    let message = format!(
        "Transaction cancelled, please refer cancellation reasons for specific reasons [{}]",
        summary.join(", ")
    );

    AwsError::bad_request("TransactionCanceledException", message)
        .with_extra("CancellationReasons", Value::Array(reasons))
}

/// AWS TransactGetItems caps a single call at 100 actions and 4 MB
/// of response payload. Unlike BatchGetItem, transactions don't
/// paginate — exceeding the response cap is a hard
/// ValidationException, so the workload doesn't quietly leak memory
/// while building a 100 MB Value tree.
const TRANSACT_GET_MAX_ACTIONS: usize = 100;
const TRANSACT_GET_MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

pub fn transact_get_items(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let transact_items = input
        .get("TransactItems")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AwsError::validation("TransactItems is required"))?;

    if transact_items.len() > TRANSACT_GET_MAX_ACTIONS {
        return Err(AwsError::validation(format!(
            "TransactGetItems cannot process more than {TRANSACT_GET_MAX_ACTIONS} actions per call ({} supplied)",
            transact_items.len()
        )));
    }

    // Resolve every (table, pk, sk) tuple before opening the read txn so
    // we don't hold the dashmap guard across SQLite IO.
    struct ResolvedGet {
        table_name: String,
        pk: String,
        sk: String,
    }
    let mut gets: Vec<ResolvedGet> = Vec::with_capacity(transact_items.len());

    for tx_item in transact_items {
        let get = tx_item.get("Get").ok_or_else(|| {
            AwsError::validation("Each TransactGetItem must have a Get operation")
        })?;
        let table_name = get
            .get("TableName")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AwsError::validation("TableName is required in Get"))?;
        let key = parse_item(&get["Key"])
            .ok_or_else(|| AwsError::validation("Key is required in Get"))?;

        let (pk, sk) = {
            let table = state.tables.get(table_name).ok_or_else(|| {
                AwsError::service_not_found(
                    "ResourceNotFoundException",
                    format!("Cannot do operations on a non-existent table: {table_name}"),
                )
            })?;
            extract_pk_sk(&table, &key)
                .ok_or_else(|| AwsError::validation("Could not construct item key"))?
        };

        gets.push(ResolvedGet {
            table_name: table_name.to_string(),
            pk,
            sk,
        });
    }

    // Snapshot read across all gets — a deferred sqlite txn pins the
    // visible commit point.
    let responses =
        sqlite.with_read_transaction(|tx: &ReadTx<'_>| -> Result<Vec<Value>, AwsError> {
            let mut out = Vec::with_capacity(gets.len());
            let mut response_bytes = 0usize;
            for g in &gets {
                let stored =
                    tx.get_item(&ctx.account_id, &ctx.region, &g.table_name, &g.pk, &g.sk)?;
                let entry = match decode_existing(stored)? {
                    None => json!({}),
                    Some(item) => json!({ "Item": item_to_json(&item) }),
                };
                response_bytes += estimate_value_bytes(&entry);
                if response_bytes > TRANSACT_GET_MAX_RESPONSE_BYTES {
                    return Err(AwsError::validation(format!(
                        "TransactGetItems response exceeds the {TRANSACT_GET_MAX_RESPONSE_BYTES}-byte cap"
                    )));
                }
                out.push(entry);
            }
            Ok(out)
        })?;

    Ok(json!({ "Responses": responses }))
}

pub fn transact_write_items(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let transact_items = input
        .get("TransactItems")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AwsError::validation("TransactItems is required"))?;

    // Translate each transact-item into a fully-resolved Action up front.
    // We do schema-dependent key extraction here while the in-memory
    // schema cache is in scope; the sqlite txn body just runs the actions.
    // Boxed gsi keeps the Put variant from dwarfing the others — the
    // 5-slot array would otherwise pad every Action to ~500 bytes.
    enum Action {
        Put {
            pk: String,
            sk: String,
            attrs: Value,
            gsi: Box<[(Option<String>, Option<String>); MAX_GSI_SLOTS]>,
            condition_expr: Option<String>,
            expr_attr_names: std::collections::HashMap<String, String>,
            expr_attr_values: serde_json::Map<String, Value>,
        },
        Delete {
            pk: String,
            sk: String,
            condition_expr: Option<String>,
            expr_attr_names: std::collections::HashMap<String, String>,
            expr_attr_values: serde_json::Map<String, Value>,
        },
        Update {
            pk: String,
            sk: String,
            update_expr: String,
            condition_expr: Option<String>,
            expr_attr_names: std::collections::HashMap<String, String>,
            expr_attr_values: serde_json::Map<String, Value>,
            key: DynamoItem,
        },
        ConditionCheck {
            pk: String,
            sk: String,
            condition_expr: Option<String>,
            expr_attr_names: std::collections::HashMap<String, String>,
            expr_attr_values: serde_json::Map<String, Value>,
        },
    }
    struct Mutation {
        table_name: String,
        action: Action,
    }
    let mut mutations: Vec<Mutation> = Vec::new();

    for tx_item in transact_items {
        if let Some(put) = tx_item.get("Put") {
            let table_name = put
                .get("TableName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AwsError::validation("TableName required in Put"))?
                .to_string();
            let item = parse_item(&put["Item"])
                .ok_or_else(|| AwsError::validation("Item is required in Put"))?;
            let sqlite_keys = {
                let table = state.tables.get(&table_name).ok_or_else(|| {
                    AwsError::service_not_found(
                        "ResourceNotFoundException",
                        format!("Table not found: {table_name}"),
                    )
                })?;
                extract_item_keys(&table, &item)
                    .ok_or_else(|| AwsError::validation("Could not construct key"))?
            };
            mutations.push(Mutation {
                table_name,
                action: Action::Put {
                    pk: sqlite_keys.pk,
                    sk: sqlite_keys.sk,
                    attrs: item_to_storage_value(&item),
                    gsi: Box::new(sqlite_keys.gsi),
                    condition_expr: opt_str(put, "ConditionExpression").map(str::to_string),
                    expr_attr_names: get_expr_attr_names(put),
                    expr_attr_values: get_expr_attr_values(put),
                },
            });
        } else if let Some(delete) = tx_item.get("Delete") {
            let table_name = delete
                .get("TableName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AwsError::validation("TableName required in Delete"))?
                .to_string();
            let key = parse_item(&delete["Key"])
                .ok_or_else(|| AwsError::validation("Key is required in Delete"))?;
            let (pk, sk) = {
                let table = state.tables.get(&table_name).ok_or_else(|| {
                    AwsError::service_not_found(
                        "ResourceNotFoundException",
                        format!("Table not found: {table_name}"),
                    )
                })?;
                extract_pk_sk(&table, &key)
                    .ok_or_else(|| AwsError::validation("Could not construct key"))?
            };
            mutations.push(Mutation {
                table_name,
                action: Action::Delete {
                    pk,
                    sk,
                    condition_expr: opt_str(delete, "ConditionExpression").map(str::to_string),
                    expr_attr_names: get_expr_attr_names(delete),
                    expr_attr_values: get_expr_attr_values(delete),
                },
            });
        } else if let Some(update) = tx_item.get("Update") {
            let table_name = update
                .get("TableName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AwsError::validation("TableName required in Update"))?
                .to_string();
            let key = parse_item(&update["Key"])
                .ok_or_else(|| AwsError::validation("Key is required in Update"))?;
            let update_expr = opt_str(update, "UpdateExpression")
                .ok_or_else(|| AwsError::validation("UpdateExpression required in Update"))?
                .to_string();
            let (pk, sk) = {
                let table = state.tables.get(&table_name).ok_or_else(|| {
                    AwsError::service_not_found(
                        "ResourceNotFoundException",
                        format!("Table not found: {table_name}"),
                    )
                })?;
                extract_pk_sk(&table, &key)
                    .ok_or_else(|| AwsError::validation("Could not construct key"))?
            };
            mutations.push(Mutation {
                table_name,
                action: Action::Update {
                    pk,
                    sk,
                    update_expr,
                    condition_expr: opt_str(update, "ConditionExpression").map(str::to_string),
                    expr_attr_names: get_expr_attr_names(update),
                    expr_attr_values: get_expr_attr_values(update),
                    key,
                },
            });
        } else if let Some(condition_check) = tx_item.get("ConditionCheck") {
            let table_name = condition_check
                .get("TableName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AwsError::validation("TableName required in ConditionCheck"))?
                .to_string();
            let key = parse_item(&condition_check["Key"])
                .ok_or_else(|| AwsError::validation("Key is required in ConditionCheck"))?;
            let (pk, sk) = {
                let table = state.tables.get(&table_name).ok_or_else(|| {
                    AwsError::service_not_found(
                        "ResourceNotFoundException",
                        format!("Table not found: {table_name}"),
                    )
                })?;
                extract_pk_sk(&table, &key)
                    .ok_or_else(|| AwsError::validation("Could not construct key"))?
            };
            mutations.push(Mutation {
                table_name,
                action: Action::ConditionCheck {
                    pk,
                    sk,
                    condition_expr: opt_str(condition_check, "ConditionExpression")
                        .map(str::to_string),
                    expr_attr_names: get_expr_attr_names(condition_check),
                    expr_attr_values: get_expr_attr_values(condition_check),
                },
            });
        }
    }

    // Snapshot the schema cache up front so the txn body — which can't
    // reach back into the dashmap (we'd block other writers) — has every
    // GSI key schema it needs.
    use std::collections::HashMap;
    let mut schema_cache: HashMap<String, crate::state::Table> = HashMap::new();
    for m in &mutations {
        if !schema_cache.contains_key(&m.table_name) {
            let table = state.tables.get(&m.table_name).ok_or_else(|| {
                AwsError::service_not_found(
                    "ResourceNotFoundException",
                    format!("Table not found: {}", m.table_name),
                )
            })?;
            // Clone just the parts we need (key schema + GSIs). Easier to
            // keep the full schema clone since it's only run on the
            // transact path which is rare.
            schema_cache.insert(
                m.table_name.clone(),
                crate::state::Table {
                    name: table.name.clone(),
                    arn: table.arn.clone(),
                    key_schema: table.key_schema.clone(),
                    attribute_definitions: table.attribute_definitions.clone(),
                    billing_mode: table.billing_mode.clone(),
                    status: table.status.clone(),
                    created_at: table.created_at,
                    gsi: table.gsi.clone(),
                    lsi: table.lsi.clone(),
                    stream_enabled: table.stream_enabled,
                    stream_arn: table.stream_arn.clone(),
                    stream_view_type: table.stream_view_type.clone(),
                    stream_records: Vec::new(),
                    stream_sequence: 0,
                    ttl: table.ttl.clone(),
                    tags: table.tags.clone(),
                },
            );
        }
    }

    let mutation_count = mutations.len();

    // Run the entire validation + mutation sequence inside one sqlite
    // write transaction. If any condition fails or any sqlite call
    // errors, the txn auto-rolls back on Drop and no changes leak.
    sqlite.with_write_transaction(|tx: &WriteTx<'_>| -> Result<(), AwsError> {
        for (idx, mutation) in mutations.iter().enumerate() {
            match &mutation.action {
                Action::Put {
                    pk,
                    sk,
                    attrs,
                    gsi,
                    condition_expr,
                    expr_attr_names,
                    expr_attr_values,
                } => {
                    if let Some(cond_expr) = condition_expr {
                        let condition = parse_condition(cond_expr)?;
                        let existing = decode_existing(tx.get_item(
                            &ctx.account_id,
                            &ctx.region,
                            &mutation.table_name,
                            pk,
                            sk,
                        )?)?
                        .unwrap_or_default();
                        if !evaluate_condition(
                            &condition,
                            &existing,
                            expr_attr_names,
                            expr_attr_values,
                        )? {
                            return Err(transaction_canceled(
                                mutation_count,
                                idx,
                                "ConditionalCheckFailed",
                            ));
                        }
                    }
                    tx.put_item(
                        &ctx.account_id,
                        &ctx.region,
                        &mutation.table_name,
                        pk,
                        sk,
                        attrs,
                        gsi,
                    )?;
                }
                Action::Delete {
                    pk,
                    sk,
                    condition_expr,
                    expr_attr_names,
                    expr_attr_values,
                } => {
                    if let Some(cond_expr) = condition_expr {
                        let condition = parse_condition(cond_expr)?;
                        let existing = decode_existing(tx.get_item(
                            &ctx.account_id,
                            &ctx.region,
                            &mutation.table_name,
                            pk,
                            sk,
                        )?)?
                        .unwrap_or_default();
                        if !evaluate_condition(
                            &condition,
                            &existing,
                            expr_attr_names,
                            expr_attr_values,
                        )? {
                            return Err(transaction_canceled(
                                mutation_count,
                                idx,
                                "ConditionalCheckFailed",
                            ));
                        }
                    }
                    tx.delete_item(&ctx.account_id, &ctx.region, &mutation.table_name, pk, sk)?;
                }
                Action::Update {
                    pk,
                    sk,
                    update_expr,
                    condition_expr,
                    expr_attr_names,
                    expr_attr_values,
                    key,
                } => {
                    let existing = decode_existing(tx.get_item(
                        &ctx.account_id,
                        &ctx.region,
                        &mutation.table_name,
                        pk,
                        sk,
                    )?)?;

                    if let Some(cond_expr) = condition_expr {
                        let condition = parse_condition(cond_expr)?;
                        let empty: DynamoItem = DynamoItem::new();
                        let check = existing.as_ref().unwrap_or(&empty);
                        if !evaluate_condition(
                            &condition,
                            check,
                            expr_attr_names,
                            expr_attr_values,
                        )? {
                            return Err(transaction_canceled(
                                mutation_count,
                                idx,
                                "ConditionalCheckFailed",
                            ));
                        }
                    }

                    let mut item = existing.unwrap_or_else(|| key.clone());
                    apply_update_expression(
                        &mut item,
                        update_expr,
                        expr_attr_names,
                        expr_attr_values,
                    )?;
                    for (k, v) in key {
                        item.insert(k.clone(), v.clone());
                    }
                    let table = schema_cache
                        .get(&mutation.table_name)
                        .ok_or_else(|| AwsError::internal("missing schema cache entry"))?;
                    let sqlite_keys = extract_item_keys(table, &item)
                        .ok_or_else(|| AwsError::validation("Could not extract SQLite keys"))?;
                    let attrs = item_to_storage_value(&item);
                    tx.put_item(
                        &ctx.account_id,
                        &ctx.region,
                        &mutation.table_name,
                        &sqlite_keys.pk,
                        &sqlite_keys.sk,
                        &attrs,
                        &sqlite_keys.gsi,
                    )?;
                }
                Action::ConditionCheck {
                    pk,
                    sk,
                    condition_expr,
                    expr_attr_names,
                    expr_attr_values,
                } => {
                    if let Some(cond_expr) = condition_expr {
                        let condition = parse_condition(cond_expr)?;
                        let existing = decode_existing(tx.get_item(
                            &ctx.account_id,
                            &ctx.region,
                            &mutation.table_name,
                            pk,
                            sk,
                        )?)?
                        .unwrap_or_default();
                        if !evaluate_condition(
                            &condition,
                            &existing,
                            expr_attr_names,
                            expr_attr_values,
                        )? {
                            return Err(transaction_canceled(
                                mutation_count,
                                idx,
                                "ConditionalCheckFailed",
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    })?;

    Ok(json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{KeySchemaElement, Table};

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
            stream_records: Vec::new(),
            stream_sequence: 0,
            ttl: Default::default(),
            tags: Default::default(),
        };
        state.tables.insert("t".into(), table);
        state
    }

    #[test]
    fn write_items_rolls_back_on_failed_condition() {
        let state = make_state_with_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let ctx = ctx();

        // Seed one row that the second operation's condition will fail on.
        sqlite
            .put_item(
                &ctx.account_id,
                &ctx.region,
                "t",
                "p1",
                "s1",
                &json!({"pk": {"S": "p1"}, "sk": {"S": "s1"}, "v": {"N": "0"}}),
                &Default::default(),
            )
            .unwrap();

        // Transaction: Put a NEW row p2/s1 + Update p1/s1 with a
        // condition that will FAIL (attribute_not_exists(pk) — but p1
        // does exist). Expectation: neither write commits.
        let input = json!({
            "TransactItems": [
                {
                    "Put": {
                        "TableName": "t",
                        "Item": {"pk": {"S": "p2"}, "sk": {"S": "s1"}, "v": {"N": "9"}},
                    }
                },
                {
                    "Update": {
                        "TableName": "t",
                        "Key": {"pk": {"S": "p1"}, "sk": {"S": "s1"}},
                        "UpdateExpression": "SET v = :nv",
                        "ConditionExpression": "attribute_not_exists(pk)",
                        "ExpressionAttributeValues": {":nv": {"N": "99"}},
                    }
                }
            ]
        });
        let res = transact_write_items(&state, &sqlite, &input, &ctx);
        assert!(res.is_err(), "expected TransactionCanceledException");

        // p2 must NOT have been inserted, p1 must still hold v=0.
        let p2 = sqlite
            .get_item(&ctx.account_id, &ctx.region, "t", "p2", "s1")
            .unwrap();
        assert!(p2.is_none(), "rollback failed: p2 leaked");
        let p1 = sqlite
            .get_item(&ctx.account_id, &ctx.region, "t", "p1", "s1")
            .unwrap()
            .unwrap();
        assert_eq!(
            p1["v"],
            json!({"N": "0"}),
            "rollback failed: p1 was mutated"
        );
    }

    #[test]
    fn cancellation_reasons_array_marks_failed_index() {
        let state = make_state_with_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let ctx = ctx();

        sqlite
            .put_item(
                &ctx.account_id,
                &ctx.region,
                "t",
                "p1",
                "s1",
                &json!({"pk": {"S": "p1"}, "sk": {"S": "s1"}}),
                &Default::default(),
            )
            .unwrap();

        // First Put OK, second Put fails its condition. The error should
        // expose CancellationReasons with [None, ConditionalCheckFailed].
        let input = json!({
            "TransactItems": [
                {
                    "Put": {
                        "TableName": "t",
                        "Item": {"pk": {"S": "fresh"}, "sk": {"S": "1"}},
                    }
                },
                {
                    "Put": {
                        "TableName": "t",
                        "Item": {"pk": {"S": "p1"}, "sk": {"S": "s1"}},
                        "ConditionExpression": "attribute_not_exists(pk)",
                    }
                }
            ]
        });
        let err = transact_write_items(&state, &sqlite, &input, &ctx).unwrap_err();
        assert_eq!(err.code, "TransactionCanceledException");
        assert_eq!(err.status.as_u16(), 400);

        let extras = err.extras.as_ref().expect("extras populated");
        let reasons = extras
            .get("CancellationReasons")
            .and_then(|v| v.as_array())
            .expect("CancellationReasons array");
        assert_eq!(reasons.len(), 2);
        assert_eq!(reasons[0]["Code"], json!("None"));
        assert_eq!(reasons[1]["Code"], json!("ConditionalCheckFailed"));
        assert!(reasons[1].get("Message").is_some());
    }

    #[test]
    fn write_items_commits_when_all_conditions_pass() {
        let state = make_state_with_table();
        let sqlite = SqliteStore::in_memory().unwrap();
        let ctx = ctx();

        let input = json!({
            "TransactItems": [
                {"Put": {"TableName": "t", "Item": {"pk": {"S": "a"}, "sk": {"S": "1"}}}},
                {"Put": {"TableName": "t", "Item": {"pk": {"S": "b"}, "sk": {"S": "2"}}}},
            ]
        });
        transact_write_items(&state, &sqlite, &input, &ctx).unwrap();

        assert_eq!(
            sqlite
                .count_items(&ctx.account_id, &ctx.region, "t")
                .unwrap(),
            2
        );
    }
}
