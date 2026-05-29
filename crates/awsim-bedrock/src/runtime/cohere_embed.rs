//! Cohere Embed format ↔ OpenAI /v1/embeddings translator.
//!
//! Cohere Embed request body:
//! ```text
//! { "texts": ["...","..."], "input_type": "search_document",
//!   "truncate": "NONE" }
//! ```
//! Cohere Embed response body:
//! ```text
//! { "id": "...",
//!   "embeddings": [[...], [...]],
//!   "texts": ["...","..."],
//!   "response_type": "embeddings_floats" }
//! ```

use awsim_core::AwsError;
use serde_json::{Value, json};
use uuid::Uuid;

use super::openai::{EmbeddingsInput, EmbeddingsRequest, EmbeddingsResponse};
use crate::backend::BedrockBackends;

fn to_openai_request(model_tag: &str, body: &Value) -> Result<EmbeddingsRequest, AwsError> {
    let texts = body
        .get("texts")
        .and_then(Value::as_array)
        .ok_or_else(|| AwsError::bad_request("ValidationException", "texts is required"))?;
    let strs: Vec<String> = texts
        .iter()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect();
    if strs.is_empty() {
        return Err(AwsError::bad_request(
            "ValidationException",
            "texts must contain at least one string",
        ));
    }
    Ok(EmbeddingsRequest {
        model: model_tag.to_string(),
        input: EmbeddingsInput::Many(strs),
    })
}

fn to_bedrock_response(texts: &[String], resp: EmbeddingsResponse) -> Value {
    let embeddings: Vec<Vec<f32>> = resp.data.into_iter().map(|d| d.embedding).collect();
    json!({
        "id": Uuid::new_v4().to_string(),
        "embeddings": embeddings,
        "texts": texts,
        "response_type": "embeddings_floats",
    })
}

pub async fn invoke(
    backends: &BedrockBackends,
    bedrock_id: &str,
    body: &Value,
) -> Result<Value, AwsError> {
    let texts: Vec<String> = body
        .get("texts")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let resp = super::call_embed(backends, bedrock_id, |tag| to_openai_request(tag, body)).await?;
    let prompt_tokens = resp.usage.clone().unwrap_or_default().prompt_tokens;
    let mut value = to_bedrock_response(&texts, resp);
    let patch = super::pricing_patch(backends, bedrock_id, prompt_tokens, 0);
    super::merge_pricing_into(&mut value, patch);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_text_array() {
        let body = json!({ "texts": ["a", "b"] });
        let req = to_openai_request("nomic-embed-text", &body).unwrap();
        match req.input {
            EmbeddingsInput::Many(v) => assert_eq!(v, vec!["a".to_string(), "b".to_string()]),
            _ => panic!("expected array input"),
        }
    }

    #[test]
    fn empty_texts_is_validation_error() {
        let err = to_openai_request("m", &json!({ "texts": [] })).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }
}
