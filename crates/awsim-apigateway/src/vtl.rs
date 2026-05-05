//! Velocity Template Language (VTL) renderer for API Gateway request and
//! response template mapping.
//!
//! Scope is intentionally tight — handles the interpolation patterns that
//! actually appear in real AWS templates, not full VTL semantics. Specifically:
//!
//!   * `$input.body`, `$input.json('$.path')`, `$input.path('$.path')`,
//!     `$input.params()`, `$input.params('name')`
//!   * `$context.X.Y...` and `$stageVariables.X`
//!   * `$util.escapeJavaScript(s)`, `$util.urlEncode(s)`, `$util.urlDecode(s)`,
//!     `$util.base64Encode(s)`, `$util.base64Decode(s)`, `$util.parseJson(s)`
//!
//! Unrecognized references render as the empty string (matches AWS).
//! Directives like `#set`, `#if`, `#foreach` are not interpreted; lines that
//! start with `#` (after optional whitespace) are dropped.
//!
//! JSONPath support is limited to dot-accessed object fields and bracket
//! indices — `$`, `$.a.b`, `$.a[0].b`. Filter expressions are not supported.

use std::collections::HashMap;

use base64::Engine;
use serde_json::{Value, json};

#[derive(Default, Clone)]
pub struct RenderContext {
    pub body: String,
    pub path_params: HashMap<String, String>,
    pub query_params: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub stage_variables: HashMap<String, String>,
    pub request_context: Value,
}

impl RenderContext {
    fn parsed_body(&self) -> Value {
        if self.body.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&self.body).unwrap_or(Value::Null)
        }
    }

    fn header_lookup(&self, name: &str) -> Option<&String> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v)
    }
}

/// Render a VTL template using the given context. Returns the rendered
/// string. Unmatched references render as the empty string.
pub fn render(template: &str, ctx: &RenderContext) -> String {
    let mut out = String::new();
    let mut i = 0;
    let bytes = template.as_bytes();

    while i < bytes.len() {
        let c = bytes[i];

        // Drop lines whose first non-whitespace char is `#` (directives).
        if at_line_start(template, i) && line_is_directive(&template[i..]) {
            i = skip_line(template, i);
            continue;
        }

        if c == b'$' && i + 1 < bytes.len() {
            if let Some((value, consumed)) = try_parse_reference(&template[i..], ctx) {
                out.push_str(&value);
                i += consumed;
                continue;
            }
            out.push('$');
            i += 1;
            continue;
        }

        // Copy one UTF-8 char.
        let end = next_char_boundary(template, i);
        out.push_str(&template[i..end]);
        i = end;
    }

    out
}

fn at_line_start(s: &str, i: usize) -> bool {
    i == 0 || s.as_bytes()[i - 1] == b'\n'
}

fn line_is_directive(line: &str) -> bool {
    line.trim_start_matches([' ', '\t']).starts_with('#')
}

fn skip_line(s: &str, mut i: usize) -> usize {
    let bytes = s.as_bytes();
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    if i < bytes.len() {
        i += 1;
    }
    i
}

fn next_char_boundary(s: &str, i: usize) -> usize {
    let mut e = i + 1;
    while e < s.len() && !s.is_char_boundary(e) {
        e += 1;
    }
    e
}

/// Try to parse a `$ref` at the start of `s`. Returns the rendered value
/// and the number of bytes consumed. Returns `None` if `s` doesn't start
/// with a parseable reference.
fn try_parse_reference(s: &str, ctx: &RenderContext) -> Option<(String, usize)> {
    let bytes = s.as_bytes();
    debug_assert_eq!(bytes[0], b'$');

    let (start, silent_braces) = if bytes.len() > 1 && bytes[1] == b'{' {
        (2, true)
    } else {
        (1, false)
    };

    let (head, mut after) = read_ident(&s[start..])?;
    after += start;

    let mut chain: Vec<Segment> = Vec::new();
    let bytes = s.as_bytes();

    while after < bytes.len() {
        match bytes[after] {
            b'.' => {
                let (name, end) = match read_ident(&s[after + 1..]) {
                    Some(v) => v,
                    None => break,
                };
                let mut pos = after + 1 + end;
                if pos < bytes.len() && bytes[pos] == b'(' {
                    let (args, args_end) = parse_call_args(&s[pos..])?;
                    pos += args_end;
                    chain.push(Segment::Call(name.to_string(), args));
                } else {
                    chain.push(Segment::Field(name.to_string()));
                }
                after = pos;
            }
            b'(' => {
                let (args, args_end) = parse_call_args(&s[after..])?;
                after += args_end;
                chain.push(Segment::CallOnHead(args));
            }
            _ => break,
        }
    }

    if silent_braces {
        if after >= bytes.len() || bytes[after] != b'}' {
            return None;
        }
        after += 1;
    }

    let value = evaluate(head, &chain, ctx);
    Some((value, after))
}

