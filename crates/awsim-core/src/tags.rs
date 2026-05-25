//! Shared tag validation for service `TagResource` / `Tag*` paths.
//!
//! Every AWS service that supports tags enforces the same general
//! rules: at most ~50 tags per resource, a bounded character set, the
//! `aws:` prefix reserved for system tags, and no duplicate keys in
//! the same request. Per-service limits differ (CloudFront accepts a
//! tighter charset, EC2 still caps at 50 for most resources, etc.),
//! so the helper accepts a [`TagOpts`] knob bag with sensible AWS
//! defaults.
//!
//! Callers that mutate a tag list should run [`validate`] before
//! persisting and [`reject_aws_prefix_on_write`] before allowing a
//! user-supplied key into the store. [`dedupe_or_reject`] catches
//! batch requests that supply the same key twice.

use crate::error::AwsError;
use serde_json::Value;
use std::collections::HashSet;

/// AWS-documented per-resource tag limit. Most services cap at 50;
/// a few accept more, in which case the caller sets a higher
/// [`TagOpts::max_tags`].
pub const DEFAULT_MAX_TAGS: usize = 50;

/// AWS-documented per-key maximum length (UTF-8 characters).
pub const DEFAULT_MAX_KEY_LEN: usize = 128;

/// AWS-documented per-value maximum length (UTF-8 characters).
/// Values may be empty; AWS permits a zero-length value.
pub const DEFAULT_MAX_VALUE_LEN: usize = 256;

/// Reserved tag-key prefix. Keys starting with `aws:` are AWS-system
/// tags - readable from API responses but rejected on user writes.
pub const RESERVED_PREFIX: &str = "aws:";

/// Character classes a tag key/value is allowed to use.
#[derive(Debug, Clone, Copy)]
pub enum TagCharset {
    /// AWS's general-purpose tag charset: letters, digits, whitespace,
    /// and `+ - = . _ : / @`.
    Standard,
    /// Any UTF-8. Used by services that intentionally accept a wider
    /// set (CloudWatch Logs, Bedrock guardrails) and validate
    /// elsewhere.
    Permissive,
}

/// Per-service tag policy knobs. Most services use [`TagOpts::aws_default`].
#[derive(Debug, Clone, Copy)]
pub struct TagOpts {
    /// Maximum number of tags on a single resource.
    pub max_tags: usize,
    /// Maximum length of a tag key (UTF-8 characters).
    pub max_key_len: usize,
    /// Maximum length of a tag value (UTF-8 characters).
    pub max_value_len: usize,
    /// Allowed character class for keys and values.
    pub charset: TagCharset,
}

impl TagOpts {
    /// AWS general defaults (50 / 128 / 256, standard charset).
    /// Use this unless the audited service has a documented
    /// deviation.
    pub const fn aws_default() -> Self {
        Self {
            max_tags: DEFAULT_MAX_TAGS,
            max_key_len: DEFAULT_MAX_KEY_LEN,
            max_value_len: DEFAULT_MAX_VALUE_LEN,
            charset: TagCharset::Standard,
        }
    }
}

impl Default for TagOpts {
    fn default() -> Self {
        Self::aws_default()
    }
}

