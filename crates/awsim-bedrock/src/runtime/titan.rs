//! Amazon Titan text format ↔ OpenAI chat.completions translator.
//!
//! Titan request body:
//! ```text
//! { "inputText": "...",
//!   "textGenerationConfig": { "maxTokenCount":N, "temperature":F, "topP":F, "stopSequences":[...] } }
//! ```
//! Titan response body:
//! ```text
//! { "inputTextTokenCount": N,
//!   "results": [{ "tokenCount":M, "outputText":"...", "completionReason":"FINISH"|"LENGTH" }] }
//! ```

use awsim_core::AwsError;
use serde_json::{Value, json};

use super::openai::{ChatMessage, ChatRequest, ChatResponse};
use crate::backend::BedrockBackends;

fn to_openai_request(model_tag: &str, body: &Value) -> Result<ChatRequest, AwsError> {
    let prompt = body
        .get("inputText")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("ValidationException", "inputText is required"))?
        .to_string();
    let cfg = &body["textGenerationConfig"];
    Ok(ChatRequest {
        model: model_tag.to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: prompt,
        }],
        max_tokens: cfg
            .get("maxTokenCount")
            .and_then(Value::as_u64)
            .map(|v| v as u32),
        temperature: cfg
            .get("temperature")
            .and_then(Value::as_f64)
            .map(|v| v as f32),
        top_p: cfg.get("topP").and_then(Value::as_f64).map(|v| v as f32),
        stop: cfg.get("stopSequences").and_then(Value::as_array).map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        }),
        stream: None,
        stream_options: None,
    })
}

fn to_bedrock_response(resp: ChatResponse) -> Value {
    let choice = resp.choices.into_iter().next();
    let (text, finish) = match &choice {
        Some(c) => (c.message.content.clone(), c.finish_reason.clone()),
        None => (String::new(), None),
    };
    let completion_reason = match finish.as_deref() {
        Some("length") => "LENGTH",
        _ => "FINISH",
    };
    let usage = resp.usage.unwrap_or_default();
    json!({
        "inputTextTokenCount": usage.prompt_tokens,
        "results": [{
            "tokenCount": usage.completion_tokens,
            "outputText": text,
            "completionReason": completion_reason,
        }]
    })
}

pub async fn invoke(
    backends: &BedrockBackends,
    bedrock_id: &str,
    body: &Value,
) -> Result<Value, AwsError> {
    super::call_chat(backends, bedrock_id, |tag| to_openai_request(tag, body))
        .await
        .map(to_bedrock_response)
}

/// Single accumulated chunk in Titan streaming format.
pub async fn invoke_streaming(
    backends: &BedrockBackends,
    bedrock_id: &str,
    body: &Value,
) -> Result<Value, AwsError> {
    let acc =
        super::call_chat_stream(backends, bedrock_id, |tag| to_openai_request(tag, body)).await?;
    let completion_reason = match acc.finish_reason.as_deref() {
        Some("length") => "LENGTH",
        _ => "FINISH",
    };
    let chunk = json!({
        "outputText": acc.text,
        "completionReason": completion_reason,
        "index": 0,
        "inputTextTokenCount": acc.prompt_tokens,
        "totalOutputTextTokenCount": acc.completion_tokens,
    });
    Ok(super::stream_envelope(vec![chunk]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_input_text_into_user_message() {
        let body = json!({
            "inputText": "Hello",
            "textGenerationConfig": { "maxTokenCount": 128, "temperature": 0.5 }
        });
        let req = to_openai_request("llama3.1:8b", &body).unwrap();
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert_eq!(req.messages[0].content, "Hello");
        assert_eq!(req.max_tokens, Some(128));
    }

    #[test]
    fn maps_finish_reason_to_completion_reason() {
        let resp = ChatResponse {
            id: "x".into(),
            model: "m".into(),
            choices: vec![super::super::openai::ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: "Hi".into(),
                },
                finish_reason: Some("length".into()),
            }],
            usage: Some(super::super::openai::Usage {
                prompt_tokens: 1,
                completion_tokens: 2,
                total_tokens: 3,
            }),
        };
        let v = to_bedrock_response(resp);
        assert_eq!(v["results"][0]["completionReason"], "LENGTH");
        assert_eq!(v["results"][0]["outputText"], "Hi");
    }
}
