use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{error::resource_not_found, state::LambdaState, util::require_str};

/// Resolve which kind of resource an ARN points at. We dispatch tag
/// operations on this so a single helper handles function / layer / ESM.
enum TagTarget<'a> {
    Function(&'a str),
    LayerVersion { layer_name: &'a str, version: u64 },
    EventSourceMapping(&'a str),
}

fn classify(resource: &str) -> Result<TagTarget<'_>, AwsError> {
    if !resource.starts_with("arn:") {
        return Ok(TagTarget::Function(resource));
    }
    let parts: Vec<&str> = resource.split(':').collect();
    // ARN format: arn:aws:lambda:region:account:resource-type:...
    let resource_type = parts.get(5).copied().unwrap_or("");
    match resource_type {
        "function" => {
            // arn:aws:lambda:region:account:function:name[:qualifier]
            let name = parts.get(6).copied().unwrap_or(resource);
            Ok(TagTarget::Function(name))
        }
        "layer" => {
            // arn:aws:lambda:region:account:layer:name:version
            let layer_name = parts.get(6).copied().unwrap_or("");
            let version = parts
                .get(7)
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or_else(|| {
                    AwsError::bad_request(
                        "InvalidParameterValueException",
                        "layer ARN must include a version",
                    )
                })?;
            Ok(TagTarget::LayerVersion {
                layer_name,
                version,
            })
        }
        "event-source-mapping" => {
            let uuid = parts.get(6).copied().unwrap_or("");
            Ok(TagTarget::EventSourceMapping(uuid))
        }
        _ => Err(AwsError::bad_request(
            "InvalidParameterValueException",
            format!("unsupported tag target ARN: {resource}"),
        )),
    }
}

fn parse_tags(input: &Value) -> Result<HashMap<String, String>, AwsError> {
    let obj = input["Tags"].as_object().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterValueException", "Tags is required")
    })?;
    Ok(obj
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect())
}

fn parse_tag_keys(input: &Value) -> Result<Vec<String>, AwsError> {
    let arr = input["TagKeys"].as_array().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterValueException", "TagKeys is required")
    })?;
    Ok(arr
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect())
}

pub fn tag_resource(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource = require_str(input, "Resource")?;
    let new_tags = parse_tags(input)?;

    match classify(resource)? {
        TagTarget::Function(name) => {
            let mut f = state
                .functions
                .get_mut(name)
                .ok_or_else(|| resource_not_found("function", name))?;
            f.tags.extend(new_tags);
        }
        TagTarget::LayerVersion {
            layer_name,
            version,
        } => {
            let mut entry = state
                .layers
                .get_mut(layer_name)
                .ok_or_else(|| resource_not_found("layer", layer_name))?;
            let lv = entry
                .iter_mut()
                .find(|v| v.version == version)
                .ok_or_else(|| resource_not_found("layer version", &version.to_string()))?;
            lv.tags.extend(new_tags);
        }
        TagTarget::EventSourceMapping(uuid) => {
            let mut m = state
                .event_source_mappings
                .get_mut(uuid)
                .ok_or_else(|| resource_not_found("event source mapping", uuid))?;
            m.tags.extend(new_tags);
        }
    }
    Ok(json!({}))
}

pub fn untag_resource(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource = require_str(input, "Resource")?;
    let keys = parse_tag_keys(input)?;

    match classify(resource)? {
        TagTarget::Function(name) => {
            let mut f = state
                .functions
                .get_mut(name)
                .ok_or_else(|| resource_not_found("function", name))?;
            for k in &keys {
                f.tags.remove(k);
            }
        }
        TagTarget::LayerVersion {
            layer_name,
            version,
        } => {
            let mut entry = state
                .layers
                .get_mut(layer_name)
                .ok_or_else(|| resource_not_found("layer", layer_name))?;
            let lv = entry
                .iter_mut()
                .find(|v| v.version == version)
                .ok_or_else(|| resource_not_found("layer version", &version.to_string()))?;
            for k in &keys {
                lv.tags.remove(k);
            }
        }
        TagTarget::EventSourceMapping(uuid) => {
            let mut m = state
                .event_source_mappings
                .get_mut(uuid)
                .ok_or_else(|| resource_not_found("event source mapping", uuid))?;
            for k in &keys {
                m.tags.remove(k);
            }
        }
    }
    Ok(json!({}))
}

pub fn list_tags(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource = require_str(input, "Resource")?;

    let tags: HashMap<String, String> = match classify(resource)? {
        TagTarget::Function(name) => {
            let f = state
                .functions
                .get(name)
                .ok_or_else(|| resource_not_found("function", name))?;
            f.tags.clone()
        }
        TagTarget::LayerVersion {
            layer_name,
            version,
        } => {
            let entry = state
                .layers
                .get(layer_name)
                .ok_or_else(|| resource_not_found("layer", layer_name))?;
            entry
                .iter()
                .find(|v| v.version == version)
                .ok_or_else(|| resource_not_found("layer version", &version.to_string()))?
                .tags
                .clone()
        }
        TagTarget::EventSourceMapping(uuid) => {
            let m = state
                .event_source_mappings
                .get(uuid)
                .ok_or_else(|| resource_not_found("event source mapping", uuid))?;
            m.tags.clone()
        }
    };

    let map: serde_json::Map<String, Value> = tags
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    Ok(json!({ "Tags": map }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::{
        event_source_mappings::create_event_source_mapping, layers::publish_layer_version,
    };
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;

    fn ctx() -> RequestContext {
        RequestContext::new("lambda", "us-east-1")
    }

    fn empty_zip_b64() -> String {
        let bytes: [u8; 22] = [
            0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        BASE64.encode(bytes)
    }

    #[test]
    fn tag_resource_attaches_tags_to_layer_version() {
        let state = LambdaState::default();
        publish_layer_version(
            &state,
            &json!({
                "LayerName": "shared",
                "Content": { "ZipFile": empty_zip_b64() },
            }),
            &ctx(),
        )
        .unwrap();

        let arn = "arn:aws:lambda:us-east-1:000000000000:layer:shared:1";
        tag_resource(
            &state,
            &json!({ "Resource": arn, "Tags": { "env": "dev" } }),
            &ctx(),
        )
        .unwrap();

        let got = list_tags(&state, &json!({ "Resource": arn }), &ctx()).unwrap();
        assert_eq!(got["Tags"]["env"], json!("dev"));
    }

    #[test]
    fn tag_resource_attaches_tags_to_event_source_mapping() {
        let state = LambdaState::default();
        let resp = create_event_source_mapping(
            &state,
            &json!({
                "EventSourceArn": "arn:aws:sqs:us-east-1:000000000000:q",
                "FunctionName": "f",
            }),
            &ctx(),
        )
        .unwrap();
        let uuid = resp["UUID"].as_str().unwrap().to_string();
        let arn = format!("arn:aws:lambda:us-east-1:000000000000:event-source-mapping:{uuid}");

        tag_resource(
            &state,
            &json!({ "Resource": arn, "Tags": { "team": "ingest" } }),
            &ctx(),
        )
        .unwrap();

        let got = list_tags(&state, &json!({ "Resource": arn }), &ctx()).unwrap();
        assert_eq!(got["Tags"]["team"], json!("ingest"));

        untag_resource(
            &state,
            &json!({ "Resource": arn, "TagKeys": ["team"] }),
            &ctx(),
        )
        .unwrap();
        let got = list_tags(&state, &json!({ "Resource": arn }), &ctx()).unwrap();
        assert!(got["Tags"].as_object().unwrap().is_empty());
    }
}
