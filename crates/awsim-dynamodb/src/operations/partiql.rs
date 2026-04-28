/// Simplified PartiQL execution for the DynamoDB dev emulator.
///
/// Supports a small subset of PartiQL that tooling commonly generates:
///   SELECT * FROM "TableName"
///   SELECT * FROM "TableName" WHERE "pk" = 'value'
///   INSERT INTO "TableName" VALUE {'pk': 'val', ...}
///   UPDATE "TableName" SET attr = val WHERE "pk" = 'val'
///   DELETE FROM "TableName" WHERE "pk" = 'val'
///
/// Parameters (`?`) are substituted from the `Parameters` list in order.
///
/// Stage 4: backed by SqliteStore — items live only in SQLite.
use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    keys::{extract_item_keys, item_to_storage_value, storage_value_to_item},
    sqlite_store::SqliteStore,
    state::{DynamoItem, DynamoState},
};

// ─── Public entry points ──────────────────────────────────────────────────────

pub fn execute_statement(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stmt = input
        .get("Statement")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Statement is required"))?;

    let params = input.get("Parameters").cloned().unwrap_or(json!([]));

    let items = run_statement(state, sqlite, ctx, stmt, &params)?;

    Ok(json!({
        "Items": items,
        "NextToken": null
    }))
}

pub fn batch_execute_statement(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stmts = input
        .get("Statements")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Statements is required"))?
        .clone();

    let mut responses = Vec::new();

    for stmt_obj in &stmts {
        let stmt = stmt_obj
            .get("Statement")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let params = stmt_obj.get("Parameters").cloned().unwrap_or(json!([]));

        match run_statement(state, sqlite, ctx, stmt, &params) {
            Ok(items) => {
                let first = items.into_iter().next().unwrap_or(json!(null));
                responses.push(json!({ "Item": first }));
            }
            Err(e) => {
                responses.push(json!({
                    "Error": {
                        "Code": e.code,
                        "Message": e.message
                    }
                }));
            }
        }
    }

    Ok(json!({ "Responses": responses }))
}

pub fn execute_transaction(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stmts = input
        .get("TransactStatements")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            AwsError::bad_request("ValidationException", "TransactStatements is required")
        })?
        .clone();

    let mut responses = Vec::new();

    for stmt_obj in &stmts {
        let stmt = stmt_obj
            .get("Statement")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let params = stmt_obj.get("Parameters").cloned().unwrap_or(json!([]));

        match run_statement(state, sqlite, ctx, stmt, &params) {
            Ok(items) => {
                let first = items.into_iter().next().unwrap_or(json!(null));
                responses.push(json!({ "Item": first }));
            }
            Err(e) => {
                // Transactional failures bubble up as a whole-transaction error.
                return Err(e);
            }
        }
    }

    Ok(json!({ "Responses": responses }))
}

// ─── Core statement runner ────────────────────────────────────────────────────

fn run_statement(
    state: &DynamoState,
    sqlite: &SqliteStore,
    ctx: &RequestContext,
    raw_stmt: &str,
    params: &Value,
) -> Result<Vec<Value>, AwsError> {
    let stmt = raw_stmt.trim();
    let upper = stmt.to_uppercase();

    if upper.starts_with("SELECT") {
        run_select(state, sqlite, ctx, stmt, params)
    } else if upper.starts_with("INSERT") {
        run_insert(state, sqlite, ctx, stmt, params)?;
        Ok(vec![])
    } else if upper.starts_with("UPDATE") {
        run_update(state, sqlite, ctx, stmt, params)?;
        Ok(vec![])
    } else if upper.starts_with("DELETE") {
        run_delete(state, sqlite, ctx, stmt, params)?;
        Ok(vec![])
    } else {
        Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "Unsupported PartiQL statement: {}",
                &stmt[..stmt.len().min(60)]
            ),
        ))
    }
}

/// Decode a stored row into a `DynamoItem` plus a JSON value suitable for
/// the SELECT response shape (just the inner attribute map).
fn decode_row(stored: Value) -> Result<DynamoItem, AwsError> {
    storage_value_to_item(stored)
        .ok_or_else(|| AwsError::internal("DynamoDB stored attrs is not an object"))
}

// ─── SELECT ──────────────────────────────────────────────────────────────────

fn run_select(
    state: &DynamoState,
    sqlite: &SqliteStore,
    ctx: &RequestContext,
    stmt: &str,
    params: &Value,
) -> Result<Vec<Value>, AwsError> {
    let (table_name, where_key, where_val) = parse_from_where(stmt, params)?;

    // Confirm the table exists; we don't need anything else from the
    // schema for this minimal SELECT subset.
    if !state.tables.contains_key(&table_name) {
        return Err(AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Table '{table_name}' not found"),
        ));
    }

    let mut items: Vec<Value> = Vec::new();
    sqlite.scan_table(
        &ctx.account_id,
        &ctx.region,
        &table_name,
        None,
        |_pk, _sk, attrs| {
            let item = decode_row(attrs)?;
            let keep = match (&where_key, &where_val) {
                (Some(key), Some(val)) => item.get(key).map(|attr| attr == val).unwrap_or(false),
                _ => true,
            };
            if keep {
                items.push(json!(item));
            }
            Ok(true)
        },
    )?;
    Ok(items)
}

