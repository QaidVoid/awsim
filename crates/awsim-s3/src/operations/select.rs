//! `SelectObjectContent`: run a small SQL query over a CSV or JSON object
//! and stream the matching records back in the S3 Select event-stream
//! framing.
//!
//! The supported SQL is the subset applications most often use against
//! S3 Select:
//!
//! - Projection: `SELECT *`, a comma-separated list of column references,
//!   or `SELECT COUNT(*)`.
//! - Source: `FROM S3Object` with an optional alias (`FROM S3Object s`).
//! - Filter: an optional `WHERE` clause of comparisons (`=`, `<>`, `!=`,
//!   `<`, `<=`, `>`, `>=`) combined with `AND` and `OR`, where `AND` binds
//!   tighter than `OR`.
//!
//! Column references resolve against CSV headers (when `FileHeaderInfo` is
//! `USE`), one-based positional names (`_1`, `_2`, ...) otherwise, or JSON
//! object fields. A table alias prefix (`s.name`) and surrounding double
//! quotes are stripped before lookup. Comparisons are numeric when both
//! sides parse as numbers and lexicographic otherwise.

use std::collections::HashMap;

use awsim_core::protocol::eventstream::{EventHeader, append_message};
use awsim_core::{AwsError, RequestContext};
use base64::Engine as _;
use serde_json::{Map, Value, json};

use super::bucket::no_such_bucket;
use super::object::no_such_key;
use super::require_str;
use crate::state::S3State;

/// A single decoded input record, addressable by column name and by the
/// ordered position used for `SELECT *`.
struct Record {
    ordered: Vec<(String, Value)>,
    by_name: HashMap<String, Value>,
}

impl Record {
    fn get(&self, reference: &str) -> Option<&Value> {
        self.by_name.get(&normalize_ref(reference))
    }
}

/// Strip a table alias prefix and surrounding quotes from a column
/// reference, e.g. `s."Name"` becomes `Name`.
fn normalize_ref(reference: &str) -> String {
    let after_dot = reference.rsplit('.').next().unwrap_or(reference);
    after_dot.trim_matches('"').to_string()
}

pub fn select_object_content(
    state: &S3State,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?;
    let key = require_str(input, "Key")?;

    let root = input.get("SelectObjectContentRequest").unwrap_or(input);
    let expression = root
        .get("Expression")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AwsError::bad_request("MissingRequiredParameter", "Expression is required")
        })?;

    let input_ser = root.get("InputSerialization");
    let output_ser = root.get("OutputSerialization");

    let body = read_object_bytes(state, bucket_name, key)?;
    let bytes_scanned = body.len() as u64;

    let records = parse_records(&body, input_ser)?;
    let query = parse_query(expression)?;
    let output = run_query(&query, &records, output_ser)?;

    let bytes_returned = output.len() as u64;
    let frames = encode_event_stream(&output, bytes_scanned, bytes_returned);

    Ok(json!({
        "__raw_body": base64::engine::general_purpose::STANDARD.encode(&frames),
        "__headers": { "content-type": "application/vnd.amazon.eventstream" },
    }))
}

fn read_object_bytes(state: &S3State, bucket: &str, key: &str) -> Result<Vec<u8>, AwsError> {
    let bucket_ref = state
        .buckets
        .get(bucket)
        .ok_or_else(|| no_such_bucket(bucket))?;
    let versions = bucket_ref
        .objects
        .get(key)
        .ok_or_else(|| no_such_key(key))?;
    let object = versions.current().ok_or_else(|| no_such_key(key))?;
    object
        .body
        .read_all()
        .map_err(|e| AwsError::internal(format!("read object body: {e}")))
}

// ── Input parsing ─────────────────────────────────────────────────────────

fn parse_records(body: &[u8], input_ser: Option<&Value>) -> Result<Vec<Record>, AwsError> {
    let text = std::str::from_utf8(body)
        .map_err(|_| AwsError::bad_request("InvalidRequest", "Object is not valid UTF-8"))?;

    if let Some(csv) = input_ser.and_then(|s| s.get("CSV")) {
        parse_csv_records(text, csv)
    } else if let Some(json_cfg) = input_ser.and_then(|s| s.get("JSON")) {
        parse_json_records(text, json_cfg)
    } else {
        Err(AwsError::bad_request(
            "InvalidRequest",
            "InputSerialization must specify CSV or JSON",
        ))
    }
}

