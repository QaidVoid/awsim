//! OpenAI-compatible request / response types — the wire format
//! every translator emits, regardless of which Bedrock model
//! family the SDK called with. Backed by Ollama / LM Studio /
//! llama.cpp server / vLLM, all of which clone this contract.
//!
//! Only the fields awsim actually emits / consumes are modeled —
//! per OpenAI's compatibility doc most additional fields are
//! optional and unused servers tolerate them either way.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// "system" | "user" | "assistant"
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    #[serde(default)]
    #[allow(dead_code)]
    pub id: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub model: String,
    #[serde(default)]
    pub choices: Vec<ChatChoice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatChoice {
    #[serde(default)]
    #[allow(dead_code)]
    pub index: u32,
    #[serde(default)]
    pub message: ChatMessage,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

impl Default for ChatMessage {
    fn default() -> Self {
        Self {
            role: "assistant".to_string(),
            content: String::new(),
        }
    }
}

#[allow(dead_code)] // total_tokens used by Converse metrics in commit #7
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    #[allow(dead_code)] // total_tokens used by Converse metrics in commit #7
    pub total_tokens: u32,
}
