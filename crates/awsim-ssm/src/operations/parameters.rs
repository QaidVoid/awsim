use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::{debug, info};

use crate::state::{Parameter, ParameterVersion, SsmState};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the current time as Unix epoch seconds.
///
/// The AWS SDK for SSM deserialises `LastModifiedDate` as a JSON number (f64).
/// Storing and emitting it as a `u64` integer satisfies the SDK's expectation.
pub fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn build_arn(ctx: &RequestContext, name: &str) -> String {
    // Name may or may not start with /; ARN always has /name
    let normalized = if name.starts_with('/') {
        name.to_string()
    } else {
        format!("/{name}")
    };
    format!(
        "arn:aws:ssm:{}:{}:parameter{}",
        ctx.region, ctx.account_id, normalized
    )
}

fn validate_param_type(param_type: &str) -> Result<(), AwsError> {
    match param_type {
        "String" | "StringList" | "SecureString" => Ok(()),
        _ => Err(AwsError::bad_request(
            "InvalidParameterType",
            format!(
                "Invalid parameter type: {param_type}. Must be String, StringList, or SecureString"
            ),
        )),
    }
}

fn parameter_to_value(p: &Parameter) -> Value {
    json!({
        "Name": p.name,
        "Type": p.param_type,
        "Value": p.value,
        "Version": p.version,
        "LastModifiedDate": p.last_modified_date,
        "ARN": p.arn,
        "DataType": "text",
    })
}

fn parameter_metadata(p: &Parameter) -> Value {
    json!({
        "Name": p.name,
        "Type": p.param_type,
        "Version": p.version,
        "LastModifiedDate": p.last_modified_date,
        "ARN": p.arn,
        "Description": p.description,
        "Tier": p.tier,
        "DataType": "text",
    })
}

// ---------------------------------------------------------------------------
// PutParameter
// ---------------------------------------------------------------------------

