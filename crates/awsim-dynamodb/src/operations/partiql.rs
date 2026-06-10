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
    keys::{extract_item_keys, extract_pk_sk, item_to_storage_value, storage_value_to_item},
    sqlite_store::SqliteStore,
    state::{DynamoItem, DynamoState, Table},
    throttle::BucketKind,
};

use super::{
    item::{estimate_item_bytes, estimate_value_bytes},
    read_capacity_units, write_capacity_units,
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

    if stmts.is_empty() {
        return Err(AwsError::bad_request(
            "ValidationException",
            "Statements must not be empty",
        ));
    }
    if stmts.len() > BATCH_STATEMENT_MAX {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "Statements can contain at most {BATCH_STATEMENT_MAX} statements ({} provided)",
                stmts.len()
            ),
        ));
    }

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
                // BatchStatementError uses short codes (no "Exception"
                // suffix), distinct from the top-level exception names.
                responses.push(json!({
                    "Error": {
                        "Code": batch_error_code(&e.code),
                        "Message": e.message
                    }
                }));
            }
        }
    }

    Ok(json!({ "Responses": responses }))
}

/// AWS caps `BatchExecuteStatement` at 25 statements per call.
const BATCH_STATEMENT_MAX: usize = 25;

/// Map a top-level exception name to the short `BatchStatementError` code AWS
/// reports per statement (e.g. `ResourceNotFoundException` -> `ResourceNotFound`,
/// `ValidationException` -> `ValidationError`).
fn batch_error_code(code: &str) -> String {
    match code {
        "ValidationException" => "ValidationError".to_string(),
        other => other.strip_suffix("Exception").unwrap_or(other).to_string(),
    }
}