#[derive(Debug)]
enum Segment {
    Field(String),
    Call(String, Vec<String>),
    CallOnHead(#[allow(dead_code)] Vec<String>),
}

fn read_ident(s: &str) -> Option<(&str, usize)> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    if !is_ident_start(bytes[0]) {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() && is_ident_continue(bytes[i]) {
        i += 1;
    }
    Some((&s[..i], i))
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Parse `(arg1, 'arg2', ...)` starting at `(`. Returns the arg list and
/// the position immediately after the closing `)`. Tracks paren depth so
/// nested calls in unquoted args don't end the outer call early.
fn parse_call_args(s: &str) -> Option<(Vec<String>, usize)> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes[0] != b'(' {
        return None;
    }
    let mut i = 1;
    let mut args: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut depth: u32 = 0;

    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b')' if depth == 0 => {
                if started || !current.is_empty() {
                    args.push(current.trim().to_string());
                }
                return Some((args, i + 1));
            }
            b'(' => {
                started = true;
                current.push('(');
                depth += 1;
                i += 1;
            }
            b')' => {
                started = true;
                current.push(')');
                depth -= 1;
                i += 1;
            }
            b',' if depth == 0 => {
                args.push(current.trim().to_string());
                current.clear();
                started = false;
                i += 1;
            }
            b'\'' | b'"' => {
                let quote = c;
                started = true;
                i += 1;
                let arg_start = i;
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if i >= bytes.len() {
                    return None;
                }
                let raw = &s[arg_start..i];
                current.push_str(&unescape(raw));
                i += 1;
            }
            b' ' | b'\t' | b'\n' | b'\r' if current.is_empty() && !started => {
                i += 1;
            }
            _ => {
                started = true;
                current.push(c as char);
                i += 1;
            }
        }
    }
    None
}

fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'n' => out.push('\n'),
                b't' => out.push('\t'),
                b'r' => out.push('\r'),
                b'\\' => out.push('\\'),
                b'\'' => out.push('\''),
                b'"' => out.push('"'),
                other => {
                    out.push('\\');
                    out.push(other as char);
                }
            }
            i += 2;
        } else {
            let end = next_char_boundary(raw, i);
            out.push_str(&raw[i..end]);
            i = end;
        }
    }
    out
}

fn evaluate(head: &str, chain: &[Segment], ctx: &RenderContext) -> String {
    match head {
        "input" => evaluate_input(chain, ctx),
        "context" => evaluate_dot_chain(&ctx.request_context, chain),
        "stageVariables" => evaluate_stage_variables(chain, ctx),
        "util" => evaluate_util(chain, ctx),
        _ => String::new(),
    }
}

fn evaluate_input(chain: &[Segment], ctx: &RenderContext) -> String {
    let Some(first) = chain.first() else {
        return String::new();
    };
    match first {
        Segment::Field(name) if name == "body" => ctx.body.clone(),
        Segment::Call(name, args) if name == "json" => {
            let path = render_arg(args.first(), ctx);
            let v = json_path(&ctx.parsed_body(), &path);
            value_to_json_string(&v)
        }
        Segment::Call(name, args) if name == "path" => {
            let path = render_arg(args.first(), ctx);
            let v = json_path(&ctx.parsed_body(), &path);
            evaluate_dot_chain(&v, &chain[1..])
        }
        Segment::Call(name, args) if name == "params" => {
            let arg = render_arg(args.first(), ctx);
            if !arg.is_empty() {
                lookup_param(ctx, &arg)
            } else {
                let v = json!({
                    "path": map_to_json(&ctx.path_params),
                    "querystring": map_to_json(&ctx.query_params),
                    "header": map_to_json(&ctx.headers),
                });
                evaluate_dot_chain(&v, &chain[1..])
            }
        }
        _ => String::new(),
    }
}

