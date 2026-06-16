//! Browser-based `PostObject` form uploads.
//!
//! Real S3 lets an HTML form upload an object straight to a bucket with a
//! `multipart/form-data` POST to the bucket root. The form carries the
//! object key, an optional base64 POST policy plus signature fields, any
//! metadata, and finally the file part. This module parses that body,
//! enforces the POST policy, stores the object through the normal
//! [`put_object`](super::object::put_object) path so checksums, encryption,
//! and versioning all apply, and shapes the success response according to
//! the form's `success_action_redirect` or `success_action_status` field.

use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use base64::Engine as _;
use serde_json::{Map, Value, json};

use super::require_str;
use crate::state::S3State;

/// One decoded part of a `multipart/form-data` body.
struct FormPart {
    name: String,
    filename: Option<String>,
    content_type: Option<String>,
    data: Vec<u8>,
}

/// Handle a browser `PostObject` form upload.
pub fn post_object(
    state: &S3State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let bucket_name = require_str(input, "Bucket")?.to_string();

    let raw = input
        .get("__raw_body")
        .and_then(Value::as_str)
        .ok_or_else(|| malformed("The body of the POST request is empty."))?;
    let body = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|_| malformed("The POST request body could not be decoded."))?;

    let parts = parse_multipart_form(&body)?;

    // Split the file part from the ordinary form fields. Field lookups are
    // case-insensitive, matching how S3 treats POST form field names.
    let mut fields: HashMap<String, String> = HashMap::new();
    let mut file: Option<FormPart> = None;
    for part in parts {
        if part.name.eq_ignore_ascii_case("file") {
            file = Some(part);
        } else {
            fields.insert(
                part.name.to_ascii_lowercase(),
                String::from_utf8_lossy(&part.data).into_owned(),
            );
        }
    }
    let file =
        file.ok_or_else(|| malformed("POST requires a file or text content to be provided."))?;

    let key_template = fields.get("key").ok_or_else(|| {
        AwsError::bad_request(
            "InvalidArgument",
            "Bucket POST must contain a field named 'key'. If it is specified, \
             please check the order of the fields.",
        )
    })?;
    let filename = file.filename.clone().unwrap_or_default();
    let key = key_template.replace("${filename}", &filename);
    if key.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidArgument",
            "User key must have a length greater than 0.",
        ));
    }

    // Enforce the POST policy when one is supplied. Anonymous uploads to a
    // permissive bucket may omit it, matching the rest of the emulator's
    // lenient stance on signing.
    if let Some(policy_b64) = fields.get("policy") {
        enforce_post_policy(
            policy_b64,
            &fields,
            &bucket_name,
            &key,
            file.data.len() as u64,
        )?;
    }

    let put_input = build_put_input(&bucket_name, &key, &file, &fields);
    let result = super::object::put_object(state, &put_input, ctx)?;

    let etag = result
        .get("ETag")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let version_id = result
        .get("VersionId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let size = file.data.len() as u64;

    Ok(build_response(
        &fields,
        &bucket_name,
        &key,
        &etag,
        version_id.as_deref(),
        size,
        ctx,
    ))
}

/// Build the [`put_object`](super::object::put_object) input from the parsed
/// form so the upload reuses the standard write path.
fn build_put_input(
    bucket_name: &str,
    key: &str,
    file: &FormPart,
    fields: &HashMap<String, String>,
) -> Value {
    let mut put = Map::new();
    put.insert("Bucket".to_string(), json!(bucket_name));
    put.insert("Key".to_string(), json!(key));
    put.insert(
        "__raw_body".to_string(),
        json!(base64::engine::general_purpose::STANDARD.encode(&file.data)),
    );

    let content_type = fields
        .get("content-type")
        .cloned()
        .or_else(|| file.content_type.clone());
    if let Some(ct) = content_type {
        put.insert("ContentType".to_string(), json!(ct));
    }

    for (field, target) in [
        ("cache-control", "CacheControl"),
        ("content-disposition", "ContentDisposition"),
        ("content-encoding", "ContentEncoding"),
        ("content-language", "ContentLanguage"),
        ("expires", "Expires"),
        ("x-amz-server-side-encryption", "ServerSideEncryption"),
        ("x-amz-server-side-encryption-aws-kms-key-id", "SSEKMSKeyId"),
        ("x-amz-storage-class", "StorageClass"),
    ] {
        if let Some(v) = fields.get(field) {
            put.insert(target.to_string(), json!(v));
        }
    }

    // User metadata: each `x-amz-meta-*` form field becomes a `Meta*` key
    // that the put path turns back into an `x-amz-meta-*` entry.
    for (name, value) in fields {
        if let Some(suffix) = name.strip_prefix("x-amz-meta-") {
            put.insert(format!("Meta{}", pascal_segments(suffix)), json!(value));
        }
    }

    Value::Object(put)
}

