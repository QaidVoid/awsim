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

use super::openai::{ChatMessage, ChatRequest, ChatResponse};
use crate::backend::BedrockBackends;

fn to_openai_request(model_tag: &str, input: &Value) -> Result<ChatRequest, AwsError> {
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
                content: text,
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
        let mut text = String::new();
        if let Some(blocks) = m.get("content").and_then(Value::as_array) {
            for block in blocks {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(t);
                }
            }
        }
        messages.push(ChatMessage {
            role: role.to_string(),
            content: text,
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
    })
}

fn map_stop_reason(finish: Option<&str>) -> &'static str {
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
        Some(c) => (c.message.content.clone(), c.finish_reason.clone()),
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
        assert_eq!(req.messages[0].content, "You are helpful.");
        assert_eq!(req.messages[1].role, "user");
        assert_eq!(req.messages[1].content, "Hi");
        assert_eq!(req.max_tokens, Some(256));
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