/// Render a call argument through the engine so that references nested
/// inside the arg (e.g. `$util.escapeJavaScript($input.body)`) resolve.
/// `None` and empty produce the empty string. Strings without a `$`
/// short-circuit to the literal value.
fn render_arg(arg: Option<&String>, ctx: &RenderContext) -> String {
    let Some(a) = arg else {
        return String::new();
    };
    if !a.contains('$') {
        return a.clone();
    }
    render(a, ctx)
}

fn evaluate_stage_variables(chain: &[Segment], ctx: &RenderContext) -> String {
    let Some(Segment::Field(name)) = chain.first() else {
        return String::new();
    };
    ctx.stage_variables.get(name).cloned().unwrap_or_default()
}

fn evaluate_util(chain: &[Segment], ctx: &RenderContext) -> String {
    let Some(Segment::Call(name, args)) = chain.first() else {
        return String::new();
    };
    let arg = render_arg(args.first(), ctx);
    // For `$util.parseJson(...)`, downstream chain access is meaningful.
    let value: Value = match name.as_str() {
        "escapeJavaScript" => Value::String(escape_js(&arg)),
        "urlEncode" => Value::String(url_encode(&arg)),
        "urlDecode" => Value::String(url_decode(&arg)),
        "base64Encode" => {
            Value::String(base64::engine::general_purpose::STANDARD.encode(arg.as_bytes()))
        }
        "base64Decode" => Value::String(
            base64::engine::general_purpose::STANDARD
                .decode(&arg)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .unwrap_or_default(),
        ),
        "parseJson" => {
            // Inner ref like `$input.body` is resolved by the caller before
            // we get here, so `arg` is already a JSON string.
            serde_json::from_str(&arg).unwrap_or(Value::Null)
        }
        _ => return String::new(),
    };
    if chain.len() == 1 {
        match value {
            Value::String(s) => s,
            _ => value.to_string(),
        }
    } else {
        evaluate_dot_chain(&value, &chain[1..])
    }
}

fn evaluate_dot_chain(root: &Value, chain: &[Segment]) -> String {
    let mut current = root.clone();
    for seg in chain {
        match seg {
            Segment::Field(name) => {
                current = current.get(name).cloned().unwrap_or(Value::Null);
            }
            Segment::Call(_, _) | Segment::CallOnHead(_) => {
                return String::new();
            }
        }
    }
    match current {
        Value::Null => String::new(),
        Value::String(s) => s,
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn lookup_param(ctx: &RenderContext, name: &str) -> String {
    if let Some(v) = ctx.path_params.get(name) {
        return v.clone();
    }
    if let Some(v) = ctx.query_params.get(name) {
        return v.clone();
    }
    if let Some(v) = ctx.header_lookup(name) {
        return v.clone();
    }
    String::new()
}

fn map_to_json(map: &HashMap<String, String>) -> Value {
    Value::Object(
        map.iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect(),
    )
}

fn value_to_json_string(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::String(s) => format!("\"{}\"", escape_js(s)),
        other => other.to_string(),
    }
}

/// Walk a dotted JSONPath like `$.a.b[0].c`. Any failure returns `Null`.
fn json_path(root: &Value, path: &str) -> Value {
    let path = path.trim();
    if path == "$" || path.is_empty() {
        return root.clone();
    }
    let path = path
        .strip_prefix("$.")
        .or_else(|| path.strip_prefix('$'))
        .unwrap_or(path);

    let mut current = root.clone();
    let mut i = 0;
    let bytes = path.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'.' {
            i += 1;
            continue;
        }
        if bytes[i] == b'[' {
            let close = match path[i..].find(']') {
                Some(c) => i + c,
                None => return Value::Null,
            };
            let inner = &path[i + 1..close];
            i = close + 1;
            if let Ok(idx) = inner.parse::<usize>() {
                current = current.get(idx).cloned().unwrap_or(Value::Null);
            } else {
                let key = inner.trim_matches(|c| c == '\'' || c == '"');
                current = current.get(key).cloned().unwrap_or(Value::Null);
            }
            continue;
        }
        let mut end = i;
        while end < bytes.len() && bytes[end] != b'.' && bytes[end] != b'[' {
            end += 1;
        }
        let key = &path[i..end];
        current = current.get(key).cloned().unwrap_or(Value::Null);
        i = end;
    }
    current
}

