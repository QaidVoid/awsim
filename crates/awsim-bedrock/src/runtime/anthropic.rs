//! Anthropic Messages format ↔ OpenAI chat.completions translator.
//!
//! The Bedrock SDK encodes Anthropic Claude requests as JSON in the
//! shape of Anthropic's native Messages API:
//!
//! ```text
//! {
//!   "anthropic_version": "bedrock-2023-05-31",
//!   "max_tokens": 1024,
//!   "system": "You are a pirate.",
//!   "messages": [
//!     { "role": "user", "content": "Hello!" },
//!     { "role": "assistant", "content": "Ahoy!" },
//!     { "role": "user", "content": "What is your name?" }
//!   ],
//!   "temperature": 0.7,
//!   "top_p": 1.0,
//!   "stop_sequences": ["\n\nHuman:"]
//! }
//! ```
//!
//! The response shape is the Messages-API response with `content`
//! as a list of typed blocks (currently we emit a single
//! `{"type":"text","text":"…"}`).

use awsim_core::AwsError;
use serde_json::{Value, json};
use tracing::warn;

use super::openai::{ChatMessage, ChatRequest, ChatResponse};
use crate::backend::BedrockBackend;

/// Convert a Bedrock-flavoured Anthropic Messages request body into
/// an OpenAI-compatible chat.completions request.
pub fn to_openai_request(model_tag: &str, body: &Value) -> Result<ChatRequest, AwsError> {
    let max_tokens = body
        .get("max_tokens")
        .and_then(Value::as_u64)
        .map(|v| v as u32);
    let temperature = body
        .get("temperature")
        .and_then(Value::as_f64)
        .map(|v| v as f32);
    let top_p = body.get("top_p").and_then(Value::as_f64).map(|v| v as f32);
    let stop = body
        .get("stop_sequences")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect::<Vec<_>>()
        });

    let mut messages: Vec<ChatMessage> = Vec::new();
    if let Some(system) = body.get("system").and_then(Value::as_str)
        && !system.is_empty()
    {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: system.to_string(),
        });
    }
    let arr = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            AwsError::bad_request("ValidationException", "messages array is required")
        })?;
    for m in arr {
        let role = m
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .to_string();
        let content = extract_content_text(m.get("content"));
        messages.push(ChatMessage { role, content });
    }

    Ok(ChatRequest {
        model: model_tag.to_string(),
        messages,
        max_tokens,
        temperature,
        top_p,
        stop,
        stream: None,
    })
}

/// Anthropic content fields can be either a plain string (legacy) or
/// an array of typed content blocks. Concatenate every `text` block.
/// Image / tool-use blocks are dropped with a warning since we proxy
/// to a text-only backend.
fn extract_content_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => {
            let mut out = String::new();
            for block in blocks {
                let kind = block.get("type").and_then(Value::as_str).unwrap_or("text");
                match kind {
                    "text" => {
                        if let Some(t) = block.get("text").and_then(Value::as_str) {
                            if !out.is_empty() {
                                out.push('\n');
                            }
                            out.push_str(t);
                        }
                    }
                    other => warn!(
                        block_type = %other,
                        "Anthropic content block of type other than 'text' dropped — backend doesn't support it"
                    ),
                }
            }
            out
        }
        _ => String::new(),
    }
}

/// Convert the OpenAI chat.completions response back into an
/// Anthropic Messages response. The Bedrock SDK shape mirrors
/// Anthropic's native API except that `model` carries the Bedrock
/// id, not the underlying backend tag — so we restore it from the
/// caller's bedrock id.
pub fn to_bedrock_response(bedrock_id: &str, resp: ChatResponse) -> Value {
    let choice = resp.choices.into_iter().next();
    let (text, finish) = match &choice {
        Some(c) => (
            c.message.content.clone(),
            c.finish_reason
                .clone()
                .unwrap_or_else(|| "end_turn".to_string()),
        ),
        None => (String::new(), "end_turn".to_string()),
    };
    let stop_reason = match finish.as_str() {
        // OpenAI uses "stop"; Anthropic uses "end_turn".
        "stop" => "end_turn",
        "length" => "max_tokens",
        "tool_calls" => "tool_use",
        other => other,
    };
    let usage = resp.usage.unwrap_or_default();

    json!({
        "id": format!("msg_{}", uuid::Uuid::new_v4().simple()),
        "type": "message",
        "role": "assistant",
        "content": [{ "type": "text", "text": text }],
        "model": bedrock_id,
        "stop_reason": stop_reason,
        "stop_sequence": Value::Null,
        "usage": {
            "input_tokens":  usage.prompt_tokens,
            "output_tokens": usage.completion_tokens,
        }
    })
}

