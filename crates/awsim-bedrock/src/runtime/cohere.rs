//! Cohere Command format ↔ OpenAI chat.completions translator.
//!
//! Cohere request body:
//! ```text
//! { "prompt": "...", "max_tokens": 512, "temperature": 0.7,
//!   "p": 0.9, "k": 50, "stop_sequences": [...], "return_likelihoods": "NONE" }
//! ```
//! Cohere response body:
//! ```text
//! { "id": "...", "prompt": "...",
//!   "generations": [{ "id": "...", "text": "...",
//!                     "finish_reason": "COMPLETE"|"MAX_TOKENS" }] }
//! ```

use awsim_core::AwsError;
use serde_json::{Value, json};
use uuid::Uuid;

use super::openai::{ChatMessage, ChatRequest, ChatResponse, MessageContent};
use crate::backend::BedrockBackends;

fn to_openai_request(model_tag: &str, body: &Value) -> Result<ChatRequest, AwsError> {
    let prompt = body
        .get("prompt")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("ValidationException", "prompt is required"))?
        .to_string();
    Ok(ChatRequest {
        model: model_tag.to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: MessageContent::text(prompt),
            ..ChatMessage::default()
        }],
        max_tokens: body
            .get("max_tokens")
            .and_then(Value::as_u64)
            .map(|v| v as u32),
        temperature: body
            .get("temperature")
            .and_then(Value::as_f64)
            .map(|v| v as f32),
        // Cohere calls top_p just `p`.
        top_p: body.get("p").and_then(Value::as_f64).map(|v| v as f32),
        stop: body
            .get("stop_sequences")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            }),
        ..ChatRequest::default()
    })
}

fn to_bedrock_response(prompt: &str, resp: ChatResponse) -> Value {
    let choice = resp.choices.into_iter().next();
    let (text, finish) = match &choice {
        Some(c) => (c.message.content.as_text(), c.finish_reason.clone()),
        None => (String::new(), None),
    };
    let cohere_finish = match finish.as_deref() {
        Some("length") => "MAX_TOKENS",
        _ => "COMPLETE",
    };
    json!({
        "id": Uuid::new_v4().to_string(),
        "prompt": prompt,
        "generations": [{
            "id": Uuid::new_v4().to_string(),
            "text": text,
            "finish_reason": cohere_finish,
        }]
    })
}

pub async fn invoke(
    backends: &BedrockBackends,
    bedrock_id: &str,
    body: &Value,
) -> Result<Value, AwsError> {
    let prompt = body
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    super::call_chat(backends, bedrock_id, |tag| to_openai_request(tag, body))
        .await
        .map(|resp| to_bedrock_response(&prompt, resp))
}

pub async fn invoke_streaming(
    backends: &BedrockBackends,
    bedrock_id: &str,
    body: &Value,
) -> Result<Value, AwsError> {
    let acc =
        super::call_chat_stream(backends, bedrock_id, |tag| to_openai_request(tag, body)).await?;
    let cohere_finish = match acc.finish_reason.as_deref() {
        Some("length") => "MAX_TOKENS",
        _ => "COMPLETE",
    };
    // Cohere streams one delta-style chunk plus a final is_finished:true
    // chunk; we emit both even though both come from the same accumulated
    // text, to keep the SDK's chunk-counting logic happy.
    let body = json!({
        "text": acc.text,
        "is_finished": false,
        "finish_reason": Value::Null,
    });
    let stop = json!({
        "is_finished": true,
        "finish_reason": cohere_finish,
    });
    Ok(super::stream_envelope(vec![body, stop]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_finish_reason_to_cohere_form() {
        let resp = ChatResponse {
            id: "x".into(),
            model: "m".into(),
            choices: vec![super::super::openai::ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: "Sure".into(),
                    ..ChatMessage::default()
                },
                finish_reason: Some("length".into()),
            }],
            usage: None,
        };
        let v = to_bedrock_response("Hi", resp);
        assert_eq!(v["generations"][0]["finish_reason"], "MAX_TOKENS");
        assert_eq!(v["prompt"], "Hi");
    }
}