pub fn put_parameter(
    state: &SsmState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    let value = input["Value"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Value is required"))?;

    let tier = input["Tier"].as_str().unwrap_or("Standard");
    if !matches!(tier, "Standard" | "Advanced" | "Intelligent-Tiering") {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            format!("Tier '{tier}' must be Standard, Advanced, or Intelligent-Tiering."),
        ));
    }
    // AWS-documented per-value byte cap. Standard tier is 4 KiB,
    // Advanced (and Intelligent-Tiering when it promotes) is 8 KiB.
    // Over-limit values come back as ValidationException at the API
    // boundary; without the check, callers can persist values here
    // that real SSM would refuse on the same call.
    let max_value_bytes = match tier {
        "Standard" => 4 * 1024,
        _ => 8 * 1024,
    };
    if value.len() > max_value_bytes {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "Parameter value is {} bytes; the maximum for tier '{tier}' is {max_value_bytes}.",
                value.len()
            ),
        ));
    }

    let param_type = input["Type"].as_str().unwrap_or("String");
    validate_param_type(param_type)?;

    // AWS optionally tags the parameter's `DataType` (separate from
    // `Type`). Allowed values: `text` (default), `aws:ec2:image`,
    // `aws:ssm:integration`, plus list variants. `aws:ec2:image`
    // additionally validates that Value looks like an AMI id when
    // Type=String.
    let data_type = input["DataType"].as_str().unwrap_or("text");
    if !matches!(data_type, "text" | "aws:ec2:image" | "aws:ssm:integration") {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("DataType `{data_type}` must be text, aws:ec2:image, or aws:ssm:integration."),
        ));
    }
    if data_type == "aws:ec2:image" && !value.starts_with("ami-") {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("DataType aws:ec2:image requires Value to start with `ami-`; got `{value}`."),
        ));
    }

    // AWS optionally validates Value against AllowedPattern at
    // PutParameter time. Reject malformed regex and non-matching
    // values with ValidationException to match the real API.
    if let Some(pattern) = input["AllowedPattern"].as_str()
        && !pattern.is_empty()
    {
        let re = regex::Regex::new(pattern).map_err(|e| {
            AwsError::bad_request(
                "ValidationException",
                format!("AllowedPattern `{pattern}` is not a valid regular expression: {e}"),
            )
        })?;
        if !re.is_match(value) {
            return Err(AwsError::bad_request(
                "ValidationException",
                format!("Parameter value does not match the AllowedPattern `{pattern}`."),
            ));
        }
    }

    let description = input["Description"].as_str().unwrap_or("").to_string();
    let overwrite = input["Overwrite"].as_bool().unwrap_or(false);

    let mut tags: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Some(tag_list) = input["Tags"].as_array() {
        for tag in tag_list {
            if let (Some(k), Some(v)) = (tag["Key"].as_str(), tag["Value"].as_str()) {
                tags.insert(k.to_string(), v.to_string());
            }
        }
    }

    let now = now_epoch_secs();
    let arn = build_arn(ctx, name);

    if let Some(mut existing) = state.parameters.get_mut(name) {
        if !overwrite {
            return Err(AwsError::bad_request(
                "ParameterAlreadyExists",
                format!("Parameter {name} already exists. Use Overwrite to update."),
            ));
        }
        // Record history entry for the previous value
        let prev_version = ParameterVersion {
            value: existing.value.clone(),
            version: existing.version,
            date: existing.last_modified_date,
            description: existing.description.clone(),
            labels: existing.labels.clone(),
        };
        existing.history.push(prev_version);

        existing.version += 1;
        existing.value = value.to_string();
        existing.param_type = param_type.to_string();
        existing.description = description;
        existing.last_modified_date = now;
        existing.labels.clear();
        // Merge tags if provided
        if !tags.is_empty() {
            existing.tags.extend(tags);
        }

        existing.tier = tier.to_string();
        let version = existing.version;
        info!(name, version, "Updated parameter");
        return Ok(json!({ "Version": version, "Tier": tier }));
    }

    let param = Parameter {
        name: name.to_string(),
        arn,
        param_type: param_type.to_string(),
        value: value.to_string(),
        description,
        version: 1,
        last_modified_date: now,
        tags,
        history: Vec::new(),
        tier: tier.to_string(),
        labels: Vec::new(),
    };

    info!(name, "Created parameter");
    state.parameters.insert(name.to_string(), param);

    Ok(json!({ "Version": 1, "Tier": tier }))
}

// ---------------------------------------------------------------------------
// GetParameter
// ---------------------------------------------------------------------------

pub fn get_parameter(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    let param = state.parameters.get(name).ok_or_else(|| {
        AwsError::bad_request("ParameterNotFound", format!("Parameter {name} not found"))
    })?;

    debug!(name, "GetParameter");
    Ok(json!({ "Parameter": parameter_to_value(&param) }))
}

// ---------------------------------------------------------------------------
// GetParameters
// ---------------------------------------------------------------------------

