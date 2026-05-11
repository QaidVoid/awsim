//! Bedrock Converse / ConverseStream — the unified cross-model
//! API. Same request / response shape for every vendor, so there's
//! a single translator that maps to OpenAI chat.completions.
//!
//! Converse request:
//! ```text
//! { "modelId": "...",
//!   "messages": [{ "role": "user", "content": [{"text":"..."}] }],
//!   "system":   [{ "text": "..." }],
//!   "inferenceConfig": { "maxTokens":N, "temperature":F, "topP":F,
//!                        "stopSequences":[...] } }
//! ```
//! Converse response:
//! ```text
//! { "output": { "message": { "role":"assistant",
//!                            "content":[{"text":"..."}] } },
//!   "stopReason": "end_turn"|"max_tokens"|"stop_sequence"|"tool_use",
//!   "usage": { "inputTokens":N, "outputTokens":M, "totalTokens":NM },
//!   "metrics": { "latencyMs":… } }
//! ```

use awsim_core::AwsError;
use base64::Engine;
use serde_json::{Value, json};

use super::openai::{
    ChatMessage, ChatRequest, ChatResponse, ContentPart, FunctionDef, ImageUrl, MessageContent,
    Tool, ToolCall, ToolCallFunction,
};
use crate::backend::BedrockBackends;
use tracing::warn;

pub(crate) fn to_openai_request(model_tag: &str, input: &Value) -> Result<ChatRequest, AwsError> {
    let cfg = &input["inferenceConfig"];
    let max_tokens = cfg
        .get("maxTokens")
        .and_then(Value::as_u64)
        .map(|v| v as u32);
    let temperature = cfg
        .get("temperature")
        .and_then(Value::as_f64)
        .map(|v| v as f32);
    let top_p = cfg.get("topP").and_then(Value::as_f64).map(|v| v as f32);
    let stop = cfg.get("stopSequences").and_then(Value::as_array).map(|a| {
        a.iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect::<Vec<_>>()
    });

    let mut messages: Vec<ChatMessage> = Vec::new();
    if let Some(arr) = input.get("system").and_then(Value::as_array) {
        let mut text = String::new();
        for block in arr {
            if let Some(t) = block.get("text").and_then(Value::as_str) {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(t);
            }
        }
        if !text.is_empty() {
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: MessageContent::text(text),
                ..ChatMessage::default()
            });
        }
    }

    let arr = input
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            AwsError::bad_request("ValidationException", "messages array is required")
        })?;
    for m in arr {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("user");
        messages.extend(extract_converse_messages(
            role,
            m.get("content").and_then(Value::as_array),
        ));
    }

    let tool_cfg = input.get("toolConfig");
    Ok(ChatRequest {
        model: model_tag.to_string(),
        messages,
        max_tokens,
        temperature,
        top_p,
        stop,
        tools: tool_cfg.and_then(|c| extract_tools(c.get("tools"))),
        tool_choice: tool_cfg.and_then(|c| extract_tool_choice(c.get("toolChoice"))),
        ..ChatRequest::default()
    })
}

