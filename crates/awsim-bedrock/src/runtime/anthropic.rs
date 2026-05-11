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
use base64::Engine;
use serde_json::{Value, json};
use tracing::warn;

use super::openai::{
    ChatMessage, ChatRequest, ChatResponse, ContentPart, FunctionDef, ImageUrl, MessageContent,
    Tool, ToolCall, ToolCallFunction,
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
            ..ChatMessage::default()
        });
    }
    let arr = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            AwsError::bad_request("ValidationException", "messages array is required")
        })?;
    for m in arr {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("user");
        messages.extend(extract_anthropic_messages(role, m.get("content")));
    }

    Ok(ChatRequest {
        model: model_tag.to_string(),
        messages,
        max_tokens,
        temperature,
        top_p,
        stop,
        tools: extract_tools(body.get("tools")),
        tool_choice: extract_tool_choice(body.get("tool_choice")),
        ..ChatRequest::default()
    })
}

/// Translate Anthropic's `tools` array (each entry has `name`,
/// `description`, and `input_schema`) into OpenAI function specs.
/// Returns `None` if no tools were declared so the field gets
/// `skip_serializing_if`'d off the wire.
fn extract_tools(v: Option<&Value>) -> Option<Vec<Tool>> {
    let arr = v.and_then(Value::as_array)?;
    let mut out = Vec::new();
    for t in arr {
        let Some(name) = t.get("name").and_then(Value::as_str) else {
            continue;
        };
        let description = t
            .get("description")
            .and_then(Value::as_str)
            .map(String::from);
        let parameters = t
            .get("input_schema")
            .cloned()
            .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
        out.push(Tool {
            kind: "function".to_string(),
            function: FunctionDef {
                name: name.to_string(),
                description,
                parameters,
            },
        });
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Anthropic tool_choice -> OpenAI tool_choice. Mappings:
/// `auto` -> `"auto"`, `any` -> `"required"`, `none` -> `"none"`,
/// `tool` with `name` -> `{ "type": "function", "function": { "name": ... }}`.
fn extract_tool_choice(v: Option<&Value>) -> Option<Value> {
    let obj = v?.as_object()?;
    let kind = obj.get("type").and_then(Value::as_str)?;
    match kind {
        "auto" => Some(json!("auto")),
        "any" => Some(json!("required")),
        "none" => Some(json!("none")),
        "tool" => {
            let name = obj.get("name").and_then(Value::as_str)?;
            Some(json!({ "type": "function", "function": { "name": name } }))
        }
        _ => None,
    }
}

/// Split one Anthropic message into the OpenAI messages it represents.
/// Assistant messages collapse into a single OpenAI message whose
/// `tool_calls` carries any `tool_use` blocks. User messages may emit
/// multiple OpenAI messages: each `tool_result` block becomes a
/// `role: tool` message (OpenAI requires tool replies to precede the
/// next user turn) and any remaining text / image / document blocks
/// form one trailing `role: user` message.
fn extract_anthropic_messages(role: &str, content: Option<&Value>) -> Vec<ChatMessage> {
    match content {
        Some(Value::String(s)) => vec![ChatMessage {
            role: role.to_string(),
            content: MessageContent::text(s),
            ..ChatMessage::default()
        }],
        Some(Value::Array(blocks)) => match role {
            "assistant" => vec![extract_assistant_message(blocks)],
            _ => extract_user_messages(blocks),
        },
        _ => vec![ChatMessage {
            role: role.to_string(),
            content: MessageContent::default(),
            ..ChatMessage::default()
        }],
    }
}

fn extract_assistant_message(blocks: &[Value]) -> ChatMessage {
    let mut parts: Vec<ContentPart> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
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
            "document" => {
                if let Some(part) = anthropic_document_to_part(block) {
                    parts.push(part);
                }
            }
            "tool_use" => {
                let id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let arguments = block
                    .get("input")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "{}".to_string());
                tool_calls.push(ToolCall {
                    id,
                    kind: "function".to_string(),
                    function: ToolCallFunction { name, arguments },
                });
            }
            other => warn!(
                block_type = %other,
                "Anthropic assistant content block dropped, backend doesn't support it"
            ),
        }
    }
    ChatMessage {
        role: "assistant".to_string(),
        content: collapse_parts(parts),
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        tool_call_id: None,
    }
}

fn extract_user_messages(blocks: &[Value]) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::new();
    let mut user_parts: Vec<ContentPart> = Vec::new();
    for block in blocks {
        let kind = block.get("type").and_then(Value::as_str).unwrap_or("text");
        match kind {
            "text" => {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    user_parts.push(ContentPart::Text {
                        text: t.to_string(),
                    });
                }
            }
            "image" => match anthropic_image_to_data_url(block.get("source")) {
                Some(url) => user_parts.push(ContentPart::ImageUrl {
                    image_url: ImageUrl { url },
                }),
                None => warn!("Anthropic image block had no recognizable source, dropped"),
            },
            "document" => {
                if let Some(part) = anthropic_document_to_part(block) {
                    user_parts.push(part);
                }
            }
            "tool_result" => {
                let id = block
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let text = tool_result_to_text(block.get("content"));
                out.push(ChatMessage {
                    role: "tool".to_string(),
                    content: MessageContent::text(text),
                    tool_calls: None,
                    tool_call_id: Some(id),
                });
            }
            other => warn!(
                block_type = %other,
                "Anthropic user content block dropped, backend doesn't support it"
            ),
        }
    }
    if !user_parts.is_empty() {
        out.push(ChatMessage {
            role: "user".to_string(),
            content: collapse_parts(user_parts),
            ..ChatMessage::default()
        });
    }
    out
}