pub fn get_parameters(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let names = input["Names"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Names is required"))?;

    let mut parameters: Vec<Value> = Vec::new();
    let mut invalid: Vec<Value> = Vec::new();

    for name_val in names {
        let name = name_val.as_str().unwrap_or("");
        match state.parameters.get(name) {
            Some(p) => parameters.push(parameter_to_value(&p)),
            None => invalid.push(json!(name)),
        }
    }

    Ok(json!({
        "Parameters": parameters,
        "InvalidParameters": invalid,
    }))
}

// ---------------------------------------------------------------------------
// GetParametersByPath
// ---------------------------------------------------------------------------

pub fn get_parameters_by_path(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let path = input["Path"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Path is required"))?;

    let recursive = input["Recursive"].as_bool().unwrap_or(false);
    let max_results = input["MaxResults"].as_u64().unwrap_or(10) as usize;

    // Normalize: ensure path ends with /
    let prefix = if path.ends_with('/') {
        path.to_string()
    } else {
        format!("{path}/")
    };

    let mut parameters: Vec<Value> = state
        .parameters
        .iter()
        .filter(|entry| {
            let name = entry.key();
            if !name.starts_with(&prefix) {
                return false;
            }
            if recursive {
                return true;
            }
            // Non-recursive: only direct children — no additional slashes after prefix
            let suffix = &name[prefix.len()..];
            !suffix.contains('/')
        })
        .map(|entry| parameter_to_value(entry.value()))
        .take(max_results)
        .collect();

    // Stable sort by name
    parameters.sort_by(|a, b| {
        a["Name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["Name"].as_str().unwrap_or(""))
    });

    Ok(json!({ "Parameters": parameters }))
}

// ---------------------------------------------------------------------------
// DeleteParameter
// ---------------------------------------------------------------------------

pub fn delete_parameter(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    if state.parameters.remove(name).is_none() {
        return Err(AwsError::bad_request(
            "ParameterNotFound",
            format!("Parameter {name} not found"),
        ));
    }

    info!(name, "Deleted parameter");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DeleteParameters
// ---------------------------------------------------------------------------

pub fn delete_parameters(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let names = input["Names"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Names is required"))?;

    let mut deleted: Vec<Value> = Vec::new();
    let mut invalid: Vec<Value> = Vec::new();

    for name_val in names {
        let name = name_val.as_str().unwrap_or("");
        if state.parameters.remove(name).is_some() {
            deleted.push(json!(name));
        } else {
            invalid.push(json!(name));
        }
    }

    Ok(json!({
        "DeletedParameters": deleted,
        "InvalidParameters": invalid,
    }))
}

// ---------------------------------------------------------------------------
// DescribeParameters
// ---------------------------------------------------------------------------

pub fn describe_parameters(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let max_results = input["MaxResults"].as_u64().unwrap_or(50) as usize;

    // AWS exposes two filter-shape parameters. `Filters` (legacy) only
    // accepts Name / Type / KeyId / Tag with Equals semantics, while
    // `ParameterFilters` (newer) honors Option (Equals / BeginsWith /
    // Contains / Recursive / OneLevel) across a wider key set. Honor
    // both shapes — both can be supplied per AWS docs.
    let legacy_filters = input["Filters"].as_array().cloned().unwrap_or_default();
    let new_filters = input["ParameterFilters"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut params: Vec<Value> = state
        .parameters
        .iter()
        .filter(|entry| {
            let name = entry.key().as_str();
            let p = entry.value();
            for f in &legacy_filters {
                if !legacy_filter_match(f, name, p) {
                    return false;
                }
            }
            for f in &new_filters {
                if !parameter_filter_match(f, name, p) {
                    return false;
                }
            }
            true
        })
        .map(|entry| parameter_metadata(entry.value()))
        .take(max_results)
        .collect();

    params.sort_by(|a, b| {
        a["Name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["Name"].as_str().unwrap_or(""))
    });

    Ok(json!({ "Parameters": params }))
}

/// Legacy `Filters[]` keys: Name / Type (KeyId / Tag pass-through since
/// we don't persist them today).
fn legacy_filter_match(f: &Value, name: &str, p: &Parameter) -> bool {
    let key = f.get("Key").and_then(Value::as_str).unwrap_or("");
    let values = f
        .get("Values")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    match key {
        "Name" => values.iter().any(|v| v == name),
        "Type" => values.iter().any(|v| v == &p.param_type),
        _ => true,
    }
}

/// Newer `ParameterFilters[]` shape: { Key, Option, Values[] }. Option
/// defaults to `Equals`; some keys accept `BeginsWith` / `Contains` and
/// `Path` accepts `Recursive` / `OneLevel`.
fn parameter_filter_match(f: &Value, name: &str, p: &Parameter) -> bool {
    let key = f.get("Key").and_then(Value::as_str).unwrap_or("");
    let option = f.get("Option").and_then(Value::as_str).unwrap_or("Equals");
    let values: Vec<&str> = f
        .get("Values")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let str_match = |field: &str| match option {
        "Equals" => values.contains(&field),
        "BeginsWith" => values.iter().any(|v| field.starts_with(v)),
        "Contains" => values.iter().any(|v| field.contains(v)),
        _ => true,
    };
    match key {
        "Name" => str_match(name),
        "Type" => str_match(&p.param_type),
        "Tier" => str_match(&p.tier),
        "DataType" => str_match("text"),
        "Label" => values.iter().any(|v| p.labels.iter().any(|l| l == v)),
        "Path" => {
            // Path filter uses Option = Recursive | OneLevel; the
            // Values list is the path prefix to match against.
            let prefix = values.first().copied().unwrap_or("");
            if !name.starts_with(prefix) {
                return false;
            }
            if option == "OneLevel" {
                // The remainder after `prefix/` must not contain another `/`.
                let rest = name
                    .strip_prefix(prefix)
                    .unwrap_or("")
                    .trim_start_matches('/');
                !rest.contains('/')
            } else {
                true
            }
        }
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// GetParameterHistory
// ---------------------------------------------------------------------------

pub fn get_parameter_history(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    let param = state.parameters.get(name).ok_or_else(|| {
        AwsError::bad_request("ParameterNotFound", format!("Parameter {name} not found"))
    })?;

    // Build history: all previous versions + current
    let mut history: Vec<Value> = param
        .history
        .iter()
        .map(|h| {
            json!({
                "Name": param.name,
                "Type": param.param_type,
                "Value": h.value,
                "Version": h.version,
                "LastModifiedDate": h.date,
                "Description": h.description,
                "Labels": h.labels,
            })
        })
        .collect();

    // Append current version
    history.push(json!({
        "Name": param.name,
        "Type": param.param_type,
        "Value": param.value,
        "Version": param.version,
        "LastModifiedDate": param.last_modified_date,
        "Description": param.description,
        "Labels": param.labels,
    }));

    Ok(json!({ "Parameters": history }))
}

// ---------------------------------------------------------------------------
// LabelParameterVersion
// ---------------------------------------------------------------------------

pub fn label_parameter_version(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    let raw_labels: Vec<String> = input["Labels"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Labels is required"))?
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    // AWS caps the request at 10 labels per call.
    if raw_labels.len() > 10 {
        return Err(AwsError::bad_request(
            "ParameterVersionLabelLimitExceeded",
            format!(
                "A maximum of 10 labels may be supplied per LabelParameterVersion call \
                 ({} supplied).",
                raw_labels.len()
            ),
        ));
    }

    // Partition labels into accepted (passes shape checks) vs invalid
    // (reported in `InvalidLabels`). AWS does not error the whole call
    // when some labels are invalid; the caller is expected to inspect
    // InvalidLabels.
    let mut labels: Vec<String> = Vec::new();
    let mut invalid: Vec<String> = Vec::new();
    for label in &raw_labels {
        if is_valid_label(label) {
            labels.push(label.clone());
        } else {
            invalid.push(label.clone());
        }
    }

    let requested_version = input["ParameterVersion"].as_u64();

    let mut param = state.parameters.get_mut(name).ok_or_else(|| {
        AwsError::bad_request("ParameterNotFound", format!("Parameter {name} not found"))
    })?;

    // Determine the target version up front so we know where to attach
    // labels and which other versions to strip them from.
    let target_version = match requested_version {
        Some(ver) => {
            if ver != param.version && !param.history.iter().any(|h| h.version == ver) {
                return Err(AwsError::bad_request(
                    "ParameterVersionNotFound",
                    format!("Version {ver} of parameter {name} not found"),
                ));
            }
            ver
        }
        None => param.version,
    };

    // AWS allows a label to live on exactly one version of a parameter
    // — calling LabelParameterVersion with the same label moves it from
    // wherever it was before to the requested version. Strip the labels
    // from every other version (current + history) before we add them
    // to the target.
    if param.version != target_version {
        param.labels.retain(|l| !labels.contains(l));
    }
    for h in &mut param.history {
        if h.version != target_version {
            h.labels.retain(|l| !labels.contains(l));
        }
    }

    if target_version == param.version {
        for label in &labels {
            if !param.labels.contains(label) {
                param.labels.push(label.clone());
            }
        }
    } else if let Some(h) = param
        .history
        .iter_mut()
        .find(|h| h.version == target_version)
    {
        for label in &labels {
            if !h.labels.contains(label) {
                h.labels.push(label.clone());
            }
        }
    }

    Ok(json!({
        "InvalidLabels": invalid,
        "ParameterVersion": target_version,
    }))
}

/// Validate a Parameter Store label against AWS's documented constraints:
///   * length 1..=100
///   * `[A-Za-z0-9_.-]+` (no whitespace, no slashes)
///   * may not start with `aws` (case-insensitive) or `ssm`
///   * may not start with a digit, period, or hyphen
fn is_valid_label(label: &str) -> bool {
    if !(1..=100).contains(&label.len()) {
        return false;
    }
    let lower = label.to_ascii_lowercase();
    if lower.starts_with("aws") || lower.starts_with("ssm") {
        return false;
    }
    let mut chars = label.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    label
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

#[cfg(test)]
mod label_parameter_tests {
    use super::*;

    #[test]
    fn valid_labels_pass() {
        assert!(is_valid_label("prod"));
        assert!(is_valid_label("blue-green"));
        assert!(is_valid_label("Release_1.2"));
        assert!(is_valid_label("_internal"));
    }

    #[test]
    fn invalid_labels_rejected() {
        assert!(!is_valid_label(""));
        assert!(!is_valid_label("aws-reserved"));
        assert!(!is_valid_label("AWS-Foo"));
        assert!(!is_valid_label("ssm-thing"));
        assert!(!is_valid_label("1prod"));
        assert!(!is_valid_label("-prod"));
        assert!(!is_valid_label("with space"));
        assert!(!is_valid_label("with/slash"));
    }

    fn ctx() -> awsim_core::RequestContext {
        awsim_core::RequestContext::new("ssm", "us-east-1")
    }

    fn make_param(state: &SsmState, name: &str, n_versions: u64) {
        for i in 1..=n_versions {
            put_parameter(
                state,
                &json!({
                    "Name": name,
                    "Value": format!("v{i}"),
                    "Type": "String",
                    "Overwrite": i > 1,
                }),
                &ctx(),
            )
            .unwrap();
        }
    }

    #[test]
    fn label_attaches_to_current_version_when_no_version_specified() {
        let state = SsmState::default();
        make_param(&state, "p1", 2);
        let resp =
            label_parameter_version(&state, &json!({ "Name": "p1", "Labels": ["prod"] }), &ctx())
                .unwrap();
        assert_eq!(resp["ParameterVersion"], 2);
        assert_eq!(resp["InvalidLabels"].as_array().unwrap().len(), 0);
        assert!(
            state
                .parameters
                .get("p1")
                .unwrap()
                .labels
                .contains(&"prod".to_string())
        );
    }

    #[test]
    fn label_moves_between_versions() {
        let state = SsmState::default();
        make_param(&state, "p2", 3);
        // Attach to version 2
        label_parameter_version(
            &state,
            &json!({ "Name": "p2", "Labels": ["prod"], "ParameterVersion": 2 }),
            &ctx(),
        )
        .unwrap();
        // Now move it to the current version (3)
        label_parameter_version(&state, &json!({ "Name": "p2", "Labels": ["prod"] }), &ctx())
            .unwrap();
        let p = state.parameters.get("p2").unwrap();
        assert!(
            p.labels.contains(&"prod".to_string()),
            "label must end up on current version"
        );
        let v2 = p.history.iter().find(|h| h.version == 2).unwrap();
        assert!(
            !v2.labels.contains(&"prod".to_string()),
            "label must be stripped from prior version"
        );
    }

    #[test]
    fn invalid_labels_returned_without_failing_call() {
        let state = SsmState::default();
        make_param(&state, "p3", 1);
        let resp = label_parameter_version(
            &state,
            &json!({ "Name": "p3", "Labels": ["good", "aws-bad", "1bad"] }),
            &ctx(),
        )
        .unwrap();
        let invalid = resp["InvalidLabels"].as_array().unwrap();
        assert_eq!(invalid.len(), 2);
        assert!(
            state
                .parameters
                .get("p3")
                .unwrap()
                .labels
                .contains(&"good".to_string())
        );
    }

    #[test]
    fn rejects_more_than_10_labels_per_call() {
        let state = SsmState::default();
        make_param(&state, "p4", 1);
        let labels: Vec<String> = (0..11).map(|i| format!("l{i}")).collect();
        let err =
            label_parameter_version(&state, &json!({ "Name": "p4", "Labels": labels }), &ctx())
                .unwrap_err();
        assert_eq!(err.code, "ParameterVersionLabelLimitExceeded");
    }

    #[test]
    fn unknown_version_returns_parameter_version_not_found() {
        let state = SsmState::default();
        make_param(&state, "p5", 1);
        let err = label_parameter_version(
            &state,
            &json!({ "Name": "p5", "Labels": ["x"], "ParameterVersion": 99 }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ParameterVersionNotFound");
    }
}

#[cfg(test)]
mod describe_parameters_tests {
    use super::*;

    fn ctx() -> awsim_core::RequestContext {
        awsim_core::RequestContext::new("ssm", "us-east-1")
    }

    fn seed(state: &SsmState) {
        for (name, tier) in [
            ("/prod/api/url", "Standard"),
            ("/prod/api/key", "Advanced"),
            ("/dev/api/url", "Standard"),
        ] {
            put_parameter(
                state,
                &json!({
                    "Name": name,
                    "Value": "v",
                    "Type": "String",
                    "Tier": tier,
                }),
                &ctx(),
            )
            .unwrap();
        }
    }

    fn names(resp: &Value) -> Vec<String> {
        resp["Parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["Name"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn parameter_filter_path_recursive_returns_full_subtree() {
        let state = SsmState::default();
        seed(&state);
        let resp = describe_parameters(
            &state,
            &json!({
                "ParameterFilters": [
                    { "Key": "Path", "Option": "Recursive", "Values": ["/prod"] }
                ]
            }),
            &ctx(),
        )
        .unwrap();
        let ns = names(&resp);
        assert_eq!(ns.len(), 2);
        assert!(ns.iter().all(|n| n.starts_with("/prod")));
    }

    #[test]
    fn parameter_filter_path_one_level_only_top_children() {
        let state = SsmState::default();
        seed(&state);
        let resp = describe_parameters(
            &state,
            &json!({
                "ParameterFilters": [
                    { "Key": "Path", "Option": "OneLevel", "Values": ["/prod/api"] }
                ]
            }),
            &ctx(),
        )
        .unwrap();
        let ns = names(&resp);
        assert_eq!(ns.len(), 2);
    }

    #[test]
    fn parameter_filter_tier_equals() {
        let state = SsmState::default();
        seed(&state);
        let resp = describe_parameters(
            &state,
            &json!({
                "ParameterFilters": [
                    { "Key": "Tier", "Option": "Equals", "Values": ["Advanced"] }
                ]
            }),
            &ctx(),
        )
        .unwrap();
        let ns = names(&resp);
        assert_eq!(ns, vec!["/prod/api/key"]);
    }

    #[test]
    fn parameter_filter_name_begins_with() {
        let state = SsmState::default();
        seed(&state);
        let resp = describe_parameters(
            &state,
            &json!({
                "ParameterFilters": [
                    { "Key": "Name", "Option": "BeginsWith", "Values": ["/dev"] }
                ]
            }),
            &ctx(),
        )
        .unwrap();
        let ns = names(&resp);
        assert_eq!(ns, vec!["/dev/api/url"]);
    }
}
