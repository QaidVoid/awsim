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

use super::openai::{
    ChatMessage, ChatRequest, ChatResponse, ContentPart, ImageUrl, MessageContent,
};
use crate::backend::BedrockBackends;

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
            content: MessageContent::text(system),
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
        let content = extract_content(m.get("content"));
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
        stream_options: None,
    })
}

/// Anthropic content fields can be either a plain string (legacy) or
/// an array of typed content blocks. Text blocks are concatenated;
/// `image` blocks are forwarded as OpenAI-compat `image_url` parts
/// (data URL for base64 sources, raw URL for `url` sources). When the
/// message has no images we collapse back to a plain string so
/// text-only backends (which may reject the parts array) keep working.
/// Tool-use / unknown blocks are dropped with a warning.
fn extract_content(v: Option<&Value>) -> MessageContent {
    match v {
        Some(Value::String(s)) => MessageContent::Text(s.clone()),
        Some(Value::Array(blocks)) => {
            let mut parts: Vec<ContentPart> = Vec::new();
            for block in blocks {
                let kind = block.get("type").and_then(Value::as_str).unwrap_or("text");
                match kind {
                    "text" => {
                        if let Some(t) = block.get("text").and_then(Value::as_str) {
                            parts.push(ContentPart::Text {
                                text: t.to_string(),
                            });
                        }
                    }
                    "image" => match anthropic_image_to_data_url(block.get("source")) {
                        Some(url) => parts.push(ContentPart::ImageUrl {
                            image_url: ImageUrl { url },
                        }),
                        None => warn!("Anthropic image block had no recognizable source, dropped"),
                    },
                    other => warn!(
                        block_type = %other,
                        "Anthropic content block dropped, backend doesn't support it"
                    ),
                }
            }
            collapse_parts(parts)
        }
        _ => MessageContent::Text(String::new()),
    }
}

/// Translate an Anthropic image `source` object into a data URL the
/// OpenAI-compat surface understands. Anthropic's two source shapes:
///
/// - `{ "type": "base64", "media_type": "image/png", "data": "..." }`
/// - `{ "type": "url", "url": "https://..." }`
fn anthropic_image_to_data_url(source: Option<&Value>) -> Option<String> {
    let source = source?;
    let stype = source
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("base64");
    match stype {
        "base64" => {
            let mime = source
                .get("media_type")
                .and_then(Value::as_str)
                .unwrap_or("image/png");
            let data = source.get("data").and_then(Value::as_str)?;
            Some(format!("data:{mime};base64,{data}"))
        }
        "url" => source.get("url").and_then(Value::as_str).map(String::from),
        _ => None,
    }
}

/// If every part is text, fold them into a single newline-joined
/// string. Keeps requests for text-only backends in their native
/// shape and matches the legacy behaviour of the old extractor.
fn collapse_parts(parts: Vec<ContentPart>) -> MessageContent {
    if parts.iter().all(|p| matches!(p, ContentPart::Text { .. })) {
        let mut out = String::new();
        for p in parts {
            if let ContentPart::Text { text } = p {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&text);
            }
        }
        MessageContent::Text(out)
    } else {
        MessageContent::Parts(parts)
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
            c.message.content.as_text(),
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

/// Hit the backend with `stream:true`, accumulate the SSE chunks,
/// and emit the Anthropic-flavoured streaming-event sequence.
pub async fn invoke_streaming(
    backends: &BedrockBackends,
    bedrock_id: &str,
    body: &Value,
) -> Result<Value, AwsError> {
    let acc =
        super::call_chat_stream(backends, bedrock_id, |tag| to_openai_request(tag, body)).await?;
    let events = build_events(bedrock_id, &acc);
    Ok(super::stream_envelope(events))
}

/// Convert an accumulated stream into the Anthropic event sequence:
///
/// 1. `message_start`
/// 2. `content_block_start` (single text block, index 0)
/// 3. `content_block_delta` (full text in one chunk for now)
/// 4. `content_block_stop`
/// 5. `message_delta` (with stop_reason)
/// 6. `message_stop`
fn build_events(bedrock_id: &str, acc: &super::AccumulatedStream) -> Vec<Value> {
    let stop_reason = match acc.finish_reason.as_deref() {
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
            "usage": { "input_tokens": acc.prompt_tokens, "output_tokens": 0 }
        }
    }));
    events.push(json!({
        "type": "content_block_start",
        "index": 0,
        "content_block": { "type": "text", "text": "" }
    }));
    if !acc.text.is_empty() {
        events.push(json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "text_delta", "text": &acc.text }
        }));
    }
    events.push(json!({ "type": "content_block_stop", "index": 0 }));
    events.push(json!({
        "type": "message_delta",
        "delta": { "stop_reason": stop_reason, "stop_sequence": Value::Null },
        "usage": { "output_tokens": acc.completion_tokens }
    }));
    events.push(json!({ "type": "message_stop" }));
    events
}

