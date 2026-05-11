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
use serde_json::{Value, json};

use super::openai::{
    ChatMessage, ChatRequest, ChatResponse, ContentPart, ImageUrl, MessageContent,
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
        let content = extract_content(m.get("content").and_then(Value::as_array));
        messages.push(ChatMessage {
            role: role.to_string(),
            content,
        });
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

/// Walk a Converse content array and produce an OpenAI-compat
/// `MessageContent`. Text blocks accumulate; `image` blocks become
/// `image_url` parts with a base64 data URL the proxied backend can
/// consume directly. `document`, `video`, `toolUse`, and `toolResult`
/// blocks are dropped with a warning since the OpenAI compatibility
/// surface and most local backends have no equivalent. When no images
/// are present the parts list collapses back to a plain string so
/// text-only backends keep working.
fn extract_content(blocks: Option<&Vec<Value>>) -> MessageContent {
    let Some(blocks) = blocks else {
        return MessageContent::Text(String::new());
    };
    let mut parts: Vec<ContentPart> = Vec::new();
    for block in blocks {
        if let Some(t) = block.get("text").and_then(Value::as_str) {
            parts.push(ContentPart::Text {
                text: t.to_string(),
            });
            continue;
        }
        if let Some(img) = block.get("image") {
            match converse_image_to_data_url(img) {
                Some(url) => parts.push(ContentPart::ImageUrl {
                    image_url: ImageUrl { url },
                }),
                None => warn!("Converse image block missing source.bytes, dropped"),
            }
            continue;
        }
        for unsupported in ["document", "video", "toolUse", "toolResult"] {
            if block.get(unsupported).is_some() {
                warn!(
                    block_kind = unsupported,
                    "Converse content block dropped, backend doesn't support it"
                );
                break;
            }
        }
    }
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
    let (text, finish) = match &choice {
        Some(c) => (c.message.content.as_text(), c.finish_reason.clone()),
        None => (String::new(), None),
    };
    let stop_reason = map_stop_reason(finish.as_deref());
    let usage = resp.usage.unwrap_or_default();
    json!({
        "output": {
            "message": {
                "role": "assistant",
                "content": [{ "text": text }],
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
    let stop_reason = map_stop_reason(acc.finish_reason.as_deref());
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
    fn drops_document_blocks_with_warning() {
        let input = json!({
            "modelId": "anthropic.claude-3-5-sonnet-20241022-v2:0",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        { "text": "summarize" },
                        { "document": {
                            "format": "pdf",
                            "name": "report",
                            "source": { "bytes": "JVBERi0=" }
                        }}
                    ]
                }
            ],
            "inferenceConfig": {}
        });
        let req = to_openai_request("m", &input).unwrap();
        match &req.messages[0].content {
            MessageContent::Text(s) => assert_eq!(s, "summarize"),
            other => panic!("expected text-only collapse, got {other:?}"),
        }
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
