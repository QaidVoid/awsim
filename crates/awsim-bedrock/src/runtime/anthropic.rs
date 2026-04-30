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
pub async fn invoke_streaming(
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
    let mut req = to_openai_request(model_tag, body)?;
    req.stream = Some(true);

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
    let raw = resp
        .text()
        .await
        .map_err(|e| AwsError::internal(format!("Bedrock backend stream read failed: {e}")))?;

    let events = sse_to_anthropic_events(bedrock_id, &raw);
    Ok(json!({
        // Real Bedrock streams use vnd.amazon.eventstream binary
        // framing; awsim's gateway has no eventstream codec yet, so
        // we emit the parsed events as a JSON array. Inspection
        // tools (admin UI / curl) see real content; full SDK
        // streaming clients still need the wire-level codec.
        "contentType": "application/vnd.amazon.eventstream",
        "body": events,
    }))
}

/// Walk the OpenAI Server-Sent-Events response and emit the
/// Anthropic-flavoured streaming-event sequence:
///
/// 1. `message_start`
/// 2. `content_block_start` (single text block, index 0)
/// 3. `content_block_delta` per chunk delta
/// 4. `content_block_stop`
/// 5. `message_delta` (with stop_reason)
/// 6. `message_stop`
fn sse_to_anthropic_events(bedrock_id: &str, raw: &str) -> Vec<Value> {
    let mut text = String::new();
    let mut finish_reason: Option<String> = None;
    let mut prompt_tokens = 0u32;
    let mut completion_tokens = 0u32;

    for line in raw.lines() {
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }
        let chunk: Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(delta) = chunk["choices"][0]["delta"]["content"].as_str() {
            text.push_str(delta);
        }
        if let Some(fr) = chunk["choices"][0]["finish_reason"].as_str() {
            finish_reason = Some(fr.to_string());
        }
        if let Some(p) = chunk["usage"]["prompt_tokens"].as_u64() {
            prompt_tokens = p as u32;
        }
        if let Some(c) = chunk["usage"]["completion_tokens"].as_u64() {
            completion_tokens = c as u32;
        }
    }

    let stop_reason = match finish_reason.as_deref() {
        Some("stop") => "end_turn",
        Some("length") => "max_tokens",
        Some("tool_calls") => "tool_use",
        Some(other) => other,
        None => "end_turn",
    };

    let msg_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
    let mut events = Vec::new();
    events.push(json!({
        "type": "message_start",
        "message": {
            "id": &msg_id,
            "type": "message",
            "role": "assistant",
            "content": [],
            "model": bedrock_id,
            "stop_reason": Value::Null,
            "stop_sequence": Value::Null,
            "usage": { "input_tokens": prompt_tokens, "output_tokens": 0 }
        }
    }));
    events.push(json!({
        "type": "content_block_start",
        "index": 0,
        "content_block": { "type": "text", "text": "" }
    }));
    // Emit every byte of accumulated text in a single delta — when we
    // gain wire-level streaming the parser already handles per-chunk
    // deltas, so commit #3 stays a content-correct upgrade over the
    // canned single-line mock.
    if !text.is_empty() {
        events.push(json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "text_delta", "text": text }
        }));
    }
    events.push(json!({ "type": "content_block_stop", "index": 0 }));
    events.push(json!({
        "type": "message_delta",
        "delta": { "stop_reason": stop_reason, "stop_sequence": Value::Null },
        "usage": { "output_tokens": completion_tokens }
    }));
    events.push(json!({ "type": "message_stop" }));
    events
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
    fn sse_to_anthropic_events_walks_chunks() {
        let raw = "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\
data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\
data: [DONE]\n";
        let events = sse_to_anthropic_events("anthropic.claude-3-5-sonnet-20241022-v2:0", raw);
        assert_eq!(events[0]["type"], "message_start");
        assert_eq!(
            events[0]["message"]["model"],
            "anthropic.claude-3-5-sonnet-20241022-v2:0"
        );
        assert_eq!(events[1]["type"], "content_block_start");
        assert_eq!(events[2]["type"], "content_block_delta");
        assert_eq!(events[2]["delta"]["text"], "Hello");
        assert_eq!(events[3]["type"], "content_block_stop");
        assert_eq!(events[4]["type"], "message_delta");
        assert_eq!(events[4]["delta"]["stop_reason"], "end_turn");
        assert_eq!(events[4]["usage"]["output_tokens"], 2);
        assert_eq!(events[5]["type"], "message_stop");
    }

    #[test]
    fn sse_to_anthropic_events_handles_empty_response() {
        let events = sse_to_anthropic_events("anthropic.claude-v2", "data: [DONE]\n");
        // Skips the delta event when no text was emitted; everything else still fires.
        assert_eq!(events[0]["type"], "message_start");
        assert_eq!(events[1]["type"], "content_block_start");
        assert_eq!(events[2]["type"], "content_block_stop");
        assert_eq!(events[3]["type"], "message_delta");
        assert_eq!(events[4]["type"], "message_stop");
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