fn parse_csv_records(text: &str, csv: &Value) -> Result<Vec<Record>, AwsError> {
    let field_delim = csv
        .get("FieldDelimiter")
        .and_then(Value::as_str)
        .and_then(|s| s.chars().next())
        .unwrap_or(',');
    let quote = csv
        .get("QuoteCharacter")
        .and_then(Value::as_str)
        .and_then(|s| s.chars().next())
        .unwrap_or('"');
    let header_info = csv
        .get("FileHeaderInfo")
        .and_then(Value::as_str)
        .unwrap_or("NONE")
        .to_ascii_uppercase();

    let mut rows = text
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| split_csv_line(line, field_delim, quote));

    let headers: Option<Vec<String>> = match header_info.as_str() {
        "USE" => rows.next(),
        "IGNORE" => {
            rows.next();
            None
        }
        _ => None,
    };

    let mut records = Vec::new();
    for row in rows {
        let mut ordered = Vec::with_capacity(row.len());
        let mut by_name = HashMap::new();
        for (i, value) in row.iter().enumerate() {
            let positional = format!("_{}", i + 1);
            let cell = Value::String(value.clone());
            by_name.insert(positional.clone(), cell.clone());
            let name = headers
                .as_ref()
                .and_then(|h| h.get(i))
                .cloned()
                .unwrap_or(positional);
            by_name.insert(name.clone(), cell.clone());
            ordered.push((name, cell));
        }
        records.push(Record { ordered, by_name });
    }
    Ok(records)
}

/// Split one CSV line on `delim`, honoring `quote`-wrapped fields and
/// doubled quote escapes.
fn split_csv_line(line: &str, delim: char, quote: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == quote {
            if in_quotes && chars.peek() == Some(&quote) {
                field.push(quote);
                chars.next();
            } else {
                in_quotes = !in_quotes;
            }
        } else if c == delim && !in_quotes {
            fields.push(std::mem::take(&mut field));
        } else {
            field.push(c);
        }
    }
    fields.push(field);
    fields
}

fn parse_json_records(text: &str, json_cfg: &Value) -> Result<Vec<Record>, AwsError> {
    let json_type = json_cfg
        .get("Type")
        .and_then(Value::as_str)
        .unwrap_or("DOCUMENT")
        .to_ascii_uppercase();

    let values: Vec<Value> = if json_type == "LINES" {
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                serde_json::from_str::<Value>(l).map_err(|e| {
                    AwsError::bad_request("InvalidRequest", format!("Invalid JSON: {e}"))
                })
            })
            .collect::<Result<_, _>>()?
    } else {
        let parsed: Value = serde_json::from_str(text.trim())
            .map_err(|e| AwsError::bad_request("InvalidRequest", format!("Invalid JSON: {e}")))?;
        match parsed {
            Value::Array(arr) => arr,
            other => vec![other],
        }
    };

    Ok(values.into_iter().map(record_from_json).collect())
}

fn record_from_json(value: Value) -> Record {
    let mut ordered = Vec::new();
    let mut by_name = HashMap::new();
    if let Value::Object(map) = &value {
        for (k, v) in map {
            ordered.push((k.clone(), v.clone()));
            by_name.insert(k.clone(), v.clone());
        }
    }
    Record { ordered, by_name }
}

// ── Query parsing ───────────────────────────────────────────────────────────

struct Query {
    projection: Projection,
    filter: Option<Expr>,
}

enum Projection {
    All,
    Count,
    Columns(Vec<String>),
}