pub fn execute_transaction(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let (idempotency, replay) = super::idempotency::begin(state, input, ctx)?;
    if let Some(cached) = replay {
        return Ok(cached);
    }

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

    let result = json!({ "Responses": responses });
    idempotency.record(state, &result);
    Ok(result)
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
    let mut response_bytes = 0usize;
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
                let entry = json!(item);
                response_bytes += estimate_value_bytes(&entry);
                items.push(entry);
            }
            Ok(true)
        },
    )?;
    let read_units = read_capacity_units(response_bytes, false, false);
    state.enforce_throughput(&table_name, BucketKind::Read, read_units)?;
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

    // PartiQL INSERT does not overwrite: a pre-existing primary key is a
    // DuplicateItemException, unlike PutItem which replaces.
    if sqlite
        .get_item(
            &ctx.account_id,
            &ctx.region,
            &table_name,
            &sqlite_keys.pk,
            &sqlite_keys.sk,
        )?
        .is_some()
    {
        return Err(AwsError::bad_request(
            "DuplicateItemException",
            "Duplicate primary key exists in table",
        ));
    }

    let item_bytes = estimate_item_bytes(&ddb_item);
    let write_units = write_capacity_units(item_bytes, false);
    state.enforce_throughput(&table_name, BucketKind::Write, write_units)?;

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
    // UPDATE "TableName" SET attr = val [, ...] WHERE <full primary key> [AND ...]
    let upper = stmt.to_uppercase();
    let after_update = stmt.get(6..).unwrap_or("").trim(); // strip leading "UPDATE"
    let table_name = extract_quoted_identifier(after_update)?;

    let set_pos = upper.find(" SET ").ok_or_else(|| {
        AwsError::bad_request("ValidationException", "Invalid UPDATE: missing SET clause")
    })?;
    let after_set = &stmt[set_pos + 5..];
    let (assignments, where_clause) = match after_set.to_uppercase().find(" WHERE ") {
        Some(wp) => (after_set[..wp].trim(), after_set[wp + 7..].trim()),
        None => {
            return Err(AwsError::bad_request(
                "ValidationException",
                "UPDATE statement must specify the primary key in a WHERE clause",
            ));
        }
    };

    // Positional parameters fill SET assignments first, then the WHERE clause.
    let mut cursor = ParamCursor::new(params);
    let set_updates = parse_set_assignments(assignments, &mut cursor)?;
    let conditions = parse_where_equalities(where_clause, &mut cursor)?;

    // Resolve the exact target key (and any non-key conditions) up front while
    // the schema guard is held; release it before touching SQLite.
    let (pk, sk, non_key) = {
        let table = state.tables.get(&table_name).ok_or_else(|| {
            AwsError::service_not_found(
                "ResourceNotFoundException",
                format!("Table '{table_name}' not found"),
            )
        })?;
        let (key_item, non_key) = resolve_key_and_conditions(&table, conditions)?;
        let (pk, sk) = extract_pk_sk(&table, &key_item)
            .ok_or_else(|| AwsError::validation("Could not construct primary key"))?;
        (pk, sk, non_key)
    };

    // UPDATE targets a single existing item; a missing key is a no-op.
    let Some(stored) = sqlite.get_item(&ctx.account_id, &ctx.region, &table_name, &pk, &sk)? else {
        return Ok(());
    };
    let mut item = decode_row(stored)?;

    // Non-key WHERE predicates act as a conditional check on the target item.
    for (attr, val) in &non_key {
        if item.get(attr) != Some(val) {
            return Err(conditional_check_failed());
        }
    }

    for (attr, val) in set_updates {
        item.insert(attr, val);
    }

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

    let item_bytes = estimate_item_bytes(&item);
    let write_units = write_capacity_units(item_bytes, false);
    state.enforce_throughput(&table_name, BucketKind::Write, write_units)?;

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
    // DELETE FROM "TableName" WHERE <full primary key> [AND ...]
    let upper = stmt.to_uppercase();
    let from_pos = upper.find("FROM").ok_or_else(|| {
        AwsError::bad_request("ValidationException", "DELETE missing FROM clause")
    })?;
    let table_name = extract_quoted_identifier(stmt[from_pos + 4..].trim())?;

    let Some(wp) = upper.find(" WHERE ") else {
        return Err(AwsError::bad_request(
            "ValidationException",
            "DELETE statement must specify the primary key in a WHERE clause",
        ));
    };
    let mut cursor = ParamCursor::new(params);
    let conditions = parse_where_equalities(stmt[wp + 7..].trim(), &mut cursor)?;

    let (pk, sk, non_key) = {
        let table = state.tables.get(&table_name).ok_or_else(|| {
            AwsError::service_not_found(
                "ResourceNotFoundException",
                format!("Table '{table_name}' not found"),
            )
        })?;
        let (key_item, non_key) = resolve_key_and_conditions(&table, conditions)?;
        let (pk, sk) = extract_pk_sk(&table, &key_item)
            .ok_or_else(|| AwsError::validation("Could not construct primary key"))?;
        (pk, sk, non_key)
    };

    // Charge the 1 WCU AWS minimum regardless of whether a row matched.
    state.enforce_throughput(&table_name, BucketKind::Write, 1.0)?;

    let Some(stored) = sqlite.get_item(&ctx.account_id, &ctx.region, &table_name, &pk, &sk)? else {
        return Ok(()); // Deleting a missing key is a no-op success.
    };

    // Non-key WHERE predicates act as a conditional check before deleting.
    if !non_key.is_empty() {
        let item = decode_row(stored)?;
        for (attr, val) in &non_key {
            if item.get(attr) != Some(val) {
                return Err(conditional_check_failed());
            }
        }
    }

    sqlite.delete_item(&ctx.account_id, &ctx.region, &table_name, &pk, &sk)?;
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

/// Cursor over positional `?` parameters, consumed left-to-right across a
/// statement (SET assignments first, then the WHERE clause).
struct ParamCursor<'a> {
    params: &'a [Value],
    idx: usize,
}

impl<'a> ParamCursor<'a> {
    fn new(params: &'a Value) -> Self {
        Self {
            params: params.as_array().map(Vec::as_slice).unwrap_or(&[]),
            idx: 0,
        }
    }

    fn take(&mut self) -> Option<&'a Value> {
        let v = self.params.get(self.idx);
        if v.is_some() {
            self.idx += 1;
        }
        v
    }
}

/// Resolve a right-hand-side token to a typed AttributeValue. `?` consumes the
/// next positional parameter (already an AttributeValue); literals are coerced
/// to S / N / BOOL.
fn parse_rhs_value(rhs: &str, cursor: &mut ParamCursor) -> Result<Value, AwsError> {
    let rhs = rhs.trim();
    if rhs == "?" {
        return cursor.take().cloned().ok_or_else(|| {
            AwsError::bad_request(
                "ValidationException",
                "Too few parameters for the statement's placeholders",
            )
        });
    }
    if let Some(inner) = rhs.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return Ok(json!({ "S": inner }));
    }
    if rhs.eq_ignore_ascii_case("true") || rhs.eq_ignore_ascii_case("false") {
        return Ok(json!({ "BOOL": rhs.eq_ignore_ascii_case("true") }));
    }
    if rhs.parse::<f64>().is_ok() {
        return Ok(json!({ "N": rhs }));
    }
    Ok(json!({ "S": rhs }))
}