// ─── INSERT ───────────────────────────────────────────────────────────────────

fn run_insert(
    state: &DynamoState,
    sqlite: &SqliteStore,
    ctx: &RequestContext,
    stmt: &str,
    _params: &Value,
) -> Result<(), AwsError> {
    // INSERT INTO "TableName" VALUE {'key': 'val', ...}
    let upper = stmt.to_uppercase();
    let into_pos = upper.find("INTO").ok_or_else(|| {
        AwsError::bad_request(
            "ValidationException",
            "Invalid INSERT statement: missing INTO",
        )
    })?;
    let after_into = stmt[into_pos + 4..].trim();
    let table_name = extract_quoted_identifier(after_into)?;

    let value_pos = upper.find("VALUE").ok_or_else(|| {
        AwsError::bad_request(
            "ValidationException",
            "Invalid INSERT statement: missing VALUE",
        )
    })?;
    let json_str = stmt[value_pos + 5..].trim();

    let normalized = normalize_partiql_json(json_str);
    let plain_item: Value = serde_json::from_str(&normalized).map_err(|e| {
        AwsError::bad_request("ValidationException", format!("Invalid INSERT VALUE: {e}"))
    })?;

    let ddb_item: DynamoItem = plain_to_ddb_item(&plain_item);

    let sqlite_keys = {
        let table = state.tables.get(&table_name).ok_or_else(|| {
            AwsError::service_not_found(
                "ResourceNotFoundException",
                format!("Table '{table_name}' not found"),
            )
        })?;
        extract_item_keys(&table, &ddb_item).ok_or_else(|| {
            AwsError::bad_request("ValidationException", "Item missing primary key")
        })?
    };

    let attrs = item_to_storage_value(&ddb_item);
    sqlite.put_item(
        &ctx.account_id,
        &ctx.region,
        &table_name,
        &sqlite_keys.pk,
        &sqlite_keys.sk,
        &attrs,
        &sqlite_keys.gsi,
    )?;
    Ok(())
}

// ─── UPDATE ───────────────────────────────────────────────────────────────────

fn run_update(
    state: &DynamoState,
    sqlite: &SqliteStore,
    ctx: &RequestContext,
    stmt: &str,
    params: &Value,
) -> Result<(), AwsError> {
    // UPDATE "TableName" SET attr = val WHERE "pk" = val
    let upper = stmt.to_uppercase();
    let (table_name, where_key, where_val) = parse_from_where(stmt, params)?;

    let set_pos = upper.find(" SET ").ok_or_else(|| {
        AwsError::bad_request("ValidationException", "Invalid UPDATE: missing SET clause")
    })?;

    let set_clause = &stmt[set_pos + 5..];
    let where_upper = set_clause.to_uppercase();
    let set_end = where_upper.find(" WHERE ").unwrap_or(set_clause.len());
    let assignments = set_clause[..set_end].trim();

    if !state.tables.contains_key(&table_name) {
        return Err(AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Table '{table_name}' not found"),
        ));
    }

    let (Some(key), Some(val)) = (where_key, where_val) else {
        return Ok(());
    };

    // Find the first matching row (PartiQL's UPDATE on this subset is
    // single-row by intent, mirroring the prior in-memory behaviour).
    let mut hit: Option<DynamoItem> = None;
    sqlite.scan_table(
        &ctx.account_id,
        &ctx.region,
        &table_name,
        None,
        |_pk, _sk, attrs| {
            let item = decode_row(attrs)?;
            if item.get(&key).map(|v| v == &val).unwrap_or(false) {
                hit = Some(item);
                return Ok(false);
            }
            Ok(true)
        },
    )?;

    let Some(mut item) = hit else {
        return Ok(());
    };
    apply_set_clause(&mut item, assignments, params);

    let sqlite_keys = {
        let table = state.tables.get(&table_name).ok_or_else(|| {
            AwsError::service_not_found(
                "ResourceNotFoundException",
                format!("Table '{table_name}' not found"),
            )
        })?;
        extract_item_keys(&table, &item)
            .ok_or_else(|| AwsError::validation("Could not extract SQLite keys"))?
    };

    let attrs = item_to_storage_value(&item);
    sqlite.put_item(
        &ctx.account_id,
        &ctx.region,
        &table_name,
        &sqlite_keys.pk,
        &sqlite_keys.sk,
        &attrs,
        &sqlite_keys.gsi,
    )?;
    Ok(())
}

// ─── DELETE ───────────────────────────────────────────────────────────────────

