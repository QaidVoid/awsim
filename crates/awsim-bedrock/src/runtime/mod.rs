//! Bedrock runtime translator dispatch.
//!
//! When a `BedrockBackend` is configured, each Bedrock-flavoured
//! request is routed to the per-vendor translator that converts it
//! to OpenAI-compatible chat.completions / embeddings calls and
//! shapes the response back into Bedrock's native format.
//!
//! When no backend is configured (or the backend is unreachable),
//! we fall back to deterministic canned responses so SDK code that
//! just wires up the calls keeps working in CI.

use awsim_core::AwsError;
use serde_json::Value;
use tracing::{debug, warn};

use crate::backend::BedrockBackend;

mod anthropic;
mod canned;
mod openai;

/// Dispatch InvokeModel by Bedrock model-id prefix. Routes Anthropic
/// (`anthropic.claude-*`) to the proxy translator when a backend is
/// configured; everything else still hits the canned fallback (will
/// be expanded in subsequent commits).
pub async fn invoke_model(
    backend: Option<&BedrockBackend>,
    input: &Value,
) -> Result<Value, AwsError> {
    let model_id = input["modelId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "modelId is required"))?;
    debug!(model_id = %model_id, "InvokeModel");

    let body = extract_body(input)?;

    if let Some(backend) = backend
        && matches!(ModelFamily::for_id(model_id), Some(ModelFamily::Anthropic))
    {
        match anthropic::invoke(backend, model_id, &body).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                warn!(error = %e.message, model_id, "Bedrock backend failed; serving canned response");
            }
        }
    }

    canned::invoke_model(input)
}

pub async fn invoke_model_with_response_stream(
    backend: Option<&BedrockBackend>,
    input: &Value,
) -> Result<Value, AwsError> {
    let model_id = input["modelId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "modelId is required"))?;
    debug!(model_id = %model_id, "InvokeModelWithResponseStream");
    let body = extract_body(input)?;

    if let Some(backend) = backend
        && matches!(ModelFamily::for_id(model_id), Some(ModelFamily::Anthropic))
    {
        match anthropic::invoke_streaming(backend, model_id, &body).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                warn!(error = %e.message, model_id, "Bedrock streaming backend failed; serving canned response");
            }
        }
    }
    canned::invoke_model_with_response_stream(input)
}

pub fn converse(input: &Value) -> Result<Value, AwsError> {
    canned::converse(input)
}

pub fn converse_stream(input: &Value) -> Result<Value, AwsError> {
    canned::converse_stream(input)
}

/// `body` arrives as a JSON-encoded string in the Bedrock wire
/// format. The router unwraps it to a `Value` for the SDK; we
/// further normalise here so translators get an object to walk.
fn extract_body(input: &Value) -> Result<Value, AwsError> {
    match input.get("body") {
        Some(Value::Object(_)) | Some(Value::Array(_)) => Ok(input["body"].clone()),
        Some(Value::String(s)) => serde_json::from_str(s).map_err(|e| {
            AwsError::bad_request(
                "ValidationException",
                format!("body is not valid JSON: {e}"),
            )
        }),
        Some(Value::Null) | None => Ok(Value::Object(serde_json::Map::new())),
        Some(other) => Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "body must be a JSON object or string, got {}",
                kind_of(other)
            ),
        )),
    }
}

fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[derive(Debug, Clone, Copy)]
enum ModelFamily {
    Anthropic,
    /// Catch-all for ids that aren't yet routed to a translator —
    /// land in canned fallback.
    Other,
}

impl ModelFamily {
    fn for_id(id: &str) -> Option<Self> {
        if id.starts_with("anthropic.claude") {
            Some(Self::Anthropic)
        } else {
            Some(Self::Other)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_body_accepts_json_string() {
        let input = json!({ "modelId": "x", "body": "{\"messages\":[]}" });
        let body = extract_body(&input).unwrap();
        assert!(body["messages"].is_array());
    }

    #[test]
    fn extract_body_accepts_object() {
        let input = json!({ "modelId": "x", "body": { "messages": [] } });
        let body = extract_body(&input).unwrap();
        assert!(body["messages"].is_array());
    }

    #[test]
    fn extract_body_rejects_garbage_string() {
        let input = json!({ "modelId": "x", "body": "not json" });
        let err = extract_body(&input).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }
}