/// Hit the backend's `/chat/completions` endpoint with an Anthropic-
/// shaped request. Returns the proxied response in Bedrock's native
/// shape, or an `AwsError` if the backend was unreachable.
pub async fn invoke(
    backend: &BedrockBackend,
    bedrock_id: &str,
    body: &Value,
) -> Result<Value, AwsError> {
    let model_tag = backend.resolve_invoke(bedrock_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("No backend mapping for Bedrock model {bedrock_id}"),
        )
    })?;

    let req = to_openai_request(model_tag, body)?;
    let url = format!("{}/chat/completions", backend.endpoint());
    let mut http_req = backend.client().post(&url).json(&req);
    if let Some(key) = backend.api_key() {
        http_req = http_req.bearer_auth(key);
    }

    let resp = http_req
        .send()
        .await
        .map_err(|e| AwsError::internal(format!("Bedrock backend POST {url} failed: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return Err(AwsError::internal(format!(
            "Bedrock backend returned {status}: {body_text}"
        )));
    }
    let parsed: ChatResponse = resp
        .json()
        .await
        .map_err(|e| AwsError::internal(format!("Bedrock backend JSON parse failed: {e}")))?;
    Ok(to_bedrock_response(bedrock_id, parsed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_string_content() {
        let body = json!({
            "anthropic_version": "bedrock-2023-05-31",
            "max_tokens": 256,
            "messages": [{ "role": "user", "content": "Hello" }],
        });
        let req = to_openai_request("llama3.1:8b", &body).unwrap();
        assert_eq!(req.model, "llama3.1:8b");
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert_eq!(req.messages[0].content, "Hello");
        assert_eq!(req.max_tokens, Some(256));
    }

    #[test]
    fn extracts_text_from_typed_blocks() {
        let body = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "Hello" },
                        { "type": "image", "source": { "data": "..." } },
                        { "type": "text", "text": "world" }
                    ]
                }
            ]
        });
        let req = to_openai_request("m", &body).unwrap();
        // Image block dropped, two text blocks newline-joined.
        assert_eq!(req.messages[0].content, "Hello\nworld");
    }

    #[test]
    fn promotes_system_field_to_first_message() {
        let body = json!({
            "system": "You are a pirate.",
            "messages": [{ "role": "user", "content": "Hi" }],
        });
        let req = to_openai_request("m", &body).unwrap();
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, "system");
        assert_eq!(req.messages[0].content, "You are a pirate.");
    }

    #[test]
    fn missing_messages_array_is_validation_error() {
        let body = json!({ "max_tokens": 100 });
        let err = to_openai_request("m", &body).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn response_translation_maps_finish_reason() {
        let resp = ChatResponse {
            id: "x".into(),
            model: "m".into(),
            choices: vec![super::super::openai::ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: "Yo".into(),
                },
                finish_reason: Some("length".into()),
            }],
            usage: Some(super::super::openai::Usage {
                prompt_tokens: 5,
                completion_tokens: 7,
                total_tokens: 12,
            }),
        };
        let v = to_bedrock_response("anthropic.claude-3-5-sonnet-20241022-v2:0", resp);
        assert_eq!(v["stop_reason"], "max_tokens");
        assert_eq!(v["model"], "anthropic.claude-3-5-sonnet-20241022-v2:0");
        assert_eq!(v["content"][0]["text"], "Yo");
        assert_eq!(v["usage"]["input_tokens"], 5);
        assert_eq!(v["usage"]["output_tokens"], 7);
    }
}
