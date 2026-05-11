//! Meta Llama format ↔ OpenAI chat.completions translator.
//!
//! Llama request body:
//! ```text
//! { "prompt": "...", "max_gen_len": 512, "temperature": 0.7, "top_p": 0.9 }
//! ```
//! Llama response body:
//! ```text
//! { "generation": "...", "prompt_token_count": N,
//!   "generation_token_count": M, "stop_reason": "stop"|"length" }
//! ```

use awsim_core::AwsError;
use serde_json::{Value, json};

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
            .get("max_gen_len")
            .and_then(Value::as_u64)
            .map(|v| v as u32),
        temperature: body
            .get("temperature")
            .and_then(Value::as_f64)
            .map(|v| v as f32),
        top_p: body.get("top_p").and_then(Value::as_f64).map(|v| v as f32),
        ..ChatRequest::default()
    })
}

fn to_bedrock_response(resp: ChatResponse) -> Value {
    let choice = resp.choices.into_iter().next();
    let (text, finish) = match &choice {
        Some(c) => (c.message.content.as_text(), c.finish_reason.clone()),
        None => (String::new(), None),
    };
    let usage = resp.usage.unwrap_or_default();
    json!({
        "generation": text,
        "prompt_token_count": usage.prompt_tokens,
        "generation_token_count": usage.completion_tokens,
        "stop_reason": finish.unwrap_or_else(|| "stop".to_string()),
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

pub async fn invoke_streaming(
    backends: &BedrockBackends,
    bedrock_id: &str,
    body: &Value,
) -> Result<Value, AwsError> {
    let acc =
        super::call_chat_stream(backends, bedrock_id, |tag| to_openai_request(tag, body)).await?;
    let chunk = json!({
        "generation": acc.text,
        "prompt_token_count": acc.prompt_tokens,
        "generation_token_count": acc.completion_tokens,
        "stop_reason": acc.finish_reason.unwrap_or_else(|| "stop".to_string()),
    });
    Ok(super::stream_envelope(vec![chunk]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_prompt_into_user_message() {
        let body = json!({ "prompt": "Hi", "max_gen_len": 64 });
        let req = to_openai_request("llama3.1:8b", &body).unwrap();
        assert_eq!(req.messages[0].content.as_text(), "Hi");
        assert_eq!(req.max_tokens, Some(64));
    }
}