/// Split a clause on the ` AND ` connective (case-insensitive), leaving
/// attribute names that merely contain "and" intact.
fn split_and(clause: &str) -> Vec<&str> {
    let upper = clause.to_uppercase();
    let mut parts = Vec::new();
    let mut last = 0;
    let mut from = 0;
    while let Some(rel) = upper[from..].find(" AND ") {
        let pos = from + rel;
        parts.push(clause[last..pos].trim());
        last = pos + 5;
        from = pos + 5;
    }
    parts.push(clause[last..].trim());
    parts
}

/// Parse `SET a = v, b = ?` into (attribute, value) pairs, consuming positional
/// parameters in order.
fn parse_set_assignments(
    clause: &str,
    cursor: &mut ParamCursor,
) -> Result<Vec<(String, Value)>, AwsError> {
    let mut out = Vec::new();
    for assignment in clause.split(',') {
        let a = assignment.trim();
        if a.is_empty() {
            continue;
        }
        let eq = a.find('=').ok_or_else(|| {
            AwsError::bad_request(
                "ValidationException",
                format!("Invalid SET assignment: {a}"),
            )
        })?;
        let attr = a[..eq].trim().trim_matches('"').to_string();
        let val = parse_rhs_value(&a[eq + 1..], cursor)?;
        out.push((attr, val));
    }
    Ok(out)
}

/// Parse a WHERE clause of AND-ed equalities into (attribute, value) pairs,
/// consuming positional parameters in order.
fn parse_where_equalities(
    clause: &str,
    cursor: &mut ParamCursor,
) -> Result<Vec<(String, Value)>, AwsError> {
    let mut out = Vec::new();
    for cond in split_and(clause) {
        let eq = cond.find('=').ok_or_else(|| {
            AwsError::bad_request(
                "ValidationException",
                format!("Unsupported WHERE condition (only equality is supported): {cond}"),
            )
        })?;
        let attr = cond[..eq].trim().trim_matches('"').to_string();
        let val = parse_rhs_value(&cond[eq + 1..], cursor)?;
        out.push((attr, val));
    }
    Ok(out)
}

/// Split parsed WHERE equalities into the primary-key match and the remaining
/// non-key conditions, requiring every key attribute to be constrained by
/// equality (UPDATE / DELETE must target a full primary key, as AWS demands).
fn resolve_key_and_conditions(
    table: &Table,
    conditions: Vec<(String, Value)>,
) -> Result<(DynamoItem, Vec<(String, Value)>), AwsError> {
    let key_names: Vec<&str> = table
        .key_schema
        .iter()
        .map(|k| k.attribute_name.as_str())
        .collect();

    let mut key_item = DynamoItem::new();
    let mut non_key = Vec::new();
    for (attr, val) in conditions {
        if key_names.contains(&attr.as_str()) {
            key_item.insert(attr, val);
        } else {
            non_key.push((attr, val));
        }
    }

    for name in &key_names {
        if !key_item.contains_key(*name) {
            return Err(AwsError::bad_request(
                "ValidationException",
                format!(
                    "Where clause does not contain a mandatory equality on the \
                     primary key attribute: {name}"
                ),
            ));
        }
    }
    Ok((key_item, non_key))
}