/// Flatten a tool_result `content` field (string or array of typed
/// blocks) into a plain string. The OpenAI tool message carries
/// content as a single string.
fn tool_result_to_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => {
            let mut out = String::new();
            for b in blocks {
                if let Some(t) = b.get("text").and_then(Value::as_str) {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(t);
                }
            }
            out
        }
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// Translate an Anthropic `document` block into a content part.
///
/// Documents arrive shaped like images: a `source` of `{ "type":
/// "base64", "media_type": "<mime>", "data": "<b64>" }` or `{ "type":
/// "text", "media_type": "text/plain", "data": "..." }`. We try to
/// inline the file as a text block (wrapping the body in a
/// `<document>` envelope tagged with name/format) since that flows
/// through every text-only backend. Binary payloads that don't decode
/// as UTF-8 fall back to inlining a marker so the model at least
/// knows an attachment was present.
fn anthropic_document_to_part(block: &Value) -> Option<ContentPart> {
    let source = block.get("source")?;
    let stype = source
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("base64");
    let name = block
        .get("title")
        .or_else(|| block.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("document");
    let media_type = source
        .get("media_type")
        .and_then(Value::as_str)
        .unwrap_or("application/octet-stream");
    let raw = match stype {
        "text" => source.get("data").and_then(Value::as_str)?.to_string(),
        "base64" => {
            let data = source.get("data").and_then(Value::as_str)?;
            match base64::engine::general_purpose::STANDARD.decode(data) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(s) => s,
                    Err(_) => {
                        return Some(ContentPart::Text {
                            text: format!(
                                "<document name=\"{name}\" media-type=\"{media_type}\">[binary content omitted, {} bytes base64-encoded]</document>",
                                data.len()
                            ),
                        });
                    }
                },
                Err(e) => {
                    warn!(error = %e, "Anthropic document block had invalid base64, dropped");
                    return None;
                }
            }
        }
        other => {
            warn!(source_type = %other, "Anthropic document source type unsupported, dropped");
            return None;
        }
    };
    Some(ContentPart::Text {
        text: format!("<document name=\"{name}\" media-type=\"{media_type}\">\n{raw}\n</document>"),
    })
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
    let (text, tool_calls, finish) = match choice {
        Some(c) => (
            c.message.content.as_text(),
            c.message.tool_calls.unwrap_or_default(),
            c.finish_reason.unwrap_or_else(|| "end_turn".to_string()),
        ),
        None => (String::new(), Vec::new(), "end_turn".to_string()),
    };
    let mut blocks: Vec<Value> = Vec::new();
    if !text.is_empty() {
        blocks.push(json!({ "type": "text", "text": text }));
    }
    for tc in &tool_calls {
        blocks.push(tool_call_to_use_block(tc));
    }
    if blocks.is_empty() {
        blocks.push(json!({ "type": "text", "text": "" }));
    }
    let stop_reason = if !tool_calls.is_empty() {
        "tool_use"
    } else {
        match finish.as_str() {
            // OpenAI uses "stop"; Anthropic uses "end_turn".
            "stop" => "end_turn",
            "length" => "max_tokens",
            "tool_calls" => "tool_use",
            other => other,
        }
    };
    let usage = resp.usage.unwrap_or_default();

    json!({
        "id": format!("msg_{}", uuid::Uuid::new_v4().simple()),
        "type": "message",
        "role": "assistant",
        "content": blocks,
        "model": bedrock_id,
        "stop_reason": stop_reason,
        "stop_sequence": Value::Null,
        "usage": {
            "input_tokens":  usage.prompt_tokens,
            "output_tokens": usage.completion_tokens,
        }
    })
}

