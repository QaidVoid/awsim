use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    expressions::{apply_update_expression, evaluate_condition, parse_condition},
    state::{DynamoItem, DynamoState},
};

use super::{
    get_expr_attr_names, get_expr_attr_values,
    item::{item_to_json, parse_item},
    opt_str,
};

pub fn transact_get_items(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
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

        let table = state.tables.get(table_name).ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Cannot do operations on a non-existent table: {table_name}"),
            )
        })?;

        let key = parse_item(&get["Key"])
            .ok_or_else(|| AwsError::validation("Key is required in Get"))?;

        let composite_key = table
            .composite_key(&key)
            .ok_or_else(|| AwsError::validation("Could not construct item key"))?;

        match table.items.get(&composite_key) {
            None => responses.push(json!({})),
            Some(item) => {
                responses.push(json!({ "Item": item_to_json(item) }));
            }
        }
    }

    Ok(json!({ "Responses": responses }))
}

pub fn transact_write_items(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let transact_items = input
        .get("TransactItems")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AwsError::validation("TransactItems is required"))?;

    // Phase 1: Validate all conditions before making any changes
    // We collect all mutations to apply atomically.
    struct Mutation {
        table_name: String,
        composite_key: String,
        kind: MutationKind,
    }

    enum MutationKind {
        Put(DynamoItem),
        Delete,
        Update {
            update_expr: String,
            expr_attr_names: std::collections::HashMap<String, String>,
            expr_attr_values: serde_json::Map<String, Value>,
        },
        ConditionCheck, // No-op mutation; just validates
    }

    let mut mutations: Vec<Mutation> = Vec::new();

    for tx_item in transact_items {
        if let Some(put) = tx_item.get("Put") {
            let table_name = put
                .get("TableName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AwsError::validation("TableName required in Put"))?
                .to_string();

            let table = state.tables.get(&table_name).ok_or_else(|| {
                AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Table not found: {table_name}"),
                )
            })?;

            let item = parse_item(&put["Item"])
                .ok_or_else(|| AwsError::validation("Item is required in Put"))?;

            let expr_attr_names = get_expr_attr_names(put);
            let expr_attr_values = get_expr_attr_values(put);

            // Check condition
            if let Some(cond_expr) = opt_str(put, "ConditionExpression") {
                let condition = parse_condition(cond_expr)?;
                let composite_key = table
                    .composite_key(&item)
                    .ok_or_else(|| AwsError::validation("Could not construct key"))?;
                let empty: DynamoItem = DynamoItem::new();
                let existing = table.items.get(&composite_key).unwrap_or(&empty);
                if !evaluate_condition(&condition, existing, &expr_attr_names, &expr_attr_values)? {
                    return Err(AwsError::conflict(
                        "TransactionCanceledException",
                        "Transaction cancelled, please refer cancellation reasons for specific reasons [ConditionalCheckFailed]",
                    ));
                }
            }

            let composite_key = table
                .composite_key(&item)
                .ok_or_else(|| AwsError::validation("Could not construct key"))?;

            mutations.push(Mutation {
                table_name,
                composite_key,
                kind: MutationKind::Put(item),
            });
        } else if let Some(delete) = tx_item.get("Delete") {
            let table_name = delete
                .get("TableName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AwsError::validation("TableName required in Delete"))?
                .to_string();

            let table = state.tables.get(&table_name).ok_or_else(|| {
                AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Table not found: {table_name}"),
                )
            })?;

            let key = parse_item(&delete["Key"])
                .ok_or_else(|| AwsError::validation("Key is required in Delete"))?;

            let expr_attr_names = get_expr_attr_names(delete);
            let expr_attr_values = get_expr_attr_values(delete);

            let composite_key = table
                .composite_key(&key)
                .ok_or_else(|| AwsError::validation("Could not construct key"))?;

            // Check condition
            if let Some(cond_expr) = opt_str(delete, "ConditionExpression") {
                let condition = parse_condition(cond_expr)?;
                let empty: DynamoItem = DynamoItem::new();
                let existing = table.items.get(&composite_key).unwrap_or(&empty);
                if !evaluate_condition(&condition, existing, &expr_attr_names, &expr_attr_values)? {
                    return Err(AwsError::conflict(
                        "TransactionCanceledException",
                        "Transaction cancelled, please refer cancellation reasons for specific reasons [ConditionalCheckFailed]",
                    ));
                }
            }

            mutations.push(Mutation {
                table_name,
                composite_key,
                kind: MutationKind::Delete,
            });
        } else if let Some(update) = tx_item.get("Update") {
            let table_name = update
                .get("TableName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AwsError::validation("TableName required in Update"))?
                .to_string();

            let table = state.tables.get(&table_name).ok_or_else(|| {
                AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Table not found: {table_name}"),
                )
            })?;

            let key = parse_item(&update["Key"])
                .ok_or_else(|| AwsError::validation("Key is required in Update"))?;

            let expr_attr_names = get_expr_attr_names(update);
            let expr_attr_values = get_expr_attr_values(update);
            let update_expr = opt_str(update, "UpdateExpression")
                .ok_or_else(|| AwsError::validation("UpdateExpression required in Update"))?
                .to_string();

            let composite_key = table
                .composite_key(&key)
                .ok_or_else(|| AwsError::validation("Could not construct key"))?;

            // Check condition
            if let Some(cond_expr) = opt_str(update, "ConditionExpression") {
                let condition = parse_condition(cond_expr)?;
                let empty: DynamoItem = DynamoItem::new();
                let existing = table.items.get(&composite_key).unwrap_or(&empty);
                if !evaluate_condition(&condition, existing, &expr_attr_names, &expr_attr_values)? {
                    return Err(AwsError::conflict(
                        "TransactionCanceledException",
                        "Transaction cancelled, please refer cancellation reasons for specific reasons [ConditionalCheckFailed]",
                    ));
                }
            }

            mutations.push(Mutation {
                table_name,
                composite_key,
                kind: MutationKind::Update {
                    update_expr,
                    expr_attr_names,
                    expr_attr_values,
                },
            });
        } else if let Some(condition_check) = tx_item.get("ConditionCheck") {
            let table_name = condition_check
                .get("TableName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AwsError::validation("TableName required in ConditionCheck"))?
                .to_string();

            let table = state.tables.get(&table_name).ok_or_else(|| {
                AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Table not found: {table_name}"),
                )
            })?;

            let key = parse_item(&condition_check["Key"])
                .ok_or_else(|| AwsError::validation("Key is required in ConditionCheck"))?;

            let expr_attr_names = get_expr_attr_names(condition_check);
            let expr_attr_values = get_expr_attr_values(condition_check);

            let composite_key = table
                .composite_key(&key)
                .ok_or_else(|| AwsError::validation("Could not construct key"))?;

            if let Some(cond_expr) = opt_str(condition_check, "ConditionExpression") {
                let condition = parse_condition(cond_expr)?;
                let empty: DynamoItem = DynamoItem::new();
                let existing = table.items.get(&composite_key).unwrap_or(&empty);
                if !evaluate_condition(&condition, existing, &expr_attr_names, &expr_attr_values)? {
                    return Err(AwsError::conflict(
                        "TransactionCanceledException",
                        "Transaction cancelled, please refer cancellation reasons for specific reasons [ConditionalCheckFailed]",
                    ));
                }
            }

            mutations.push(Mutation {
                table_name,
                composite_key,
                kind: MutationKind::ConditionCheck,
            });
        }
    }

    // Phase 2: Apply all mutations
    for mutation in mutations {
        match state.tables.get_mut(&mutation.table_name) {
            None => continue,
            Some(mut table) => match mutation.kind {
                MutationKind::Put(item) => {
                    table.items.insert(mutation.composite_key, item);
                }
                MutationKind::Delete => {
                    table.items.remove(&mutation.composite_key);
                }
                MutationKind::Update {
                    update_expr,
                    expr_attr_names,
                    expr_attr_values,
                } => {
                    let mut item = table
                        .items
                        .get(&mutation.composite_key)
                        .cloned()
                        .unwrap_or_default();
                    apply_update_expression(
                        &mut item,
                        &update_expr,
                        &expr_attr_names,
                        &expr_attr_values,
                    )?;
                    table.items.insert(mutation.composite_key, item);
                }
                MutationKind::ConditionCheck => {
                    // No mutation needed
                }
            },
        }
    }

    Ok(json!({}))
}
