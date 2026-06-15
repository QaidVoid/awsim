use awsim_core::AwsError;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Value, json};

/// A SQL parameter value reduced to the literal forms the Data API can
/// carry. The Data API normally binds parameters server-side; AWSim
/// inlines them as escaped SQL literals instead, which keeps the engine
/// layer to a single "run this SQL string" call and makes the whole
/// substitution path pure and testable.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamLiteral {
    Null,
    Bool(bool),
    Long(i64),
    Double(f64),
    Str(String),
    Blob(Vec<u8>),
}

impl ParamLiteral {
    /// Render the value as a PostgreSQL literal safe to splice into a
    /// statement.
    pub fn to_sql_literal(&self) -> String {
        match self {
            ParamLiteral::Null => "NULL".to_string(),
            ParamLiteral::Bool(true) => "TRUE".to_string(),
            ParamLiteral::Bool(false) => "FALSE".to_string(),
            ParamLiteral::Long(n) => n.to_string(),
            ParamLiteral::Double(d) => format!("{d}"),
            ParamLiteral::Str(s) => format!("'{}'", s.replace('\'', "''")),
            ParamLiteral::Blob(bytes) => {
                let mut hex = String::with_capacity(bytes.len() * 2);
                for b in bytes {
                    hex.push_str(&format!("{b:02x}"));
                }
                format!("'\\x{hex}'::bytea")
            }
        }
    }
}

/// Convert a Data API `Field` value into a [`ParamLiteral`].
pub fn field_to_literal(value: &Value) -> Result<ParamLiteral, AwsError> {
    if value.get("isNull").and_then(|v| v.as_bool()) == Some(true) {
        return Ok(ParamLiteral::Null);
    }
    if let Some(b) = value.get("booleanValue").and_then(|v| v.as_bool()) {
        return Ok(ParamLiteral::Bool(b));
    }
    if let Some(n) = value.get("longValue").and_then(|v| v.as_i64()) {
        return Ok(ParamLiteral::Long(n));
    }
    if let Some(d) = value.get("doubleValue").and_then(|v| v.as_f64()) {
        return Ok(ParamLiteral::Double(d));
    }
    if let Some(s) = value.get("stringValue").and_then(|v| v.as_str()) {
        return Ok(ParamLiteral::Str(s.to_string()));
    }
    if let Some(b64) = value.get("blobValue").and_then(|v| v.as_str()) {
        let bytes = BASE64
            .decode(b64)
            .map_err(|_| bad_request("blobValue must be valid base64."))?;
        return Ok(ParamLiteral::Blob(bytes));
    }
    Err(bad_request(
        "Unsupported parameter value; expected one of isNull, booleanValue, \
         longValue, doubleValue, stringValue, blobValue.",
    ))
}

/// Parse the `parameters` request member into named literals.
pub fn parse_parameters(input: &Value) -> Result<Vec<(String, ParamLiteral)>, AwsError> {
    let Some(params) = input.get("parameters").and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(params.len());
    for param in params {
        let name = param
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| bad_request("Each parameter requires a name."))?;
        let value = param
            .get("value")
            .ok_or_else(|| bad_request(format!("Parameter `{name}` requires a value.")))?;
        out.push((name.to_string(), field_to_literal(value)?));
    }
    Ok(out)
}

/// Substitute `:name` placeholders in `sql` with the corresponding
/// literal. Placeholders inside single-quoted string literals and the
/// `::type` cast operator are left untouched.
pub fn inline_parameters(sql: &str, params: &[(String, ParamLiteral)]) -> Result<String, AwsError> {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    let mut in_string = false;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\'' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '\'' {
            in_string = true;
            out.push(c);
            i += 1;
            continue;
        }
        // Leave the `::` cast operator alone.
        if c == ':' && i + 1 < chars.len() && chars[i + 1] == ':' {
            out.push(':');
            out.push(':');
            i += 2;
            continue;
        }
        if c == ':'
            && i + 1 < chars.len()
            && (chars[i + 1].is_ascii_alphabetic() || chars[i + 1] == '_')
        {
            let mut j = i + 1;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let name: String = chars[i + 1..j].iter().collect();
            let literal = params.iter().find(|(n, _)| n == &name).ok_or_else(|| {
                bad_request(format!("No value supplied for parameter `:{name}`."))
            })?;
            out.push_str(&literal.1.to_sql_literal());
            i = j;
            continue;
        }
        out.push(c);
        i += 1;
    }
    Ok(out)
}