/// Hit the backend's `/chat/completions` endpoint with an Anthropic-
/// shaped request. Returns the proxied response in Bedrock's native
/// shape, or an `AwsError` if the backend was unreachable.
pub async fn invoke(
    backends: &BedrockBackends,
    bedrock_id: &str,
    body: &Value,
) -> Result<Value, AwsError> {
    let resp = super::call_chat(backends, bedrock_id, |tag| to_openai_request(tag, body)).await?;
    Ok(to_bedrock_response(bedrock_id, resp))
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
        assert_eq!(req.messages[0].content.as_text(), "Hello");
        assert_eq!(req.max_tokens, Some(256));
    }

    #[test]
    fn collapses_text_only_blocks_to_string() {
        // No image present, so the parts list folds back into a single
        // string and the wire format stays compatible with text-only
        // backends that reject the OpenAI multimodal parts shape.
        let body = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "Hello" },
                        { "type": "text", "text": "world" }
                    ]
                }
            ]
        });
        let req = to_openai_request("m", &body).unwrap();
        match &req.messages[0].content {
            MessageContent::Text(s) => assert_eq!(s, "Hello\nworld"),
            other => panic!("expected text-only collapse, got {other:?}"),
        }
    }

    #[test]
    fn forwards_image_block_as_image_url_part() {
        let body = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "what is this?" },
                        { "type": "image", "source": {
                            "type": "base64",
                            "media_type": "image/png",
                            "data": "iVBORw0KGgo="
                        }}
                    ]
                }
            ]
        });
        let req = to_openai_request("m", &body).unwrap();
        let parts = match &req.messages[0].content {
            MessageContent::Parts(p) => p,
            other => panic!("expected parts array, got {other:?}"),
        };
        assert_eq!(parts.len(), 2);
        match &parts[1] {
            ContentPart::ImageUrl { image_url } => {
                assert_eq!(image_url.url, "data:image/png;base64,iVBORw0KGgo=");
            }
            other => panic!("expected image_url part, got {other:?}"),
        }
    }

    #[test]
    fn forwards_url_source_image_unchanged() {
        let body = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        { "type": "image", "source": {
                            "type": "url",
                            "url": "https://example.com/cat.png"
                        }}
                    ]
                }
            ]
        });
        let req = to_openai_request("m", &body).unwrap();
        let parts = match &req.messages[0].content {
            MessageContent::Parts(p) => p,
            other => panic!("expected parts array, got {other:?}"),
        };
        match &parts[0] {
            ContentPart::ImageUrl { image_url } => {
                assert_eq!(image_url.url, "https://example.com/cat.png");
            }
            other => panic!("expected image_url part, got {other:?}"),
        }
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
        assert_eq!(req.messages[0].content.as_text(), "You are a pirate.");
    }

    #[test]
    fn missing_messages_array_is_validation_error() {
        let body = json!({ "max_tokens": 100 });
        let err = to_openai_request("m", &body).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn build_events_walks_accumulated_stream() {
        let acc = super::super::AccumulatedStream {
            text: "Hello".into(),
            finish_reason: Some("stop".into()),
            prompt_tokens: 3,
            completion_tokens: 2,
        };
        let events = build_events("anthropic.claude-3-5-sonnet-20241022-v2:0", &acc);
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
    fn build_events_handles_empty_text() {
        let acc = super::super::AccumulatedStream::default();
        let events = build_events("anthropic.claude-v2", &acc);
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
