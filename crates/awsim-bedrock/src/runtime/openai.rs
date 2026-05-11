//! OpenAI-compatible request / response types — the wire format
//! every translator emits, regardless of which Bedrock model
//! family the SDK called with. Backed by Ollama / LM Studio /
//! llama.cpp server / vLLM, all of which clone this contract.
//!
//! Only the fields awsim actually emits / consumes are modeled —
//! per OpenAI's compatibility doc most additional fields are
//! optional and unused servers tolerate them either way.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    /// String ("auto" / "none" / "required") or `{ "type": "function",
    /// "function": { "name": "..." } }`. Modeled as a raw Value so we
    /// can forward any shape the caller hands us.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
}

/// OpenAI tool spec. Today only `function` tools exist; we still tag
/// the `type` field so future tool kinds slot in cleanly.
#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDef {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: Value,
}

/// OpenAI streaming options. The only field that exists today is
/// `include_usage`, which makes the server emit a final chunk with
/// the token counters — without it usage stays absent and downstream
/// `prompt_tokens` / `completion_tokens` ride out at 0.
#[derive(Debug, Clone, Serialize)]
pub struct StreamOptions {
    pub include_usage: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// "system" | "user" | "assistant" | "tool"
    pub role: String,
    #[serde(default)]
    pub content: MessageContent,
    /// Assistant messages that invoke tools carry the call list here.
    /// Backends accept either `content: ""` or `content: null` next to
    /// `tool_calls`; we serialize empty content so both paths work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// Set on `role: tool` messages to bind the result to the
    /// originating call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// One entry in an assistant message's `tool_calls` array. OpenAI
/// streams arguments as a partial string, so it stays stringified on
/// the response side too; vendor translators that need a structured
/// view parse it themselves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "default_function_kind")]
    pub kind: String,
    pub function: ToolCallFunction,
}

fn default_function_kind() -> String {
    "function".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    /// JSON-encoded argument object. OpenAI ships this as a string,
    /// not a structured value, so callers parse it on demand.
    #[serde(default)]
    pub arguments: String,
}

/// OpenAI's chat content field is overloaded: either a plain string
/// or an array of typed parts for multimodal inputs (text + images).
/// Untagged so it round-trips both shapes. Backend responses are
/// almost always plain strings; requests with attachments go out as
/// the parts array.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

impl Default for MessageContent {
    fn default() -> Self {
        MessageContent::Text(String::new())
    }
}

impl From<String> for MessageContent {
    fn from(s: String) -> Self {
        MessageContent::Text(s)
    }
}

impl From<&str> for MessageContent {
    fn from(s: &str) -> Self {
        MessageContent::Text(s.to_string())
    }
}

impl MessageContent {
    pub fn text(s: impl Into<String>) -> Self {
        MessageContent::Text(s.into())
    }

    /// Flatten to plain text by joining text parts with newlines and
    /// dropping non-text parts. Used when shaping responses back into
    /// vendor envelopes that only carry a text payload.
    pub fn as_text(&self) -> String {
        match self {
            MessageContent::Text(s) => s.clone(),
            MessageContent::Parts(parts) => {
                let mut out = String::new();
                for p in parts {
                    if let ContentPart::Text { text } = p {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(text);
                    }
                }
                out
            }
        }
    }
}

/// One element of a multimodal `content` array. We model the two
/// part kinds the OpenAI compatibility surface defines and that local
/// runners (Ollama / llama.cpp / vLLM / LM Studio) actually accept:
/// `text` and `image_url` (data URL or http(s) URL).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
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
            content: MessageContent::default(),
            tool_calls: None,
            tool_call_id: None,
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

// ── /v1/embeddings ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum EmbeddingsInput {
    Single(String),
    Many(Vec<String>),
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingsRequest {
    pub model: String,
    pub input: EmbeddingsInput,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingsResponse {
    #[serde(default)]
    pub data: Vec<EmbeddingItem>,
    #[serde(default)]
    pub usage: Option<EmbeddingsUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingItem {
    #[serde(default)]
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EmbeddingsUsage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    #[allow(dead_code)]
    pub total_tokens: u32,
}
