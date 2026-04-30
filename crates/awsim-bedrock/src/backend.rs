//! HTTP client + config for an OpenAI-compatible LLM backend.
//!
//! The runtime layer (Anthropic / Titan / Llama / Cohere translators)
//! takes a `BedrockBackend` handle and POSTs to `<endpoint>/chat/completions`
//! or `<endpoint>/embeddings`. The translator owns the per-vendor request /
//! response shape; the backend just owns the HTTP transport + auth header
//! + model-id lookup.
//!
//! When no backend is configured (`--bedrock-backend` unset), the
//! runtime falls back to deterministic canned responses so CI keeps
//! working.

use std::sync::Arc;

use crate::model_map::ModelMap;

/// Cheap-to-clone handle. Composed once in the binary and shared
/// across every request.
#[derive(Clone)]
pub struct BedrockBackend(Arc<Inner>);

struct Inner {
    client: reqwest::Client,
    /// Base URL ending in `/v1` (OpenAI compat). Trailing slash optional.
    endpoint: String,
    /// `Authorization: Bearer …` value when set; absent for backends
    /// like a default Ollama install that don't require auth.
    api_key: Option<String>,
    model_map: ModelMap,
}

impl BedrockBackend {
    pub fn new(endpoint: String, api_key: Option<String>, model_map: ModelMap) -> Self {
        let client = reqwest::Client::builder()
            // Backends like Ollama load the first request slowly while
            // the model warms up. Keep the timeout generous so first-
            // request loads don't surface as backend errors.
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("reqwest::Client::build with default config should not fail");
        Self(Arc::new(Inner {
            client,
            endpoint: endpoint.trim_end_matches('/').to_string(),
            api_key,
            model_map,
        }))
    }

    pub fn endpoint(&self) -> &str {
        &self.0.endpoint
    }

    pub fn api_key(&self) -> Option<&str> {
        self.0.api_key.as_deref()
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.0.client
    }

    pub fn model_map(&self) -> &ModelMap {
        &self.0.model_map
    }

    /// Resolve a Bedrock id to the backend model tag. Returns `None`
    /// if the id isn't in the map — translators surface a clear
    /// `ResourceNotFoundException` when that happens, mirroring real
    /// Bedrock for unsupported model ids.
    pub fn resolve_invoke(&self, bedrock_id: &str) -> Option<&str> {
        self.0.model_map.lookup(bedrock_id, false)
    }

    pub fn resolve_embed(&self, bedrock_id: &str) -> Option<&str> {
        self.0.model_map.lookup(bedrock_id, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_strips_trailing_slash() {
        let b = BedrockBackend::new(
            "http://localhost:11434/v1/".to_string(),
            None,
            ModelMap::defaults(),
        );
        assert_eq!(b.endpoint(), "http://localhost:11434/v1");
    }

    #[test]
    fn api_key_is_optional() {
        let b = BedrockBackend::new(
            "http://localhost:11434/v1".to_string(),
            None,
            ModelMap::defaults(),
        );
        assert_eq!(b.api_key(), None);

        let b = BedrockBackend::new(
            "http://localhost:11434/v1".to_string(),
            Some("sk-test".to_string()),
            ModelMap::defaults(),
        );
        assert_eq!(b.api_key(), Some("sk-test"));
    }

    #[test]
    fn resolve_threads_through_model_map() {
        let b = BedrockBackend::new(
            "http://localhost/v1".to_string(),
            None,
            ModelMap::defaults(),
        );
        assert_eq!(
            b.resolve_invoke("anthropic.claude-3-5-sonnet-20241022-v2:0"),
            Some("llama3.1:8b")
        );
        assert_eq!(
            b.resolve_embed("amazon.titan-embed-text-v2:0"),
            Some("nomic-embed-text")
        );
    }
}
