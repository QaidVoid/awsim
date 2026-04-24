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
use std::collections::HashMap;

use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::{DynamoItem, DynamoState};

// ─── Public entry points ──────────────────────────────────────────────────────

pub fn execute_statement(
    state: &DynamoState,
    input: &Value,
    _ctx: &awsim_core::RequestContext,
) -> Result<Value, AwsError> {
    let stmt = input
        .get("Statement")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Statement is required"))?;

    let params = input.get("Parameters").cloned().unwrap_or(json!([]));

    let items = run_statement(state, stmt, &params)?;

    Ok(json!({
        "Items": items,
        "NextToken": null
    }))
}

pub fn batch_execute_statement(
    state: &DynamoState,
    input: &Value,
    _ctx: &awsim_core::RequestContext,
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

        match run_statement(state, stmt, &params) {
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
    input: &Value,
    _ctx: &awsim_core::RequestContext,
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

        match run_statement(state, stmt, &params) {
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
    raw_stmt: &str,
    params: &Value,
) -> Result<Vec<Value>, AwsError> {
    let stmt = raw_stmt.trim();
    let upper = stmt.to_uppercase();

    if upper.starts_with("SELECT") {
        run_select(state, stmt, params)
    } else if upper.starts_with("INSERT") {
        run_insert(state, stmt, params)?;
        Ok(vec![])
    } else if upper.starts_with("UPDATE") {
        run_update(state, stmt, params)?;
        Ok(vec![])
    } else if upper.starts_with("DELETE") {
        run_delete(state, stmt, params)?;
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

// ─── SELECT ──────────────────────────────────────────────────────────────────

fn run_select(state: &DynamoState, stmt: &str, params: &Value) -> Result<Vec<Value>, AwsError> {
    // Minimal parse: SELECT ... FROM "TableName" [WHERE "key" = val]
    let (table_name, where_key, where_val) = parse_from_where(stmt, params)?;

    let table = state.tables.get(&table_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Table '{table_name}' not found"),
        )
    })?;

    let items: Vec<Value> = table
        .items
        .values()
        .filter(|item| {
            if let (Some(key), Some(val)) = (&where_key, &where_val) {
                item.get(key).map(|attr| attr == val).unwrap_or(false)
            } else {
                true
            }
        })
        .map(|item| json!(item))
        .collect();

    Ok(items)
}

// ─── INSERT ───────────────────────────────────────────────────────────────────

fn run_insert(state: &DynamoState, stmt: &str, _params: &Value) -> Result<(), AwsError> {
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

    // Parse the inline JSON object (single-quoted strings → double-quoted).
    let normalized = normalize_partiql_json(json_str);
    let plain_item: Value = serde_json::from_str(&normalized).map_err(|e| {
        AwsError::bad_request("ValidationException", format!("Invalid INSERT VALUE: {e}"))
    })?;

    // Wrap each scalar into a DynamoDB attribute-value form.
    let ddb_item: DynamoItem = plain_to_ddb_item(&plain_item);

    let mut table = state.tables.get_mut(&table_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Table '{table_name}' not found"),
        )
    })?;

    let composite = table
        .composite_key(&ddb_item)
        .ok_or_else(|| AwsError::bad_request("ValidationException", "Item missing primary key"))?;

    table.items.insert(composite, ddb_item);

    Ok(())
}

// ─── UPDATE ───────────────────────────────────────────────────────────────────

fn run_update(state: &DynamoState, stmt: &str, params: &Value) -> Result<(), AwsError> {
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

    let mut table = state.tables.get_mut(&table_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Table '{table_name}' not found"),
        )
    })?;

    if let (Some(key), Some(val)) = (where_key, where_val) {
        for item in table.items.values_mut() {
            if item.get(&key).map(|v| v == &val).unwrap_or(false) {
                apply_set_clause(item, assignments, params);
                break;
            }
        }
    }

    Ok(())
}

// ─── DELETE ───────────────────────────────────────────────────────────────────

fn run_delete(state: &DynamoState, stmt: &str, params: &Value) -> Result<(), AwsError> {
    let (table_name, where_key, where_val) = parse_from_where(stmt, params)?;

    let mut table = state.tables.get_mut(&table_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Table '{table_name}' not found"),
        )
    })?;

    if let (Some(key), Some(val)) = (where_key, where_val) {
        // Collect matching composite keys first, then remove.
        let matching: Vec<String> = table
            .items
            .iter()
            .filter(|(_, item)| item.get(&key).map(|v| v == &val).unwrap_or(false))
            .map(|(k, _)| k.clone())
            .collect();

        for k in matching {
            table.items.remove(&k);
        }
    } else {
        table.items.clear();
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

    // Look for WHERE clause.
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
    if trimmed.starts_with('"') {
        let end = trimmed[1..].find('"').ok_or_else(|| {
            AwsError::bad_request("ValidationException", "Unterminated quoted identifier")
        })?;
        Ok(trimmed[1..end + 1].to_string())
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