/// A WHERE expression: a disjunction of conjunctions of comparisons.
enum Expr {
    Compare {
        left: Operand,
        op: CompareOp,
        right: Operand,
    },
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

enum Operand {
    Column(String),
    Literal(Value),
}

#[derive(Clone, Copy)]
enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

fn parse_query(expression: &str) -> Result<Query, AwsError> {
    let normalized = expression.trim().trim_end_matches(';');
    let lower = normalized.to_ascii_lowercase();

    let select_at = lower
        .find("select ")
        .ok_or_else(|| invalid_sql("query must start with SELECT"))?;
    let from_at = lower
        .find(" from ")
        .ok_or_else(|| invalid_sql("query must contain FROM"))?;

    let projection_str = normalized[select_at + 7..from_at].trim();
    let projection = parse_projection(projection_str)?;

    let after_from = &normalized[from_at + 6..];
    let after_from_lower = lower[from_at + 6..].to_string();
    let filter = if let Some(where_at) = after_from_lower.find(" where ") {
        Some(parse_expr(after_from[where_at + 7..].trim())?)
    } else {
        None
    };

    Ok(Query { projection, filter })
}

fn parse_projection(text: &str) -> Result<Projection, AwsError> {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join("");
    if text.trim() == "*" {
        return Ok(Projection::All);
    }
    if collapsed.eq_ignore_ascii_case("count(*)") {
        return Ok(Projection::Count);
    }
    let columns: Vec<String> = text
        .split(',')
        .map(|c| normalize_ref(c.trim()))
        .filter(|c| !c.is_empty())
        .collect();
    if columns.is_empty() {
        return Err(invalid_sql("empty projection"));
    }
    Ok(Projection::Columns(columns))
}

/// Parse a WHERE expression. `OR` splits first (lowest precedence), then
/// `AND`, then a single comparison.
fn parse_expr(text: &str) -> Result<Expr, AwsError> {
    if let Some((left, right)) = split_top_keyword(text, "or") {
        return Ok(Expr::Or(
            Box::new(parse_expr(&left)?),
            Box::new(parse_expr(&right)?),
        ));
    }
    if let Some((left, right)) = split_top_keyword(text, "and") {
        return Ok(Expr::And(
            Box::new(parse_expr(&left)?),
            Box::new(parse_expr(&right)?),
        ));
    }
    parse_comparison(text)
}

/// Split on the first top-level (not inside quotes) occurrence of a
/// whitespace-delimited keyword such as `and` / `or`.
fn split_top_keyword(text: &str, keyword: &str) -> Option<(String, String)> {
    let bytes = text.as_bytes();
    let mut in_quotes = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '\'' {
            in_quotes = !in_quotes;
        }
        if !in_quotes && (c == ' ' || c == '\t') && text[i..].len() > keyword.len() + 1 {
            let rest = text[i..].trim_start();
            let rest_lower = rest.to_ascii_lowercase();
            if rest_lower.starts_with(keyword)
                && rest[keyword.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| c == ' ' || c == '\t')
            {
                let left = text[..i].trim().to_string();
                let right = rest[keyword.len()..].trim().to_string();
                if !left.is_empty() && !right.is_empty() {
                    return Some((left, right));
                }
            }
        }
        i += 1;
    }
    None
}

fn parse_comparison(text: &str) -> Result<Expr, AwsError> {
    let text = text.trim();
    // Order matters: match the two-character operators before the single ones.
    for (token, op) in [
        ("<=", CompareOp::Le),
        (">=", CompareOp::Ge),
        ("<>", CompareOp::Ne),
        ("!=", CompareOp::Ne),
        ("=", CompareOp::Eq),
        ("<", CompareOp::Lt),
        (">", CompareOp::Gt),
    ] {
        if let Some(at) = find_top_level(text, token) {
            let left = parse_operand(text[..at].trim())?;
            let right = parse_operand(text[at + token.len()..].trim())?;
            return Ok(Expr::Compare { left, op, right });
        }
    }
    Err(invalid_sql("unsupported WHERE comparison"))
}