fn escape_js(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        let c = *b;
        if c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c == b'.' || c == b'~' {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    out
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with_body(body: &str) -> RenderContext {
        RenderContext {
            body: body.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn renders_input_body() {
        let ctx = ctx_with_body("hello");
        assert_eq!(render("body=$input.body", &ctx), "body=hello");
    }

    #[test]
    fn renders_input_json_simple_path() {
        let ctx = ctx_with_body(r#"{"name":"alice","n":7}"#);
        assert_eq!(render("$input.json('$.name')", &ctx), "\"alice\"");
        assert_eq!(render("$input.json('$.n')", &ctx), "7");
    }

    #[test]
    fn renders_input_json_missing_returns_null() {
        let ctx = ctx_with_body(r#"{"a":1}"#);
        assert_eq!(render("$input.json('$.b')", &ctx), "null");
    }

    #[test]
    fn renders_input_json_array_index() {
        let ctx = ctx_with_body(r#"{"xs":[10,20,30]}"#);
        assert_eq!(render("$input.json('$.xs[1]')", &ctx), "20");
    }

    #[test]
    fn renders_input_path_dot_chain() {
        let ctx = ctx_with_body(r#"{"user":{"name":"bob"}}"#);
        assert_eq!(render("$input.path('$').user.name", &ctx), "bob");
    }

    #[test]
    fn renders_input_params_named_lookup() {
        let ctx = RenderContext {
            path_params: HashMap::from([("id".into(), "42".into())]),
            headers: HashMap::from([("X-Custom".into(), "v".into())]),
            ..Default::default()
        };
        assert_eq!(render("$input.params('id')", &ctx), "42");
        assert_eq!(render("$input.params('x-custom')", &ctx), "v");
    }

    #[test]
    fn renders_context_dot_chain() {
        let ctx = RenderContext {
            request_context: json!({
                "requestId": "abc-123",
                "identity": {"sourceIp": "10.0.0.1"},
            }),
            ..Default::default()
        };
        assert_eq!(render("$context.requestId", &ctx), "abc-123");
        assert_eq!(render("$context.identity.sourceIp", &ctx), "10.0.0.1");
    }

    #[test]
    fn renders_stage_variables() {
        let ctx = RenderContext {
            stage_variables: HashMap::from([("env".into(), "prod".into())]),
            ..Default::default()
        };
        assert_eq!(render("$stageVariables.env", &ctx), "prod");
        assert_eq!(render("$stageVariables.missing", &ctx), "");
    }

    #[test]
    fn util_escape_javascript_quotes_and_newlines() {
        let ctx = ctx_with_body("a\"b\nc");
        let out = render("$util.escapeJavaScript($input.body)", &ctx);
        assert_eq!(out, "a\\\"b\\nc");
    }

    #[test]
    fn util_url_encode_decode_roundtrip() {
        let ctx = RenderContext::default();
        assert_eq!(render("$util.urlEncode('a b/c')", &ctx), "a%20b%2Fc");
        assert_eq!(render("$util.urlDecode('a%20b')", &ctx), "a b");
    }

    #[test]
    fn util_base64_roundtrip() {
        let ctx = RenderContext::default();
        assert_eq!(render("$util.base64Encode('hi')", &ctx), "aGk=");
        assert_eq!(render("$util.base64Decode('aGk=')", &ctx), "hi");
    }

    #[test]
    fn silent_reference_braces() {
        let ctx = ctx_with_body("hello");
        assert_eq!(render("[${input.body}]", &ctx), "[hello]");
    }

    #[test]
    fn directive_lines_are_dropped() {
        let ctx = ctx_with_body("hello");
        let template = "#set($x = 1)\nbody=$input.body";
        assert_eq!(render(template, &ctx), "body=hello");
    }

    #[test]
    fn unknown_reference_renders_empty() {
        let ctx = RenderContext::default();
        assert_eq!(render("[$unknown.thing]", &ctx), "[]");
    }
}
