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
            format!("Invalid parameter type: {param_type}. Must be String, StringList, or SecureString"),
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

    let param_type = input["Type"].as_str().unwrap_or("String");
    validate_param_type(param_type)?;

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
            return Err(AwsError::conflict(
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

        let version = existing.version;
        info!(name, version, "Updated parameter");
        return Ok(json!({ "Version": version, "Tier": "Standard" }));
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
        tier: "Standard".to_string(),
        labels: Vec::new(),
    };

    info!(name, "Created parameter");
    state.parameters.insert(name.to_string(), param);

    Ok(json!({ "Version": 1, "Tier": "Standard" }))
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
        AwsError::not_found(
            "ParameterNotFound",
            format!("Parameter {name} not found"),
        )
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
        a["Name"].as_str().unwrap_or("").cmp(b["Name"].as_str().unwrap_or(""))
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
        return Err(AwsError::not_found(
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

    // Optional filters by Name or Type
    let filters = input["Filters"].as_array();

    let mut params: Vec<Value> = state
        .parameters
        .iter()
        .filter(|entry| {
            if let Some(filter_arr) = filters {
                for f in filter_arr {
                    let key = f["Key"].as_str().unwrap_or("");
                    let values = f["Values"].as_array();
                    match key {
                        "Name" => {
                            if let Some(vals) = values {
                                let name = entry.key();
                                if !vals.iter().any(|v| v.as_str() == Some(name.as_str())) {
                                    return false;
                                }
                            }
                        }
                        "Type" => {
                            if let Some(vals) = values {
                                let ptype = &entry.value().param_type;
                                if !vals.iter().any(|v| v.as_str() == Some(ptype.as_str())) {
                                    return false;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            true
        })
        .map(|entry| parameter_metadata(entry.value()))
        .take(max_results)
        .collect();

    params.sort_by(|a, b| {
        a["Name"].as_str().unwrap_or("").cmp(b["Name"].as_str().unwrap_or(""))
    });

    Ok(json!({ "Parameters": params }))
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
        AwsError::not_found(
            "ParameterNotFound",
            format!("Parameter {name} not found"),
        )
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

    let labels: Vec<String> = input["Labels"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Labels is required"))?
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    let requested_version = input["ParameterVersion"].as_u64();

    let mut param = state.parameters.get_mut(name).ok_or_else(|| {
        AwsError::not_found("ParameterNotFound", format!("Parameter {name} not found"))
    })?;

    // If a specific version is given, apply to history; otherwise apply to current
    if let Some(ver) = requested_version {
        if ver == param.version {
            for label in &labels {
                if !param.labels.contains(label) {
                    param.labels.push(label.clone());
                }
            }
        } else {
            let found = param.history.iter_mut().find(|h| h.version == ver);
            if let Some(h) = found {
                for label in &labels {
                    if !h.labels.contains(label) {
                        h.labels.push(label.clone());
                    }
                }
            } else {
                return Err(AwsError::not_found(
                    "ParameterVersionNotFound",
                    format!("Version {ver} of parameter {name} not found"),
                ));
            }
        }
    } else {
        // Apply to current version
        for label in &labels {
            if !param.labels.contains(label) {
                param.labels.push(label.clone());
            }
        }
    }

    Ok(json!({
        "InvalidLabels": [],
        "ParameterVersion": param.version,
    }))
}