/// Find an operator token outside of single-quoted literals.
fn find_top_level(text: &str, token: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let token_bytes = token.as_bytes();
    let mut in_quotes = false;
    let mut i = 0;
    while i + token_bytes.len() <= bytes.len() {
        let c = bytes[i] as char;
        if c == '\'' {
            in_quotes = !in_quotes;
        }
        if !in_quotes && &bytes[i..i + token_bytes.len()] == token_bytes {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn parse_operand(text: &str) -> Result<Operand, AwsError> {
    let text = text.trim();
    if text.len() >= 2 && text.starts_with('\'') && text.ends_with('\'') {
        return Ok(Operand::Literal(Value::String(
            text[1..text.len() - 1].replace("''", "'"),
        )));
    }
    if let Ok(n) = text.parse::<f64>() {
        return Ok(Operand::Literal(json!(n)));
    }
    if text.eq_ignore_ascii_case("true") || text.eq_ignore_ascii_case("false") {
        return Ok(Operand::Literal(json!(text.eq_ignore_ascii_case("true"))));
    }
    Ok(Operand::Column(normalize_ref(text)))
}

// ── Evaluation ────────────────────────────────────────────────────────────

fn run_query(
    query: &Query,
    records: &[Record],
    output_ser: Option<&Value>,
) -> Result<Vec<u8>, AwsError> {
    let matched: Vec<&Record> = records
        .iter()
        .filter(|r| match &query.filter {
            Some(expr) => eval_expr(expr, r),
            None => true,
        })
        .collect();

    let json_output = output_ser.map(|o| o.get("JSON").is_some()).unwrap_or(false);

    if matches!(query.projection, Projection::Count) {
        let count = matched.len();
        return Ok(if json_output {
            format!("{}\n", json!({ "_1": count }))
        } else {
            format!("{count}\n")
        }
        .into_bytes());
    }

    let record_delim = output_ser
        .and_then(|o| o.get("CSV").or_else(|| o.get("JSON")))
        .and_then(|c| c.get("RecordDelimiter"))
        .and_then(Value::as_str)
        .unwrap_or("\n")
        .to_string();
    let field_delim = output_ser
        .and_then(|o| o.get("CSV"))
        .and_then(|c| c.get("FieldDelimiter"))
        .and_then(Value::as_str)
        .unwrap_or(",")
        .to_string();

    let mut out = String::new();
    for record in matched {
        let fields = project(&query.projection, record);
        if json_output {
            let mut obj = Map::new();
            for (name, value) in fields {
                obj.insert(name, value);
            }
            out.push_str(&Value::Object(obj).to_string());
        } else {
            let cells: Vec<String> = fields.into_iter().map(|(_, v)| value_to_text(&v)).collect();
            out.push_str(&cells.join(&field_delim));
        }
        out.push_str(&record_delim);
    }
    Ok(out.into_bytes())
}

fn project(projection: &Projection, record: &Record) -> Vec<(String, Value)> {
    match projection {
        Projection::All => record.ordered.clone(),
        Projection::Count => Vec::new(),
        Projection::Columns(cols) => cols
            .iter()
            .map(|c| {
                let value = record.get(c).cloned().unwrap_or(Value::Null);
                (c.clone(), value)
            })
            .collect(),
    }
}

fn eval_expr(expr: &Expr, record: &Record) -> bool {
    match expr {
        Expr::And(a, b) => eval_expr(a, record) && eval_expr(b, record),
        Expr::Or(a, b) => eval_expr(a, record) || eval_expr(b, record),
        Expr::Compare { left, op, right } => {
            let l = resolve_operand(left, record);
            let r = resolve_operand(right, record);
            compare(&l, &r, *op)
        }
    }
}

fn resolve_operand(operand: &Operand, record: &Record) -> Value {
    match operand {
        Operand::Literal(v) => v.clone(),
        Operand::Column(name) => record.get(name).cloned().unwrap_or(Value::Null),
    }
}

fn compare(left: &Value, right: &Value, op: CompareOp) -> bool {
    let (ln, rn) = (value_as_number(left), value_as_number(right));
    if let (Some(a), Some(b)) = (ln, rn) {
        return match op {
            CompareOp::Eq => a == b,
            CompareOp::Ne => a != b,
            CompareOp::Lt => a < b,
            CompareOp::Le => a <= b,
            CompareOp::Gt => a > b,
            CompareOp::Ge => a >= b,
        };
    }
    let a = value_to_text(left);
    let b = value_to_text(right);
    match op {
        CompareOp::Eq => a == b,
        CompareOp::Ne => a != b,
        CompareOp::Lt => a < b,
        CompareOp::Le => a <= b,
        CompareOp::Gt => a > b,
        CompareOp::Ge => a >= b,
    }
}

fn value_as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn value_to_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn invalid_sql(message: &str) -> AwsError {
    AwsError::bad_request("InvalidSqlExpression", message)
}

// ── Event-stream framing ────────────────────────────────────────────────────

fn encode_event_stream(records: &[u8], bytes_scanned: u64, bytes_returned: u64) -> Vec<u8> {
    let mut out = Vec::new();

    if !records.is_empty() {
        append_message(
            &mut out,
            &[
                event_header(":message-type", "event"),
                event_header(":event-type", "Records"),
                event_header(":content-type", "application/octet-stream"),
            ],
            records,
        );
    }

    let stats = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <Stats><BytesScanned>{bytes_scanned}</BytesScanned>\
         <BytesProcessed>{bytes_scanned}</BytesProcessed>\
         <BytesReturned>{bytes_returned}</BytesReturned></Stats>"
    );
    append_message(
        &mut out,
        &[
            event_header(":message-type", "event"),
            event_header(":event-type", "Stats"),
            event_header(":content-type", "text/xml"),
        ],
        stats.as_bytes(),
    );

    append_message(
        &mut out,
        &[
            event_header(":message-type", "event"),
            event_header(":event-type", "End"),
        ],
        &[],
    );

    out
}

fn event_header(name: &str, value: &str) -> EventHeader {
    EventHeader {
        name: name.to_string(),
        value: value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn csv_input() -> Value {
        json!({ "CSV": { "FileHeaderInfo": "USE" } })
    }

    fn run(expr: &str, body: &str, input: &Value, output: &Value) -> String {
        let records = parse_records(body.as_bytes(), Some(input)).expect("parse records");
        let query = parse_query(expr).expect("parse query");
        let bytes = run_query(&query, &records, Some(output)).expect("run query");
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn csv_select_all_with_filter() {
        let body = "name,age\nAlice,30\nBob,25\n";
        let out = run(
            "SELECT * FROM S3Object s WHERE s.age > 26",
            body,
            &csv_input(),
            &json!({ "CSV": {} }),
        );
        assert_eq!(out, "Alice,30\n");
    }

    #[test]
    fn csv_select_columns_json_output() {
        let body = "name,age\nAlice,30\nBob,25\n";
        let out = run(
            "SELECT name FROM S3Object WHERE age < 28",
            body,
            &csv_input(),
            &json!({ "JSON": {} }),
        );
        assert_eq!(out, "{\"name\":\"Bob\"}\n");
    }

    #[test]
    fn csv_count_star() {
        let body = "name,age\nAlice,30\nBob,25\nCarol,40\n";
        let out = run(
            "SELECT COUNT(*) FROM S3Object WHERE age >= 30",
            body,
            &csv_input(),
            &json!({ "CSV": {} }),
        );
        assert_eq!(out, "2\n");
    }

    #[test]
    fn json_lines_with_and_or() {
        let body = "{\"name\":\"Alice\",\"age\":30}\n{\"name\":\"Bob\",\"age\":25}\n{\"name\":\"Carol\",\"age\":40}\n";
        let input = json!({ "JSON": { "Type": "LINES" } });
        let out = run(
            "SELECT name FROM S3Object s WHERE s.age > 35 OR s.name = 'Alice'",
            body,
            &input,
            &json!({ "JSON": {} }),
        );
        assert_eq!(out, "{\"name\":\"Alice\"}\n{\"name\":\"Carol\"}\n");
    }

    #[test]
    fn string_inequality_filter() {
        let body = "name\nAlice\nBob\n";
        let out = run(
            "SELECT name FROM S3Object WHERE name <> 'Alice'",
            body,
            &csv_input(),
            &json!({ "CSV": {} }),
        );
        assert_eq!(out, "Bob\n");
    }
}