/// Shape the success response from `success_action_redirect` or
/// `success_action_status`, defaulting to 204 No Content.
fn build_response(
    fields: &HashMap<String, String>,
    bucket_name: &str,
    key: &str,
    etag: &str,
    version_id: Option<&str>,
    size: u64,
    ctx: &RequestContext,
) -> Value {
    let location = object_url(ctx, bucket_name, key);

    let mut headers = Map::new();
    if let Some(v) = version_id {
        headers.insert("x-amz-version-id".to_string(), json!(v));
    }

    let redirect = fields
        .get("success_action_redirect")
        .or_else(|| fields.get("redirect"))
        .filter(|v| !v.is_empty());

    let mut response = Map::new();
    // Carry the resolved object identity so the dispatcher can emit the
    // matching `s3:ObjectCreated:Post` notification. These sentinel keys
    // never reach the wire.
    response.insert("__notify_key".to_string(), json!(key));
    response.insert("__notify_etag".to_string(), json!(etag));
    response.insert("__notify_size".to_string(), json!(size));

    if let Some(redirect) = redirect {
        let target = append_query(
            redirect,
            &[("bucket", bucket_name), ("key", key), ("etag", etag)],
        );
        headers.insert("location".to_string(), json!(target));
        response.insert("__status_code".to_string(), json!(303));
        response.insert("__headers".to_string(), Value::Object(headers));
        return Value::Object(response);
    }

    headers.insert("location".to_string(), json!(location.clone()));
    headers.insert("etag".to_string(), json!(etag));

    let status = fields
        .get("success_action_status")
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|s| matches!(s, 200 | 201 | 204))
        .unwrap_or(204);

    if status == 201 {
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <PostResponse>\n\
             <Location>{}</Location>\n\
             <Bucket>{}</Bucket>\n\
             <Key>{}</Key>\n\
             <ETag>{}</ETag>\n\
             </PostResponse>",
            xml_escape(&location),
            xml_escape(bucket_name),
            xml_escape(key),
            xml_escape(etag),
        );
        headers.insert("content-type".to_string(), json!("application/xml"));
        response.insert(
            "__raw_body".to_string(),
            json!(base64::engine::general_purpose::STANDARD.encode(xml.as_bytes())),
        );
    }

    response.insert("__status_code".to_string(), json!(status));
    response.insert("__headers".to_string(), Value::Object(headers));
    Value::Object(response)
}