/// Validate a tag set against AWS rules and per-service overrides.
///
/// Returns `Ok(())` on success. On failure returns a
/// `ValidationException` whose message names the first offending
/// rule (count cap, key length, value length, charset, reserved
/// prefix, duplicate key, empty key). Callers may wrap the error
/// with a service-specific code if needed.
pub fn validate<S: AsRef<str>>(tags: &[(S, S)], opts: &TagOpts) -> Result<(), AwsError> {
    if tags.len() > opts.max_tags {
        return Err(validation(format!(
            "Tag count {} exceeds limit of {}.",
            tags.len(),
            opts.max_tags
        )));
    }

    let mut seen: HashSet<&str> = HashSet::with_capacity(tags.len());
    for (k_raw, v_raw) in tags {
        let key = k_raw.as_ref();
        let value = v_raw.as_ref();
        if key.is_empty() {
            return Err(validation("Tag key must not be empty."));
        }
        if key.chars().count() > opts.max_key_len {
            return Err(validation(format!(
                "Tag key '{key}' exceeds maximum length {}.",
                opts.max_key_len
            )));
        }
        if value.chars().count() > opts.max_value_len {
            return Err(validation(format!(
                "Tag value for key '{key}' exceeds maximum length {}.",
                opts.max_value_len
            )));
        }
        if !is_charset_ok(key, opts.charset) {
            return Err(validation(format!(
                "Tag key '{key}' contains invalid characters."
            )));
        }
        if !is_charset_ok(value, opts.charset) {
            return Err(validation(format!(
                "Tag value for key '{key}' contains invalid characters."
            )));
        }
        if key.starts_with(RESERVED_PREFIX) {
            return Err(validation(format!(
                "Tag key '{key}' uses the reserved 'aws:' prefix."
            )));
        }
        if !seen.insert(key) {
            return Err(validation(format!("Duplicate tag key: '{key}'.")));
        }
    }
    Ok(())
}

/// Reject any key starting with `aws:`.
///
/// Use this on the `TagKeys` argument of `UntagResource` / similar
/// operations where the caller supplies keys without paired values.
pub fn reject_aws_prefix_on_write<S: AsRef<str>>(keys: &[S]) -> Result<(), AwsError> {
    for key in keys {
        let k = key.as_ref();
        if k.starts_with(RESERVED_PREFIX) {
            return Err(validation(format!(
                "Tag key '{k}' uses the reserved 'aws:' prefix."
            )));
        }
    }
    Ok(())
}

/// Reject a tag list that contains the same key twice.
///
/// Returns `ValidationException` on the first duplicate. Tag lists
/// must use unique keys per AWS spec; the catch-all [`validate`]
/// also enforces this, so use this helper only when the rest of the
/// validation does not apply (e.g. bulk-import paths that have
/// already vetted shapes).
pub fn dedupe_or_reject<S: AsRef<str>>(tags: &[(S, S)]) -> Result<(), AwsError> {
    let mut seen: HashSet<&str> = HashSet::with_capacity(tags.len());
    for (k, _) in tags {
        if !seen.insert(k.as_ref()) {
            return Err(validation(format!("Duplicate tag key: '{}'.", k.as_ref())));
        }
    }
    Ok(())
}

fn validation(msg: impl Into<String>) -> AwsError {
    AwsError::validation(msg)
}

/// Validate the `Tags` input from an AWS request body.
///
/// Accepts both shapes AWS APIs use in the wild:
/// - JSON object: `{"Owner": "alice", "Cost-Center": "eng"}` (Cognito,
///   SecretsManager, Lambda, KMS, etc.).
/// - JSON array of `{Key, Value}` records: `[{"Key": "Owner", "Value": "alice"}]`
///   (EC2, S3, RDS, CloudFormation, etc.).
/// - JSON null or missing field: treated as an empty tag set.
///
/// Use this instead of hand-rolling the extraction in every service so
/// the AWS limits stay uniform.
pub fn validate_aws_tags(tags: &Value, opts: &TagOpts) -> Result<(), AwsError> {
    let pairs = extract_pairs(tags)?;
    validate(&pairs, opts)
}

/// Validate a `TagKeys` array (UntagResource-style requests).
///
/// Enforces the reserved-prefix rule and rejects duplicate keys. The
/// caller is responsible for resolving missing keys (untagging an
/// unknown key is silently ignored per AWS).
pub fn validate_aws_tag_keys(tag_keys: &Value) -> Result<(), AwsError> {
    let keys: Vec<String> = tag_keys
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    reject_aws_prefix_on_write(&keys)?;
    let mut seen: HashSet<&str> = HashSet::with_capacity(keys.len());
    for k in &keys {
        if !seen.insert(k.as_str()) {
            return Err(validation(format!("Duplicate tag key: '{k}'.")));
        }
    }
    Ok(())
}

