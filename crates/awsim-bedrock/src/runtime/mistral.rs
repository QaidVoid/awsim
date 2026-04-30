//! Mistral format ↔ OpenAI chat.completions translator.
//!
//! Mistral request body:
//! ```text
//! { "prompt": "<s>[INST]...[/INST]", "max_tokens": 512,
//!   "temperature": 0.7, "top_p": 0.9, "top_k": 50, "stop": [...] }
//! ```
//! Mistral response body:
//! ```text
//! { "outputs": [{ "text": "...", "stop_reason": "stop"|"length" }] }
//! ```

use awsim_core::AwsError;
use serde_json::{Value, json};

use super::openai::{ChatMessage, ChatRequest, ChatResponse};
use crate::backend::BedrockBackend;

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
            content: prompt,
        }],
        max_tokens: body
            .get("max_tokens")
            .and_then(Value::as_u64)
            .map(|v| v as u32),
        temperature: body
            .get("temperature")
            .and_then(Value::as_f64)
            .map(|v| v as f32),
        top_p: body.get("top_p").and_then(Value::as_f64).map(|v| v as f32),
        stop: body.get("stop").and_then(Value::as_array).map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        }),
        stream: None,
    })
}

fn to_bedrock_response(resp: ChatResponse) -> Value {
    let choice = resp.choices.into_iter().next();
    let (text, finish) = match &choice {
        Some(c) => (c.message.content.clone(), c.finish_reason.clone()),
        None => (String::new(), None),
    };
    json!({
        "outputs": [{
            "text": text,
            "stop_reason": finish.unwrap_or_else(|| "stop".to_string()),
        }]
    })
}

pub async fn invoke(
    backend: &BedrockBackend,
    bedrock_id: &str,
    body: &Value,
) -> Result<Value, AwsError> {
    super::call_chat(backend, bedrock_id, |tag| to_openai_request(tag, body))
        .await
        .map(to_bedrock_response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_prompt_with_stop_sequences() {
        let body = json!({
            "prompt": "[INST]Hi[/INST]",
            "max_tokens": 32,
            "stop": ["[/INST]"]
        });
        let req = to_openai_request("mistral:7b", &body).unwrap();
        assert_eq!(req.messages[0].content, "[INST]Hi[/INST]");
        assert_eq!(req.stop.as_deref(), Some(&["[/INST]".to_string()][..]));
    }
}
