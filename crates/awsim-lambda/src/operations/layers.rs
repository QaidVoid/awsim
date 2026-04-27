use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{invalid_parameter, resource_not_found},
    state::{LambdaState, LayerVersion},
    util::{decode_zip, now_iso8601, opt_str, require_str, sha256_base64},
};

fn layer_version_to_value(lv: &LayerVersion) -> Value {
    json!({
        "LayerArn": lv.layer_arn,
        "LayerVersionArn": lv.version_arn,
        "Version": lv.version,
        "Description": lv.description,
        "CompatibleRuntimes": lv.compatible_runtimes,
        "CodeSha256": lv.code_sha256,
        "CodeSize": lv.code_size,
        "CreatedDate": lv.created_date,
    })
}

pub fn publish_layer_version(
    state: &LambdaState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let layer_name = require_str(input, "LayerName")?;
    let description = opt_str(input, "Description").unwrap_or("").to_string();

    let compatible_runtimes: Vec<String> = input
        .get("CompatibleRuntimes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    // Resolve code
    let (code_data, code_sha256, code_size) = if let Some(content) = input.get("Content") {
        if let Some(zip_b64) = content.get("ZipFile").and_then(|v| v.as_str()) {
            let (bytes, hash, size) = decode_zip(zip_b64)?;
            (Some(bytes), hash, size)
        } else {
            let placeholder = b"s3-placeholder";
            (None, sha256_base64(placeholder), 0u64)
        }
    } else {
        return Err(invalid_parameter(
            "Content is required for PublishLayerVersion",
        ));
    };

    let layer_arn = format!(
        "arn:aws:lambda:{}:{}:layer:{}",
        ctx.region, ctx.account_id, layer_name
    );

    let mut versions = state.layers.entry(layer_name.to_string()).or_default();

    let version_number = (versions.len() + 1) as u64;
    let version_arn = format!("{}:{}", layer_arn, version_number);

    let lv = LayerVersion {
        layer_name: layer_name.to_string(),
        layer_arn: layer_arn.clone(),
        version_arn: version_arn.clone(),
        version: version_number,
        description,
        compatible_runtimes,
        code_sha256,
        code_size,
        code_data,
        created_date: now_iso8601(),
    };

    let result = layer_version_to_value(&lv);
    versions.push(lv);

    Ok(result)
}

pub fn list_layers(
    state: &LambdaState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let layers: Vec<Value> = state
        .layers
        .iter()
        .filter_map(|entry| {
            entry.value().last().map(|latest| {
                json!({
                    "LayerName": entry.key(),
                    "LayerArn": latest.layer_arn,
                    "LatestMatchingVersion": layer_version_to_value(latest),
                })
            })
        })
        .collect();

    Ok(json!({ "Layers": layers }))
}

pub fn list_layer_versions(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let layer_name = require_str(input, "LayerName")?;

    let versions = match state.layers.get(layer_name) {
        Some(v) => v.iter().map(layer_version_to_value).collect::<Vec<_>>(),
        None => vec![],
    };

    Ok(json!({ "LayerVersions": versions }))
}

pub fn delete_layer_version(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let layer_name = require_str(input, "LayerName")?;
    let version_number = input
        .get("VersionNumber")
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
        })
        .ok_or_else(|| invalid_parameter("VersionNumber is required"))?;

    let mut entry = state
        .layers
        .get_mut(layer_name)
        .ok_or_else(|| resource_not_found("layer", layer_name))?;

    let before = entry.len();
    entry.retain(|v| v.version != version_number);

    if entry.len() == before {
        return Err(resource_not_found(
            "layer version",
            &version_number.to_string(),
        ));
    }

    Ok(json!({}))
}