/// Translate the Converse `toolConfig.tools` array into OpenAI's
/// function spec list. Each entry wraps the spec in
/// `{ "toolSpec": { "name", "description", "inputSchema": { "json": ... }}}`.
fn extract_tools(v: Option<&Value>) -> Option<Vec<Tool>> {
    let arr = v.and_then(Value::as_array)?;
    let mut out = Vec::new();
    for entry in arr {
        let spec = entry.get("toolSpec").unwrap_or(entry);
        let Some(name) = spec.get("name").and_then(Value::as_str) else {
            continue;
        };
        let description = spec
            .get("description")
            .and_then(Value::as_str)
            .map(String::from);
        let parameters = spec
            .get("inputSchema")
            .and_then(|s| s.get("json"))
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

/// Converse toolChoice variants are wrapped objects:
/// `{ "auto": {} }`, `{ "any": {} }`, `{ "tool": { "name": "..." } }`.
fn extract_tool_choice(v: Option<&Value>) -> Option<Value> {
    let obj = v?.as_object()?;
    if obj.contains_key("auto") {
        return Some(json!("auto"));
    }
    if obj.contains_key("any") {
        return Some(json!("required"));
    }
    if let Some(tool) = obj.get("tool")
        && let Some(name) = tool.get("name").and_then(Value::as_str)
    {
        return Some(json!({ "type": "function", "function": { "name": name } }));
    }
    None
}

/// Split a Converse message into the OpenAI messages it represents.
/// `toolUse` blocks fold into the assistant message's `tool_calls`;
/// `toolResult` blocks each become their own `role: tool` message
/// (placed before any other content from the same user message, since
/// OpenAI requires tool replies to precede the next user turn).
fn extract_converse_messages(role: &str, blocks: Option<&Vec<Value>>) -> Vec<ChatMessage> {
    let Some(blocks) = blocks else {
        return vec![ChatMessage {
            role: role.to_string(),
            content: MessageContent::default(),
            ..ChatMessage::default()
        }];
    };
    match role {
        "assistant" => vec![converse_assistant_message(blocks)],
        _ => converse_user_messages(blocks),
    }
}

fn converse_assistant_message(blocks: &[Value]) -> ChatMessage {
    let mut parts: Vec<ContentPart> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    for block in blocks {
        consume_common_block(block, &mut parts);
        if let Some(use_block) = block.get("toolUse") {
            let id = use_block
                .get("toolUseId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let name = use_block
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let arguments = use_block
                .get("input")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "{}".to_string());
            tool_calls.push(ToolCall {
                id,
                kind: "function".to_string(),
                function: ToolCallFunction { name, arguments },
            });
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

fn converse_user_messages(blocks: &[Value]) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::new();
    let mut user_parts: Vec<ContentPart> = Vec::new();
    for block in blocks {
        consume_common_block(block, &mut user_parts);
        if let Some(result) = block.get("toolResult") {
            let id = result
                .get("toolUseId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let text = converse_tool_result_to_text(result.get("content"));
            out.push(ChatMessage {
                role: "tool".to_string(),
                content: MessageContent::text(text),
                tool_calls: None,
                tool_call_id: Some(id),
            });
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

/// Pull text / image / document parts out of a Converse content block
/// and append them to `parts`. `video`, `toolUse`, and `toolResult`
/// blocks are handled by their callers; this helper is the shared bit.
fn consume_common_block(block: &Value, parts: &mut Vec<ContentPart>) {
    if let Some(t) = block.get("text").and_then(Value::as_str) {
        parts.push(ContentPart::Text {
            text: t.to_string(),
        });
        return;
    }
    if let Some(img) = block.get("image") {
        match converse_image_to_data_url(img) {
            Some(url) => parts.push(ContentPart::ImageUrl {
                image_url: ImageUrl { url },
            }),
            None => warn!("Converse image block missing source.bytes, dropped"),
        }
        return;
    }
    if let Some(doc) = block.get("document")
        && let Some(part) = converse_document_to_part(doc)
    {
        parts.push(part);
        return;
    }
    if block.get("video").is_some() {
        warn!("Converse video block dropped, backend doesn't support it");
    }
}

fn converse_tool_result_to_text(v: Option<&Value>) -> String {
    let Some(arr) = v.and_then(Value::as_array) else {
        return v
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_default();
    };
    let mut out = String::new();
    for block in arr {
        if let Some(t) = block.get("text").and_then(Value::as_str) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(t);
            continue;
        }
        if let Some(json) = block.get("json") {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&json.to_string());
        }
    }
    out
}

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

/// Converse document blocks carry `{ "format": "<ext>", "name": "...",
/// "source": { "bytes": "<base64>" } }`. We inline the file as a text
/// part wrapped in a `<document>` envelope so every backend sees it;
/// binary payloads that don't decode as UTF-8 fall back to a marker.
fn converse_document_to_part(doc: &Value) -> Option<ContentPart> {
    let format = doc.get("format").and_then(Value::as_str).unwrap_or("");
    let name = doc
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("document");
    let data_b64 = doc
        .get("source")
        .and_then(|s| s.get("bytes"))
        .and_then(Value::as_str)?;
    match base64::engine::general_purpose::STANDARD.decode(data_b64) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => Some(ContentPart::Text {
                text: format!("<document name=\"{name}\" format=\"{format}\">\n{s}\n</document>"),
            }),
            Err(_) => Some(ContentPart::Text {
                text: format!(
                    "<document name=\"{name}\" format=\"{format}\">[binary content omitted, {} bytes base64-encoded]</document>",
                    data_b64.len()
                ),
            }),
        },
        Err(e) => {
            warn!(error = %e, "Converse document block had invalid base64, dropped");
            None
        }
    }
}

/// Bedrock Converse encodes images as
/// `{ "format": "png|jpeg|gif|webp", "source": { "bytes": "<base64>" } }`.
/// On the JSON wire the SDK hands us the bytes already base64-encoded
/// so we can fold them straight into a `data:` URL.
fn converse_image_to_data_url(img: &Value) -> Option<String> {
    let format = img.get("format").and_then(Value::as_str).unwrap_or("png");
    let bytes = img
        .get("source")
        .and_then(|s| s.get("bytes"))
        .and_then(Value::as_str)?;
    Some(format!("data:image/{format};base64,{bytes}"))
}

/// Map one OpenAI `tool_calls` entry back to a Converse `toolUse`
/// content block. Arguments arrive stringified; Converse expects
/// structured JSON under `input`, so we parse on the way out and fall
/// back to `{}` on malformed JSON.
fn tool_call_to_use_block(tc: &ToolCall) -> Value {
    let input: Value = serde_json::from_str(&tc.function.arguments).unwrap_or_else(|_| json!({}));
    json!({
        "toolUse": {
            "toolUseId": tc.id,
            "name": tc.function.name,
            "input": input,
        }
    })
}

pub(crate) fn map_stop_reason(finish: Option<&str>) -> &'static str {
    match finish {
        Some("stop") => "end_turn",
        Some("length") => "max_tokens",
        Some("tool_calls") => "tool_use",
        _ => "end_turn",
    }
}

fn to_bedrock_response(resp: ChatResponse, latency_ms: u64) -> Value {
    let choice = resp.choices.into_iter().next();
    let (text, tool_calls, finish) = match choice {
        Some(c) => (
            c.message.content.as_text(),
            c.message.tool_calls.unwrap_or_default(),
            c.finish_reason,
        ),
        None => (String::new(), Vec::new(), None),
    };
    let mut blocks: Vec<Value> = Vec::new();
    if !text.is_empty() {
        blocks.push(json!({ "text": text }));
    }
    for tc in &tool_calls {
        blocks.push(tool_call_to_use_block(tc));
    }
    if blocks.is_empty() {
        blocks.push(json!({ "text": "" }));
    }
    let stop_reason = if !tool_calls.is_empty() {
        "tool_use"
    } else {
        map_stop_reason(finish.as_deref())
    };
    let usage = resp.usage.unwrap_or_default();
    json!({
        "output": {
            "message": {
                "role": "assistant",
                "content": blocks,
            }
        },
        "stopReason": stop_reason,
        "usage": {
            "inputTokens": usage.prompt_tokens,
            "outputTokens": usage.completion_tokens,
            "totalTokens": usage.prompt_tokens + usage.completion_tokens,
        },
        "metrics": { "latencyMs": latency_ms }
    })
}

pub async fn invoke(
    backends: &BedrockBackends,
    bedrock_id: &str,
    input: &Value,
) -> Result<Value, AwsError> {
    let started = std::time::Instant::now();
    let resp = super::call_chat(backends, bedrock_id, |tag| to_openai_request(tag, input)).await?;
    Ok(to_bedrock_response(
        resp,
        started.elapsed().as_millis() as u64,
    ))
}

pub async fn invoke_streaming(
    backends: &BedrockBackends,
    bedrock_id: &str,
    input: &Value,
) -> Result<Value, AwsError> {
    let started = std::time::Instant::now();
    let acc =
        super::call_chat_stream(backends, bedrock_id, |tag| to_openai_request(tag, input)).await?;
    let has_tools = !acc.tool_calls.is_empty();
    let stop_reason = if has_tools {
        "tool_use"
    } else {
        map_stop_reason(acc.finish_reason.as_deref())
    };
    let mut events = Vec::new();
    events.push(json!({ "messageStart": { "role": "assistant" } }));
    if !acc.text.is_empty() {
        events.push(json!({
            "contentBlockDelta": {
                "delta": { "text": &acc.text },
                "contentBlockIndex": 0
            }
        }));
    }
    events.push(json!({ "contentBlockStop": { "contentBlockIndex": 0 } }));
    for (i, tc) in acc.tool_calls.iter().enumerate() {
        let idx = i + 1;
        events.push(json!({
            "contentBlockStart": {
                "start": { "toolUse": { "toolUseId": tc.id, "name": tc.name } },
                "contentBlockIndex": idx
            }
        }));
        if !tc.arguments.is_empty() {
            events.push(json!({
                "contentBlockDelta": {
                    "delta": { "toolUse": { "input": tc.arguments } },
                    "contentBlockIndex": idx
                }
            }));
        }
        events.push(json!({ "contentBlockStop": { "contentBlockIndex": idx } }));
    }
    events.push(json!({ "messageStop": { "stopReason": stop_reason } }));
    events.push(json!({
        "metadata": {
            "usage": {
                "inputTokens":  acc.prompt_tokens,
                "outputTokens": acc.completion_tokens,
                "totalTokens":  acc.prompt_tokens + acc.completion_tokens,
            },
            "metrics": { "latencyMs": started.elapsed().as_millis() as u64 }
        }
    }));
    Ok(super::converse_stream_envelope(events))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_messages_with_system_blocks() {
        let input = json!({
            "modelId": "anthropic.claude-3-5-sonnet-20241022-v2:0",
            "system": [{ "text": "You are helpful." }],
            "messages": [
                { "role": "user", "content": [{ "text": "Hi" }] }
            ],
            "inferenceConfig": { "maxTokens": 256, "temperature": 0.7 }
        });
        let req = to_openai_request("llama3.1:8b", &input).unwrap();
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, "system");
        assert_eq!(req.messages[0].content.as_text(), "You are helpful.");
        assert_eq!(req.messages[1].role, "user");
        assert_eq!(req.messages[1].content.as_text(), "Hi");
        assert_eq!(req.max_tokens, Some(256));
    }

    #[test]
    fn forwards_image_block_as_image_url_part() {
        let input = json!({
            "modelId": "anthropic.claude-3-5-sonnet-20241022-v2:0",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        { "text": "what is this?" },
                        { "image": {
                            "format": "jpeg",
                            "source": { "bytes": "/9j/4AAQ" }
                        }}
                    ]
                }
            ],
            "inferenceConfig": {}
        });
        let req = to_openai_request("llama3.1:8b", &input).unwrap();
        let parts = match &req.messages[0].content {
            MessageContent::Parts(p) => p,
            other => panic!("expected parts array, got {other:?}"),
        };
        assert_eq!(parts.len(), 2);
        match &parts[1] {
            ContentPart::ImageUrl { image_url } => {
                assert_eq!(image_url.url, "data:image/jpeg;base64,/9j/4AAQ");
            }
            other => panic!("expected image_url part, got {other:?}"),
        }
    }

    #[test]
    fn inlines_text_document_as_envelope() {
        // "name=hello.csv\nval=1" base64-encoded
        let csv_b64 = base64::engine::general_purpose::STANDARD.encode("a,b\n1,2\n");
        let input = json!({
            "modelId": "anthropic.claude-3-5-sonnet-20241022-v2:0",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        { "text": "summarize" },
                        { "document": {
                            "format": "csv",
                            "name": "rows",
                            "source": { "bytes": csv_b64 }
                        }}
                    ]
                }
            ],
            "inferenceConfig": {}
        });
        let req = to_openai_request("m", &input).unwrap();
        let s = match &req.messages[0].content {
            MessageContent::Text(s) => s.clone(),
            other => panic!("expected text-only collapse, got {other:?}"),
        };
        assert!(s.contains("summarize"));
        assert!(s.contains("<document name=\"rows\" format=\"csv\">"));
        assert!(s.contains("a,b"));
    }

    #[test]
    fn translates_tool_config_and_tool_use_messages() {
        let input = json!({
            "modelId": "anthropic.claude-3-5-sonnet-20241022-v2:0",
            "toolConfig": {
                "tools": [{ "toolSpec": {
                    "name": "get_weather",
                    "description": "Look up weather",
                    "inputSchema": { "json": {
                        "type": "object",
                        "properties": { "city": { "type": "string" } }
                    }}
                }}],
                "toolChoice": { "auto": {} }
            },
            "messages": [
                { "role": "user", "content": [{ "text": "What's the weather?" }] },
                { "role": "assistant", "content": [
                    { "toolUse": {
                        "toolUseId": "tu_1",
                        "name": "get_weather",
                        "input": { "city": "Kathmandu" }
                    }}
                ]},
                { "role": "user", "content": [
                    { "toolResult": {
                        "toolUseId": "tu_1",
                        "content": [{ "text": "20C" }]
                    }}
                ]}
            ],
            "inferenceConfig": {}
        });
        let req = to_openai_request("llama3.1:8b", &input).unwrap();
        // Request-level tool spec forwarded.
        let tools = req.tools.as_deref().expect("tools present");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function.name, "get_weather");
        assert_eq!(req.tool_choice, Some(json!("auto")));
        // Three OpenAI messages: user query, assistant tool_calls, tool result.
        assert_eq!(req.messages.len(), 3);
        assert_eq!(req.messages[1].role, "assistant");
        let calls = req.messages[1].tool_calls.as_deref().unwrap();
        assert_eq!(calls[0].id, "tu_1");
        assert_eq!(calls[0].function.name, "get_weather");
        assert_eq!(calls[0].function.arguments, "{\"city\":\"Kathmandu\"}");
        assert_eq!(req.messages[2].role, "tool");
        assert_eq!(req.messages[2].tool_call_id.as_deref(), Some("tu_1"));
        assert_eq!(req.messages[2].content.as_text(), "20C");
    }

    #[test]
    fn emits_tool_use_block_on_response() {
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
        let v = to_bedrock_response(resp, 1);
        assert_eq!(v["stopReason"], "tool_use");
        let use_block = &v["output"]["message"]["content"][0]["toolUse"];
        assert_eq!(use_block["toolUseId"], "tu_99");
        assert_eq!(use_block["name"], "get_weather");
        assert_eq!(use_block["input"]["city"], "Pokhara");
    }

    #[test]
    fn shapes_bedrock_response_with_metrics() {
        let resp = ChatResponse {
            id: "x".into(),
            model: "m".into(),
            choices: vec![super::super::openai::ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: "Hi back".into(),
                    ..ChatMessage::default()
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(super::super::openai::Usage {
                prompt_tokens: 4,
                completion_tokens: 3,
                total_tokens: 7,
            }),
        };
        let v = to_bedrock_response(resp, 42);
        assert_eq!(v["output"]["message"]["content"][0]["text"], "Hi back");
        assert_eq!(v["stopReason"], "end_turn");
        assert_eq!(v["usage"]["totalTokens"], 7);
        assert_eq!(v["metrics"]["latencyMs"], 42);
    }
}