/// Map a column's text-format value to a typed Data API `Field`. The
/// type name is the PostgreSQL type name (`int4`, `bool`, `float8`,
/// `bytea`, and so on); anything else falls back to a string value.
pub fn text_to_field(type_name: &str, value: Option<&str>) -> Value {
    let Some(text) = value else {
        return json!({ "isNull": true });
    };
    match type_name {
        "bool" => json!({ "booleanValue": text == "t" }),
        "int2" | "int4" | "int8" => match text.parse::<i64>() {
            Ok(n) => json!({ "longValue": n }),
            Err(_) => json!({ "stringValue": text }),
        },
        "float4" | "float8" => match text.parse::<f64>() {
            Ok(d) => json!({ "doubleValue": d }),
            Err(_) => json!({ "stringValue": text }),
        },
        "bytea" => match decode_bytea_hex(text) {
            Some(bytes) => json!({ "blobValue": BASE64.encode(bytes) }),
            None => json!({ "stringValue": text }),
        },
        _ => json!({ "stringValue": text }),
    }
}

/// Build a Data API `ColumnMetadata` entry for a result column.
pub fn column_metadata(name: &str, type_oid: i64, type_name: &str) -> Value {
    json!({
        "name": name,
        "label": name,
        "type": type_oid,
        "typeName": type_name,
        "nullable": 2,
        "isCurrency": false,
        "isSigned": matches!(type_name, "int2" | "int4" | "int8" | "float4" | "float8" | "numeric"),
    })
}

/// Decode PostgreSQL `\xDEADBEEF` hex bytea text into raw bytes.
fn decode_bytea_hex(text: &str) -> Option<Vec<u8>> {
    let hex = text.strip_prefix("\\x")?;
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

fn bad_request(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("BadRequestException", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_render_as_sql() {
        assert_eq!(ParamLiteral::Null.to_sql_literal(), "NULL");
        assert_eq!(ParamLiteral::Bool(true).to_sql_literal(), "TRUE");
        assert_eq!(ParamLiteral::Long(42).to_sql_literal(), "42");
        assert_eq!(
            ParamLiteral::Str("a'b".to_string()).to_sql_literal(),
            "'a''b'"
        );
        assert_eq!(
            ParamLiteral::Blob(vec![0xde, 0xad]).to_sql_literal(),
            "'\\xdead'::bytea"
        );
    }

    #[test]
    fn field_to_literal_reads_each_variant() {
        assert_eq!(
            field_to_literal(&json!({ "longValue": 7 })).unwrap(),
            ParamLiteral::Long(7)
        );
        assert_eq!(
            field_to_literal(&json!({ "isNull": true })).unwrap(),
            ParamLiteral::Null
        );
        assert_eq!(
            field_to_literal(&json!({ "stringValue": "hi" })).unwrap(),
            ParamLiteral::Str("hi".to_string())
        );
    }

    #[test]
    fn inline_substitutes_named_parameters() {
        let params = vec![
            ("id".to_string(), ParamLiteral::Long(5)),
            ("name".to_string(), ParamLiteral::Str("Ann".to_string())),
        ];
        let sql = "SELECT * FROM t WHERE id = :id AND name = :name";
        assert_eq!(
            inline_parameters(sql, &params).unwrap(),
            "SELECT * FROM t WHERE id = 5 AND name = 'Ann'"
        );
    }

    #[test]
    fn inline_leaves_cast_operator_and_strings_alone() {
        let params = vec![("x".to_string(), ParamLiteral::Long(1))];
        let sql = "SELECT ':x not a param', val::text FROM t WHERE id = :x";
        assert_eq!(
            inline_parameters(sql, &params).unwrap(),
            "SELECT ':x not a param', val::text FROM t WHERE id = 1"
        );
    }

    #[test]
    fn inline_rejects_missing_parameter() {
        let err = inline_parameters("SELECT :missing", &[]).unwrap_err();
        assert_eq!(err.code, "BadRequestException");
    }

    #[test]
    fn text_to_field_maps_types() {
        assert_eq!(
            text_to_field("int4", Some("12")),
            json!({ "longValue": 12 })
        );
        assert_eq!(
            text_to_field("bool", Some("t")),
            json!({ "booleanValue": true })
        );
        assert_eq!(
            text_to_field("float8", Some("1.5")),
            json!({ "doubleValue": 1.5 })
        );
        assert_eq!(text_to_field("text", None), json!({ "isNull": true }));
        assert_eq!(
            text_to_field("varchar", Some("hi")),
            json!({ "stringValue": "hi" })
        );
        assert_eq!(
            text_to_field("bytea", Some("\\xdead")),
            json!({ "blobValue": BASE64.encode([0xde, 0xad]) })
        );
    }
}