fn run_delete(
    state: &DynamoState,
    sqlite: &SqliteStore,
    ctx: &RequestContext,
    stmt: &str,
    params: &Value,
) -> Result<(), AwsError> {
    let (table_name, where_key, where_val) = parse_from_where(stmt, params)?;

    if !state.tables.contains_key(&table_name) {
        return Err(AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Table '{table_name}' not found"),
        ));
    }

    // Collect matching (pk, sk) pairs first, then delete — keeps the
    // sqlite read iterator from being invalidated by concurrent writes.
    let mut targets: Vec<(String, String)> = Vec::new();
    sqlite.scan_table(
        &ctx.account_id,
        &ctx.region,
        &table_name,
        None,
        |pk, sk, attrs| {
            let keep = match (&where_key, &where_val) {
                (Some(key), Some(val)) => {
                    let item = decode_row(attrs)?;
                    item.get(key).map(|v| v == val).unwrap_or(false)
                }
                _ => true, // No WHERE → DELETE all (matches the legacy behaviour).
            };
            if keep {
                targets.push((pk.to_string(), sk.to_string()));
            }
            Ok(true)
        },
    )?;

    for (pk, sk) in targets {
        sqlite.delete_item(&ctx.account_id, &ctx.region, &table_name, &pk, &sk)?;
    }
    Ok(())
}

// ─── Parsing helpers ──────────────────────────────────────────────────────────

/// Parse the table name and optional WHERE clause from a statement.
/// Returns (table_name, Option<key>, Option<ddb_attr_value>).
fn parse_from_where(
    stmt: &str,
    params: &Value,
) -> Result<(String, Option<String>, Option<Value>), AwsError> {
    let upper = stmt.to_uppercase();

    let from_pos = upper.find("FROM").ok_or_else(|| {
        AwsError::bad_request("ValidationException", "Statement missing FROM clause")
    })?;

    let after_from = stmt[from_pos + 4..].trim();
    let table_name = extract_quoted_identifier(after_from)?;

    if let Some(wp) = upper.find(" WHERE ") {
        let where_clause = stmt[wp + 7..].trim();
        if let Some((key, val)) = parse_simple_eq(where_clause, params) {
            return Ok((table_name, Some(key), Some(val)));
        }
    }

    Ok((table_name, None, None))
}

/// Extract the first `"identifier"` or unquoted word from a string.
fn extract_quoted_identifier(s: &str) -> Result<String, AwsError> {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix('"') {
        let end = rest.find('"').ok_or_else(|| {
            AwsError::bad_request("ValidationException", "Unterminated quoted identifier")
        })?;
        Ok(rest[..end].to_string())
    } else {
        let end = trimmed
            .find(|c: char| c.is_whitespace())
            .unwrap_or(trimmed.len());
        Ok(trimmed[..end].to_string())
    }
}

/// Parse `"key" = 'value'` or `"key" = ?` into (attr_name, ddb_attr_value).
fn parse_simple_eq(clause: &str, params: &Value) -> Option<(String, Value)> {
    let eq_pos = clause.find('=')?;
    let lhs = clause[..eq_pos].trim();
    let rhs = clause[eq_pos + 1..].trim();

    let key = lhs.trim_matches('"').to_string();

    let val = if rhs == "?" {
        params.as_array()?.first()?.clone()
    } else if rhs.starts_with('\'') {
        let inner = rhs.trim_matches('\'');
        json!({ "S": inner })
    } else if let Ok(n) = rhs.parse::<i64>() {
        json!({ "N": n.to_string() })
    } else {
        json!({ "S": rhs })
    };

    Some((key, val))
}

/// Apply a simple SET clause like `attr = 'value'` to an existing DynamoItem.
fn apply_set_clause(item: &mut DynamoItem, set_clause: &str, _params: &Value) {
    for assignment in set_clause.split(',') {
        let a = assignment.trim();
        if let Some(eq) = a.find('=') {
            let attr = a[..eq].trim().trim_matches('"').to_string();
            let val_str = a[eq + 1..].trim();
            let val = if val_str.starts_with('\'') {
                json!({ "S": val_str.trim_matches('\'') })
            } else if let Ok(n) = val_str.parse::<i64>() {
                json!({ "N": n.to_string() })
            } else {
                json!({ "S": val_str })
            };
            item.insert(attr, val);
        }
    }
}

/// Convert a plain JSON object to DynamoDB attribute-value format.
fn plain_to_ddb_item(plain: &Value) -> DynamoItem {
    let mut out = HashMap::new();
    if let Some(obj) = plain.as_object() {
        for (k, v) in obj {
            out.insert(k.clone(), plain_to_ddb_val(v));
        }
    }
    out
}

fn plain_to_ddb_val(v: &Value) -> Value {
    match v {
        Value::String(s) => json!({ "S": s }),
        Value::Number(n) => json!({ "N": n.to_string() }),
        Value::Bool(b) => json!({ "BOOL": b }),
        Value::Null => json!({ "NULL": true }),
        Value::Array(arr) => {
            let list: Vec<Value> = arr.iter().map(plain_to_ddb_val).collect();
            json!({ "L": list })
        }
        Value::Object(_) => {
            let inner = plain_to_ddb_item(v);
            json!({ "M": inner })
        }
    }
}

/// Replace single-quoted strings with double-quoted in JSON-like PartiQL objects.
fn normalize_partiql_json(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            result.push('"');
            while let Some(&nc) = chars.peek() {
                chars.next();
                if nc == '\'' {
                    result.push('"');
                    break;
                }
                result.push(nc);
            }
        } else {
            result.push(c);
        }
    }
    result
}