/// Map one OpenAI `tool_calls` entry back to an Anthropic `tool_use`
/// content block. The OpenAI side ships arguments as a stringified
/// JSON object; Anthropic expects a structured value under `input`,
/// so we parse on the way out (falling back to `{}` on malformed
/// JSON rather than failing the whole response).
fn tool_call_to_use_block(tc: &ToolCall) -> Value {
    let input: Value = serde_json::from_str(&tc.function.arguments).unwrap_or_else(|_| json!({}));
    json!({
        "type": "tool_use",
        "id": tc.id,
        "name": tc.function.name,
        "input": input,
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

/// Convert an accumulated stream into the Anthropic event sequence.
/// The shape mirrors Anthropic's native API: a `message_start`, then
/// one start/delta/stop trio per content block (index 0 for text,
/// 1..N for any tool_use calls), then `message_delta` carrying the
/// final stop_reason, then `message_stop`. When tool_calls are
/// present the stop_reason is forced to `tool_use`.
fn build_events(bedrock_id: &str, acc: &super::AccumulatedStream) -> Vec<Value> {
    let has_tools = !acc.tool_calls.is_empty();
    let stop_reason = if has_tools {
        "tool_use"
    } else {
        match acc.finish_reason.as_deref() {
            Some("stop") => "end_turn",
            Some("length") => "max_tokens",
            Some("tool_calls") => "tool_use",
            Some(other) => other,
            None => "end_turn",
        }
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
    for (i, tc) in acc.tool_calls.iter().enumerate() {
        let idx = i + 1;
        events.push(json!({
            "type": "content_block_start",
            "index": idx,
            "content_block": {
                "type": "tool_use",
                "id": tc.id,
                "name": tc.name,
                "input": {},
            }
        }));
        // Anthropic streams partial JSON via input_json_delta; we ship
        // the assembled object in a single delta since the backend
        // accumulator already collapsed the chunks.
        if !tc.arguments.is_empty() {
            events.push(json!({
                "type": "content_block_delta",
                "index": idx,
                "delta": { "type": "input_json_delta", "partial_json": tc.arguments }
            }));
        }
        events.push(json!({ "type": "content_block_stop", "index": idx }));
    }
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
            ..super::super::AccumulatedStream::default()
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
                    ..ChatMessage::default()
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

    #[test]
    fn forwards_tools_and_tool_choice() {
        let body = json!({
            "anthropic_version": "bedrock-2023-05-31",
            "max_tokens": 256,
            "tools": [{
                "name": "get_weather",
                "description": "Look up weather",
                "input_schema": {
                    "type": "object",
                    "properties": { "city": { "type": "string" } }
                }
            }],
            "tool_choice": { "type": "any" },
            "messages": [{ "role": "user", "content": "Hi" }],
        });
        let req = to_openai_request("m", &body).unwrap();
        let tools = req.tools.as_deref().expect("tools present");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function.name, "get_weather");
        assert_eq!(
            tools[0].function.description.as_deref(),
            Some("Look up weather")
        );
        assert_eq!(req.tool_choice, Some(json!("required")));
    }

    #[test]
    fn splits_user_tool_result_into_tool_message() {
        let body = json!({
            "messages": [
                { "role": "assistant", "content": [
                    { "type": "tool_use", "id": "tu_1", "name": "get_weather",
                      "input": { "city": "Kathmandu" }}
                ]},
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "tu_1", "content": "20C" }
                ]}
            ]
        });
        let req = to_openai_request("m", &body).unwrap();
        assert_eq!(req.messages.len(), 2);
        let calls = req.messages[0].tool_calls.as_deref().unwrap();
        assert_eq!(calls[0].id, "tu_1");
        assert_eq!(calls[0].function.arguments, "{\"city\":\"Kathmandu\"}");
        assert_eq!(req.messages[1].role, "tool");
        assert_eq!(req.messages[1].tool_call_id.as_deref(), Some("tu_1"));
        assert_eq!(req.messages[1].content.as_text(), "20C");
    }

    #[test]
    fn inlines_text_document_block() {
        let csv_b64 = base64::engine::general_purpose::STANDARD.encode("a,b\n1,2\n");
        let body = json!({
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "summarize" },
                    { "type": "document", "title": "rows.csv", "source": {
                        "type": "base64",
                        "media_type": "text/csv",
                        "data": csv_b64
                    }}
                ]
            }]
        });
        let req = to_openai_request("m", &body).unwrap();
        let s = match &req.messages[0].content {
            MessageContent::Text(s) => s.clone(),
            other => panic!("expected text-only collapse, got {other:?}"),
        };
        assert!(s.contains("summarize"));
        assert!(s.contains("<document name=\"rows.csv\" media-type=\"text/csv\">"));
        assert!(s.contains("a,b"));
    }

    #[test]
    fn response_emits_tool_use_block() {
        let resp = ChatResponse {
            id: "x".into(),
            model: "m".into(),
            choices: vec![super::super::openai::ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: MessageContent::default(),
                    tool_calls: Some(vec![ToolCall {
                        id: "tu_99".into(),
                        kind: "function".into(),
                        function: ToolCallFunction {
                            name: "get_weather".into(),
                            arguments: "{\"city\":\"Pokhara\"}".into(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
        };
        let v = to_bedrock_response("anthropic.claude-3-5-sonnet-20241022-v2:0", resp);
        assert_eq!(v["stop_reason"], "tool_use");
        let use_block = &v["content"][0];
        assert_eq!(use_block["type"], "tool_use");
        assert_eq!(use_block["id"], "tu_99");
        assert_eq!(use_block["name"], "get_weather");
        assert_eq!(use_block["input"]["city"], "Pokhara");
    }
}
