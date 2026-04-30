//! Amazon Titan Embed format ↔ OpenAI /v1/embeddings translator.
//!
//! Titan Embed request body:
//! ```text
//! { "inputText": "...", "dimensions": 1024, "normalize": true }   // v2
//! { "inputText": "..." }                                          // v1
//! ```
//! Titan Embed response body:
//! ```text
//! { "embedding": [...], "inputTextTokenCount": N }
//! ```

use awsim_core::AwsError;
use serde_json::{Value, json};

use super::openai::{EmbeddingsInput, EmbeddingsRequest, EmbeddingsResponse};
use crate::backend::BedrockBackends;

fn to_openai_request(model_tag: &str, body: &Value) -> Result<EmbeddingsRequest, AwsError> {
    let input = body
        .get("inputText")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("ValidationException", "inputText is required"))?;
    Ok(EmbeddingsRequest {
        model: model_tag.to_string(),
        input: EmbeddingsInput::Single(input.to_string()),
    })
}

fn to_bedrock_response(resp: EmbeddingsResponse) -> Value {
    let embedding = resp
        .data
        .into_iter()
        .next()
        .map(|d| d.embedding)
        .unwrap_or_default();
    let prompt_tokens = resp.usage.unwrap_or_default().prompt_tokens;
    json!({
        "embedding": embedding,
        "inputTextTokenCount": prompt_tokens,
    })
}

pub async fn invoke(
    backends: &BedrockBackends,
    bedrock_id: &str,
    body: &Value,
) -> Result<Value, AwsError> {
    super::call_embed(backends, bedrock_id, |tag| to_openai_request(tag, body))
        .await
        .map(to_bedrock_response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_input_text() {
        let body = json!({ "inputText": "Hello world" });
        let req = to_openai_request("nomic-embed-text", &body).unwrap();
        match req.input {
            EmbeddingsInput::Single(s) => assert_eq!(s, "Hello world"),
            _ => panic!("expected single input"),
        }
    }

    #[test]
    fn missing_input_is_validation_error() {
        let err = to_openai_request("m", &json!({})).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }
}