/// The standard conditional-check failure for a non-key WHERE predicate that
/// the targeted item does not satisfy.
fn conditional_check_failed() -> AwsError {
    AwsError::bad_request(
        "ConditionalCheckFailedException",
        "The conditional request failed",
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{KeySchemaElement, Table};
    use std::collections::VecDeque;

    fn ctx() -> RequestContext {
        RequestContext::new("dynamodb", "us-east-1")
    }

    /// State with a composite-key table `t` (pk HASH, sk RANGE).
    fn setup() -> (DynamoState, SqliteStore, RequestContext) {
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
        (state, SqliteStore::in_memory().unwrap(), ctx())
    }

    fn exec(
        state: &DynamoState,
        sqlite: &SqliteStore,
        c: &RequestContext,
        sql: &str,
    ) -> Result<Value, AwsError> {
        execute_statement(state, sqlite, &json!({ "Statement": sql }), c)
    }

    #[test]
    fn insert_rejects_duplicate_primary_key() {
        let (state, sqlite, c) = setup();
        exec(
            &state,
            &sqlite,
            &c,
            r#"INSERT INTO "t" VALUE {'pk': 'a', 'sk': 'b', 'v': 1}"#,
        )
        .unwrap();
        let err = exec(
            &state,
            &sqlite,
            &c,
            r#"INSERT INTO "t" VALUE {'pk': 'a', 'sk': 'b', 'v': 2}"#,
        )
        .unwrap_err();
        assert_eq!(err.code, "DuplicateItemException");
    }

    #[test]
    fn update_and_delete_require_full_primary_key() {
        let (state, sqlite, c) = setup();
        exec(
            &state,
            &sqlite,
            &c,
            r#"INSERT INTO "t" VALUE {'pk': 'a', 'sk': 'b'}"#,
        )
        .unwrap();

        // Only the partition key is constrained; the sort key is missing.
        let upd = exec(
            &state,
            &sqlite,
            &c,
            r#"UPDATE "t" SET v = 9 WHERE pk = 'a'"#,
        )
        .unwrap_err();
        assert_eq!(upd.code, "ValidationException");
        let del = exec(&state, &sqlite, &c, r#"DELETE FROM "t" WHERE pk = 'a'"#).unwrap_err();
        assert_eq!(del.code, "ValidationException");
    }

    #[test]
    fn update_then_delete_with_full_key_and_positional_params() {
        let (state, sqlite, c) = setup();
        exec(
            &state,
            &sqlite,
            &c,
            r#"INSERT INTO "t" VALUE {'pk': 'a', 'sk': 'b', 'v': 1}"#,
        )
        .unwrap();

        execute_statement(
            &state,
            &sqlite,
            &json!({
                "Statement": r#"UPDATE "t" SET v = ? WHERE pk = ? AND sk = ?"#,
                "Parameters": [{"N": "5"}, {"S": "a"}, {"S": "b"}],
            }),
            &c,
        )
        .unwrap();
        let sel = exec(&state, &sqlite, &c, r#"SELECT * FROM "t""#).unwrap();
        assert_eq!(sel["Items"][0]["v"], json!({"N": "5"}));

        exec(
            &state,
            &sqlite,
            &c,
            r#"DELETE FROM "t" WHERE pk = 'a' AND sk = 'b'"#,
        )
        .unwrap();
        let sel = exec(&state, &sqlite, &c, r#"SELECT * FROM "t""#).unwrap();
        assert_eq!(sel["Items"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn delete_non_key_condition_acts_as_conditional_check() {
        let (state, sqlite, c) = setup();
        exec(
            &state,
            &sqlite,
            &c,
            r#"INSERT INTO "t" VALUE {'pk': 'a', 'sk': 'b', 'v': 1}"#,
        )
        .unwrap();

        // Full key present, but the non-key predicate v = 2 does not hold.
        let err = exec(
            &state,
            &sqlite,
            &c,
            r#"DELETE FROM "t" WHERE pk = 'a' AND sk = 'b' AND v = 2"#,
        )
        .unwrap_err();
        assert_eq!(err.code, "ConditionalCheckFailedException");

        // The item is still present.
        let sel = exec(&state, &sqlite, &c, r#"SELECT * FROM "t""#).unwrap();
        assert_eq!(sel["Items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn batch_execute_caps_at_25_and_uses_short_error_codes() {
        let (state, sqlite, c) = setup();

        let too_many: Vec<Value> = (0..26)
            .map(|_| json!({ "Statement": r#"SELECT * FROM "t""# }))
            .collect();
        let err = batch_execute_statement(&state, &sqlite, &json!({ "Statements": too_many }), &c)
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");

        let resp = batch_execute_statement(
            &state,
            &sqlite,
            &json!({ "Statements": [{ "Statement": r#"SELECT * FROM "missing""# }] }),
            &c,
        )
        .unwrap();
        assert_eq!(
            resp["Responses"][0]["Error"]["Code"],
            json!("ResourceNotFound")
        );
    }
}