/// Validate the base64 POST policy against the submitted fields.
///
/// The signature itself is not cryptographically verified, consistent with
/// the emulator's lenient signing posture. The enforced conditions are
/// expiration, `content-length-range`, and the `eq` and `starts-with`
/// field matches that applications rely on to scope an upload.
fn enforce_post_policy(
    policy_b64: &str,
    fields: &HashMap<String, String>,
    bucket_name: &str,
    key: &str,
    size: u64,
) -> Result<(), AwsError> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(policy_b64)
        .map_err(|_| malformed("Invalid POST policy: not valid base64."))?;
    let policy: Value = serde_json::from_slice(&decoded)
        .map_err(|_| malformed("Invalid POST policy: not valid JSON."))?;

    if let Some(expiration) = policy.get("expiration").and_then(Value::as_str)
        && let Ok(expiry) = chrono::DateTime::parse_from_rfc3339(expiration)
        && chrono::Utc::now() > expiry.with_timezone(&chrono::Utc)
    {
        return Err(access_denied(
            "Invalid according to Policy: Policy expired.",
        ));
    }

    let Some(conditions) = policy.get("conditions").and_then(Value::as_array) else {
        return Ok(());
    };

    let resolve = |name: &str| -> String {
        let name = name.trim_start_matches('$').to_ascii_lowercase();
        match name.as_str() {
            "bucket" => bucket_name.to_string(),
            "key" => fields
                .get("key")
                .cloned()
                .unwrap_or_else(|| key.to_string()),
            other => fields.get(other).cloned().unwrap_or_default(),
        }
    };

    for condition in conditions {
        match condition {
            // Exact-match object form, e.g. {"bucket": "name"} or {"acl": "private"}.
            Value::Object(map) => {
                for (field, expected) in map {
                    let expected = expected.as_str().unwrap_or_default();
                    if resolve(field) != expected {
                        return Err(policy_violation(field));
                    }
                }
            }
            // Operator form: ["eq"|"starts-with"|"content-length-range", ...].
            Value::Array(items) => {
                let op = items.first().and_then(Value::as_str).unwrap_or_default();
                match op {
                    "content-length-range" => {
                        let min = items.get(1).and_then(Value::as_u64).unwrap_or(0);
                        let max = items.get(2).and_then(Value::as_u64).unwrap_or(u64::MAX);
                        if size < min {
                            return Err(AwsError::bad_request(
                                "EntityTooSmall",
                                "Your proposed upload is smaller than the minimum \
                                 allowed size.",
                            ));
                        }
                        if size > max {
                            return Err(AwsError::bad_request(
                                "EntityTooLarge",
                                "Your proposed upload exceeds the maximum allowed size.",
                            ));
                        }
                    }
                    "eq" => {
                        let field = items.get(1).and_then(Value::as_str).unwrap_or_default();
                        let expected = items.get(2).and_then(Value::as_str).unwrap_or_default();
                        if resolve(field) != expected {
                            return Err(policy_violation(field));
                        }
                    }
                    "starts-with" => {
                        let field = items.get(1).and_then(Value::as_str).unwrap_or_default();
                        let prefix = items.get(2).and_then(Value::as_str).unwrap_or_default();
                        if !resolve(field).starts_with(prefix) {
                            return Err(policy_violation(field));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    Ok(())
}

/// Parse a `multipart/form-data` body into its parts.
///
/// The boundary is read from the opening delimiter line rather than the
/// `Content-Type` header, which is not surfaced to service handlers.
fn parse_multipart_form(body: &[u8]) -> Result<Vec<FormPart>, AwsError> {
    let first_crlf = find_sub(body, b"\r\n")
        .ok_or_else(|| malformed("The POST request body is not valid multipart form data."))?;
    let delim_line = &body[..first_crlf];
    if !delim_line.starts_with(b"--") {
        return Err(malformed(
            "The POST request body is not valid multipart form data.",
        ));
    }
    let separator = delim_line.to_vec();

    let mut parts = Vec::new();
    for segment in split_on(body, &separator).into_iter().skip(1) {
        // The closing delimiter is the separator followed by "--".
        if segment.starts_with(b"--") {
            break;
        }
        let segment = strip_prefix(segment, b"\r\n");
        let segment = strip_suffix(segment, b"\r\n");
        let Some(split) = find_sub(segment, b"\r\n\r\n") else {
            continue;
        };
        let (head, rest) = segment.split_at(split);
        let data = rest[4..].to_vec();

        let mut name = None;
        let mut filename = None;
        let mut content_type = None;
        for line in std::str::from_utf8(head).unwrap_or("").split("\r\n") {
            let Some((header, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            if header.eq_ignore_ascii_case("content-disposition") {
                name = header_param(value, "name");
                filename = header_param(value, "filename");
            } else if header.eq_ignore_ascii_case("content-type") {
                content_type = Some(value.to_string());
            }
        }

        if let Some(name) = name {
            parts.push(FormPart {
                name,
                filename,
                content_type,
                data,
            });
        }
    }

    Ok(parts)
}

/// Extract a quoted parameter such as `name="value"` from a header value.
fn header_param(value: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=\"");
    let start = value.find(&needle)? + needle.len();
    let rest = &value[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Convert a hyphenated metadata suffix to PascalCase so it round-trips
/// through the put path's metadata extraction (`MetaFooBar` ->
/// `x-amz-meta-foo-bar`).
fn pascal_segments(suffix: &str) -> String {
    suffix
        .split('-')
        .map(|seg| {
            let mut chars = seg.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

/// Best-effort object URL for the `Location` header and 201 response body.
fn object_url(ctx: &RequestContext, bucket: &str, key: &str) -> String {
    format!(
        "http://s3.{}.localhost/{}/{}",
        ctx.region,
        bucket,
        encode_path(key)
    )
}

/// Append query parameters to a redirect target, preserving any the target
/// already carries.
fn append_query(base: &str, params: &[(&str, &str)]) -> String {
    let sep = if base.contains('?') { '&' } else { '?' };
    let mut out = base.to_string();
    out.push(sep);
    let encoded: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, encode_component(v)))
        .collect();
    out.push_str(&encoded.join("&"));
    out
}

/// Percent-encode a URL path, leaving the `/` separators intact.
fn encode_path(value: &str) -> String {
    value
        .split('/')
        .map(encode_component)
        .collect::<Vec<_>>()
        .join("/")
}

/// Percent-encode a single URL component, keeping the unreserved set.
fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn malformed(message: &str) -> AwsError {
    AwsError::bad_request("MalformedPOSTRequest", message)
}

fn access_denied(message: impl Into<String>) -> AwsError {
    AwsError::forbidden("AccessDenied", message)
}

fn policy_violation(field: &str) -> AwsError {
    access_denied(format!(
        "Invalid according to Policy: Policy Condition failed: condition on '{field}' did not hold."
    ))
}

// ── Byte-slice helpers ───────────────────────────────────────────────────

/// Find the first index of `needle` within `haystack`.
fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Split `haystack` on every occurrence of `separator`.
fn split_on<'a>(haystack: &'a [u8], separator: &[u8]) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i + separator.len() <= haystack.len() {
        if &haystack[i..i + separator.len()] == separator {
            out.push(&haystack[start..i]);
            i += separator.len();
            start = i;
        } else {
            i += 1;
        }
    }
    out.push(&haystack[start..]);
    out
}

fn strip_prefix<'a>(slice: &'a [u8], prefix: &[u8]) -> &'a [u8] {
    slice.strip_prefix(prefix).unwrap_or(slice)
}

fn strip_suffix<'a>(slice: &'a [u8], suffix: &[u8]) -> &'a [u8] {
    slice.strip_suffix(suffix).unwrap_or(slice)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_with(parts: &[(&str, Option<&str>, Option<&str>, &str)]) -> Vec<u8> {
        // (name, filename, content_type, content)
        let boundary = "----awsimtestboundary";
        let mut out = Vec::new();
        for (name, filename, content_type, content) in parts {
            out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            let disposition = match filename {
                Some(f) => {
                    format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{f}\"")
                }
                None => format!("Content-Disposition: form-data; name=\"{name}\""),
            };
            out.extend_from_slice(disposition.as_bytes());
            out.extend_from_slice(b"\r\n");
            if let Some(ct) = content_type {
                out.extend_from_slice(format!("Content-Type: {ct}\r\n").as_bytes());
            }
            out.extend_from_slice(b"\r\n");
            out.extend_from_slice(content.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        out
    }

    #[test]
    fn parses_fields_and_file() {
        let body = body_with(&[
            ("key", None, None, "uploads/${filename}"),
            ("Content-Type", None, None, "text/plain"),
            ("file", Some("hello.txt"), Some("text/plain"), "hello world"),
        ]);
        let parts = parse_multipart_form(&body).expect("parse");
        assert_eq!(parts.len(), 3);
        let file = parts.iter().find(|p| p.name == "file").unwrap();
        assert_eq!(file.filename.as_deref(), Some("hello.txt"));
        assert_eq!(file.data, b"hello world");
    }

    #[test]
    fn content_length_range_enforced() {
        let policy = json!({
            "conditions": [["content-length-range", 1, 5]]
        });
        let b64 = base64::engine::general_purpose::STANDARD.encode(policy.to_string());
        let fields = HashMap::new();
        let err = enforce_post_policy(&b64, &fields, "b", "k", 100).unwrap_err();
        assert_eq!(err.code, "EntityTooLarge");
        assert!(enforce_post_policy(&b64, &fields, "b", "k", 3).is_ok());
    }

    #[test]
    fn starts_with_condition_enforced() {
        let policy = json!({
            "conditions": [["starts-with", "$key", "uploads/"]]
        });
        let b64 = base64::engine::general_purpose::STANDARD.encode(policy.to_string());
        let mut ok_fields = HashMap::new();
        ok_fields.insert("key".to_string(), "uploads/a.txt".to_string());
        assert!(enforce_post_policy(&b64, &ok_fields, "b", "uploads/a.txt", 1).is_ok());

        let mut bad_fields = HashMap::new();
        bad_fields.insert("key".to_string(), "other/a.txt".to_string());
        let err = enforce_post_policy(&b64, &bad_fields, "b", "other/a.txt", 1).unwrap_err();
        assert_eq!(err.code, "AccessDenied");
    }

    #[test]
    fn expired_policy_rejected() {
        let policy = json!({
            "expiration": "2000-01-01T00:00:00.000Z",
            "conditions": []
        });
        let b64 = base64::engine::general_purpose::STANDARD.encode(policy.to_string());
        let err = enforce_post_policy(&b64, &HashMap::new(), "b", "k", 1).unwrap_err();
        assert_eq!(err.code, "AccessDenied");
    }

    #[test]
    fn pascal_segments_round_trips() {
        assert_eq!(pascal_segments("foo-bar"), "FooBar");
        assert_eq!(pascal_segments("author"), "Author");
    }
}