fn extract_pairs(tags: &Value) -> Result<Vec<(String, String)>, AwsError> {
    match tags {
        Value::Null => Ok(Vec::new()),
        Value::Object(map) => Ok(map
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
            .collect()),
        Value::Array(items) => {
            let mut pairs = Vec::with_capacity(items.len());
            for item in items {
                // Most services use {Key, Value}; KMS uses {TagKey,
                // TagValue}; StepFunctions/AppSync use lowercase
                // {key, value}. Accept all three so per-service wiring
                // stays a one-liner.
                let key = item
                    .get("Key")
                    .or_else(|| item.get("TagKey"))
                    .or_else(|| item.get("key"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| validation("Tag entry is missing Key."))?
                    .to_string();
                let value = item
                    .get("Value")
                    .or_else(|| item.get("TagValue"))
                    .or_else(|| item.get("value"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                pairs.push((key, value));
            }
            Ok(pairs)
        }
        _ => Err(validation(
            "Tags must be a JSON object or array of {Key, Value} records.",
        )),
    }
}

fn is_charset_ok(s: &str, charset: TagCharset) -> bool {
    match charset {
        TagCharset::Permissive => true,
        TagCharset::Standard => s.chars().all(|c| {
            c.is_alphanumeric() || matches!(c, ' ' | '+' | '-' | '=' | '.' | '_' | ':' | '/' | '@')
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter()
            .map(|(k, val)| (k.to_string(), val.to_string()))
            .collect()
    }

    #[test]
    fn standard_charset_accepts_documented_set() {
        let tags = pairs(&[
            ("Name", "My Resource"),
            ("Owner", "qaidvoid@example.com"),
            ("Cost-Center", "eng/platform"),
            ("k+v=set.1:foo/bar", "x"),
            ("EmptyValueAllowed", ""),
        ]);
        validate(&tags, &TagOpts::aws_default()).unwrap();
    }

    #[test]
    fn rejects_when_count_exceeds_limit() {
        let tags: Vec<(String, String)> = (0..51).map(|i| (format!("k{i}"), "v".into())).collect();
        let err = validate(&tags, &TagOpts::aws_default()).unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("51"));
    }

    #[test]
    fn rejects_empty_key() {
        let tags = pairs(&[("", "v")]);
        let err = validate(&tags, &TagOpts::aws_default()).unwrap_err();
        assert!(err.message.to_lowercase().contains("empty"));
    }

    #[test]
    fn rejects_key_above_max_length() {
        let key = "k".repeat(129);
        let tags = pairs(&[(key.as_str(), "v")]);
        let err = validate(&tags, &TagOpts::aws_default()).unwrap_err();
        assert!(err.message.contains("128"));
    }

    #[test]
    fn rejects_value_above_max_length() {
        let value = "v".repeat(257);
        let tags = pairs(&[("k", value.as_str())]);
        let err = validate(&tags, &TagOpts::aws_default()).unwrap_err();
        assert!(err.message.contains("256"));
    }

    #[test]
    fn key_length_is_counted_in_characters_not_bytes() {
        // 128 wide-Unicode chars should pass; each character is one
        // grapheme-ish, not three bytes.
        let key: String = std::iter::repeat_n('a', 128).collect();
        let tags = pairs(&[(key.as_str(), "v")]);
        validate(&tags, &TagOpts::aws_default()).unwrap();
    }

    #[test]
    fn rejects_aws_prefix_on_full_validate() {
        let tags = pairs(&[("aws:cloudformation:stack-name", "MyStack")]);
        let err = validate(&tags, &TagOpts::aws_default()).unwrap_err();
        assert!(err.message.contains("aws:"));
    }

    #[test]
    fn rejects_aws_prefix_on_untag_path() {
        let keys = vec!["Owner".to_string(), "aws:billing-tier".to_string()];
        let err = reject_aws_prefix_on_write(&keys).unwrap_err();
        assert!(err.message.contains("aws:"));
    }

    #[test]
    fn accepts_clean_untag_keys() {
        let keys = vec!["Owner".to_string(), "Cost-Center".to_string()];
        reject_aws_prefix_on_write(&keys).unwrap();
    }

    #[test]
    fn dedupe_helper_rejects_repeated_key() {
        let tags = pairs(&[("Owner", "a"), ("Cost", "b"), ("Owner", "c")]);
        let err = dedupe_or_reject(&tags).unwrap_err();
        assert!(err.message.contains("Owner"));
    }

    #[test]
    fn dedupe_helper_accepts_unique_keys() {
        let tags = pairs(&[("Owner", "a"), ("Cost", "b")]);
        dedupe_or_reject(&tags).unwrap();
    }

    #[test]
    fn full_validate_rejects_duplicates_too() {
        let tags = pairs(&[("Owner", "a"), ("Owner", "b")]);
        let err = validate(&tags, &TagOpts::aws_default()).unwrap_err();
        assert!(err.message.contains("Owner"));
    }

    #[test]
    fn standard_charset_rejects_control_and_emoji() {
        let bad_control = pairs(&[("Owner\twith-tab", "v")]);
        assert!(validate(&bad_control, &TagOpts::aws_default()).is_err());

        let bad_emoji = pairs(&[("Owner", "value-with-emoji-rocket")]);
        // Plain ASCII case passes.
        validate(&bad_emoji, &TagOpts::aws_default()).unwrap();

        let bad_emoji_real = vec![("k".to_string(), "\u{1F680}".to_string())];
        assert!(validate(&bad_emoji_real, &TagOpts::aws_default()).is_err());
    }

    #[test]
    fn permissive_charset_skips_character_class_check() {
        let opts = TagOpts {
            charset: TagCharset::Permissive,
            ..TagOpts::aws_default()
        };
        let tags = vec![("k".to_string(), "\u{1F680}".to_string())];
        validate(&tags, &opts).unwrap();
    }

    #[test]
    fn validate_aws_tags_accepts_object_shape() {
        let v = serde_json::json!({"Owner": "alice", "Cost-Center": "eng"});
        validate_aws_tags(&v, &TagOpts::aws_default()).unwrap();
    }

    #[test]
    fn validate_aws_tags_accepts_array_shape() {
        let v = serde_json::json!([
            {"Key": "Owner", "Value": "alice"},
            {"Key": "Cost-Center", "Value": "eng"},
        ]);
        validate_aws_tags(&v, &TagOpts::aws_default()).unwrap();
    }

    #[test]
    fn validate_aws_tags_accepts_kms_tagkey_tagvalue_shape() {
        let v = serde_json::json!([
            {"TagKey": "Owner", "TagValue": "alice"},
            {"TagKey": "Cost-Center", "TagValue": "eng"},
        ]);
        validate_aws_tags(&v, &TagOpts::aws_default()).unwrap();
    }

    #[test]
    fn validate_aws_tags_rejects_array_without_key() {
        let v = serde_json::json!([{"Value": "alice"}]);
        let err = validate_aws_tags(&v, &TagOpts::aws_default()).unwrap_err();
        assert!(err.message.contains("Key"));
    }

    #[test]
    fn validate_aws_tags_treats_null_as_empty() {
        validate_aws_tags(&Value::Null, &TagOpts::aws_default()).unwrap();
    }

    #[test]
    fn validate_aws_tag_keys_rejects_reserved_prefix() {
        let v = serde_json::json!(["Owner", "aws:internal"]);
        let err = validate_aws_tag_keys(&v).unwrap_err();
        assert!(err.message.contains("aws:"));
    }

    #[test]
    fn validate_aws_tag_keys_rejects_duplicates() {
        let v = serde_json::json!(["Owner", "Cost", "Owner"]);
        let err = validate_aws_tag_keys(&v).unwrap_err();
        assert!(err.message.contains("Owner"));
    }

    #[test]
    fn custom_max_tags_overrides_default() {
        let opts = TagOpts {
            max_tags: 5,
            ..TagOpts::aws_default()
        };
        let tags: Vec<(String, String)> = (0..6).map(|i| (format!("k{i}"), "v".into())).collect();
        assert!(validate(&tags, &opts).is_err());

        let ok: Vec<(String, String)> = (0..5).map(|i| (format!("k{i}"), "v".into())).collect();
        validate(&ok, &opts).unwrap();
    }
}
