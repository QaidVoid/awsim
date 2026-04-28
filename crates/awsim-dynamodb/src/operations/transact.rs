use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    expressions::{apply_update_expression, evaluate_condition, parse_condition},
    keys::{extract_item_keys, extract_pk_sk, item_to_storage_value, storage_value_to_item},
    sqlite_store::{MAX_GSI_SLOTS, SqliteStore},
    state::{DynamoItem, DynamoState},
};

use super::{
    get_expr_attr_names, get_expr_attr_values,
    item::{item_to_json, parse_item},
    opt_str,
};

/// Convenience: load `(pk, sk)` for a key map and read the existing
/// item from SQLite. `None` means the row doesn't exist.
fn fetch_existing(
    sqlite: &SqliteStore,
    ctx: &RequestContext,
    table_name: &str,
    pk: &str,
    sk: &str,
) -> Result<Option<DynamoItem>, AwsError> {
    sqlite
        .get_item(&ctx.account_id, &ctx.region, table_name, pk, sk)?
        .map(|stored| {
            storage_value_to_item(stored)
                .ok_or_else(|| AwsError::internal("DynamoDB stored attrs is not an object"))
        })
        .transpose()
}

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

    let mut responses: Vec<Value> = Vec::new();

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
                AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Cannot do operations on a non-existent table: {table_name}"),
                )
            })?;
            extract_pk_sk(&table, &key)
                .ok_or_else(|| AwsError::validation("Could not construct item key"))?
        };

        match fetch_existing(sqlite, ctx, table_name, &pk, &sk)? {
            None => responses.push(json!({})),
            Some(item) => responses.push(json!({ "Item": item_to_json(&item) })),
        }
    }

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

    // Phase 1: validate every condition. We collect resolved storage
    // keys + parsed action descriptors so phase 2 doesn't need to
    // re-touch the in-memory schema cache.
    enum Action {
        Put {
            pk: String,
            sk: String,
            attrs: Value,
            gsi: [(Option<String>, Option<String>); MAX_GSI_SLOTS],
        },
        Delete {
            pk: String,
            sk: String,
        },
        Update {
            pk: String,
            sk: String,
            update_expr: String,
            expr_attr_names: std::collections::HashMap<String, String>,
            expr_attr_values: serde_json::Map<String, Value>,
            key: DynamoItem,
        },
        ConditionCheck,
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
            let expr_attr_names = get_expr_attr_names(put);
            let expr_attr_values = get_expr_attr_values(put);

            let sqlite_keys = {
                let table = state.tables.get(&table_name).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Table not found: {table_name}"),
                    )
                })?;
                extract_item_keys(&table, &item)
                    .ok_or_else(|| AwsError::validation("Could not construct key"))?
            };

            if let Some(cond_expr) = opt_str(put, "ConditionExpression") {
                let condition = parse_condition(cond_expr)?;
                let existing =
                    fetch_existing(sqlite, ctx, &table_name, &sqlite_keys.pk, &sqlite_keys.sk)?
                        .unwrap_or_default();
                if !evaluate_condition(&condition, &existing, &expr_attr_names, &expr_attr_values)?
                {
                    return Err(AwsError::conflict(
                        "TransactionCanceledException",
                        "Transaction cancelled, please refer cancellation reasons for specific reasons [ConditionalCheckFailed]",
                    ));
                }
            }

            mutations.push(Mutation {
                table_name,
                action: Action::Put {
                    pk: sqlite_keys.pk,
                    sk: sqlite_keys.sk,
                    attrs: item_to_storage_value(&item),
                    gsi: sqlite_keys.gsi,
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
            let expr_attr_names = get_expr_attr_names(delete);
            let expr_attr_values = get_expr_attr_values(delete);

            let (pk, sk) = {
                let table = state.tables.get(&table_name).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Table not found: {table_name}"),
                    )
                })?;
                extract_pk_sk(&table, &key)
                    .ok_or_else(|| AwsError::validation("Could not construct key"))?
            };

            if let Some(cond_expr) = opt_str(delete, "ConditionExpression") {
                let condition = parse_condition(cond_expr)?;
                let existing = fetch_existing(sqlite, ctx, &table_name, &pk, &sk)?
                    .unwrap_or_default();
                if !evaluate_condition(&condition, &existing, &expr_attr_names, &expr_attr_values)?
                {
                    return Err(AwsError::conflict(
                        "TransactionCanceledException",
                        "Transaction cancelled, please refer cancellation reasons for specific reasons [ConditionalCheckFailed]",
                    ));
                }
            }

            mutations.push(Mutation {
                table_name,
                action: Action::Delete { pk, sk },
            });
        } else if let Some(update) = tx_item.get("Update") {
            let table_name = update
                .get("TableName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AwsError::validation("TableName required in Update"))?
                .to_string();

            let key = parse_item(&update["Key"])
                .ok_or_else(|| AwsError::validation("Key is required in Update"))?;
            let expr_attr_names = get_expr_attr_names(update);
            let expr_attr_values = get_expr_attr_values(update);
            let update_expr = opt_str(update, "UpdateExpression")
                .ok_or_else(|| AwsError::validation("UpdateExpression required in Update"))?
                .to_string();

            let (pk, sk) = {
                let table = state.tables.get(&table_name).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Table not found: {table_name}"),
                    )
                })?;
                extract_pk_sk(&table, &key)
                    .ok_or_else(|| AwsError::validation("Could not construct key"))?
            };

            if let Some(cond_expr) = opt_str(update, "ConditionExpression") {
                let condition = parse_condition(cond_expr)?;
                let existing = fetch_existing(sqlite, ctx, &table_name, &pk, &sk)?
                    .unwrap_or_default();
                if !evaluate_condition(&condition, &existing, &expr_attr_names, &expr_attr_values)?
                {
                    return Err(AwsError::conflict(
                        "TransactionCanceledException",
                        "Transaction cancelled, please refer cancellation reasons for specific reasons [ConditionalCheckFailed]",
                    ));
                }
            }

            mutations.push(Mutation {
                table_name,
                action: Action::Update {
                    pk,
                    sk,
                    update_expr,
                    expr_attr_names,
                    expr_attr_values,
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
            let expr_attr_names = get_expr_attr_names(condition_check);
            let expr_attr_values = get_expr_attr_values(condition_check);

            let (pk, sk) = {
                let table = state.tables.get(&table_name).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Table not found: {table_name}"),
                    )
                })?;
                extract_pk_sk(&table, &key)
                    .ok_or_else(|| AwsError::validation("Could not construct key"))?
            };

            if let Some(cond_expr) = opt_str(condition_check, "ConditionExpression") {
                let condition = parse_condition(cond_expr)?;
                let existing = fetch_existing(sqlite, ctx, &table_name, &pk, &sk)?
                    .unwrap_or_default();
                if !evaluate_condition(&condition, &existing, &expr_attr_names, &expr_attr_values)?
                {
                    return Err(AwsError::conflict(
                        "TransactionCanceledException",
                        "Transaction cancelled, please refer cancellation reasons for specific reasons [ConditionalCheckFailed]",
                    ));
                }
            }

            mutations.push(Mutation {
                table_name,
                action: Action::ConditionCheck,
            });
        }
    }

    // Phase 2: apply each mutation. This isn't truly atomic against the
    // SQLite layer yet — stage 5 wraps it in a single sqlite transaction.
    for mutation in mutations {
        match mutation.action {
            Action::Put {
                pk,
                sk,
                attrs,
                gsi,
            } => {
                sqlite.put_item(
                    &ctx.account_id,
                    &ctx.region,
                    &mutation.table_name,
                    &pk,
                    &sk,
                    &attrs,
                    &gsi,
                )?;
            }
            Action::Delete { pk, sk } => {
                sqlite.delete_item(
                    &ctx.account_id,
                    &ctx.region,
                    &mutation.table_name,
                    &pk,
                    &sk,
                )?;
            }
            Action::Update {
                pk,
                sk,
                update_expr,
                expr_attr_names,
                expr_attr_values,
                key,
            } => {
                let mut item = fetch_existing(sqlite, ctx, &mutation.table_name, &pk, &sk)?
                    .unwrap_or_else(|| key.clone());
                apply_update_expression(
                    &mut item,
                    &update_expr,
                    &expr_attr_names,
                    &expr_attr_values,
                )?;
                for (k, v) in &key {
                    item.insert(k.clone(), v.clone());
                }
                let sqlite_keys = {
                    let table = state.tables.get(&mutation.table_name).ok_or_else(|| {
                        AwsError::not_found(
                            "ResourceNotFoundException",
                            format!("Table not found: {}", mutation.table_name),
                        )
                    })?;
                    extract_item_keys(&table, &item)
                        .ok_or_else(|| AwsError::validation("Could not extract SQLite keys"))?
                };
                let attrs = item_to_storage_value(&item);
                sqlite.put_item(
                    &ctx.account_id,
                    &ctx.region,
                    &mutation.table_name,
                    &sqlite_keys.pk,
                    &sqlite_keys.sk,
                    &attrs,
                    &sqlite_keys.gsi,
                )?;
            }
            Action::ConditionCheck => {
                // No mutation needed — the validation ran in phase 1.
            }
        }
    }

    Ok(json!({}))
}
