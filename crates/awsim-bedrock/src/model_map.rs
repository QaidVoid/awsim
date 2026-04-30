//! Bedrock model id → backend model tag mapping. Powers the
//! `awsim-bedrock` proxy: every Bedrock-flavoured invocation is
//! translated to an OpenAI-compatible request, but the model name
//! the backend (Ollama, LM Studio, llama.cpp server, vLLM, …)
//! actually understands is different from the AWS-side
//! `anthropic.claude-3-5-sonnet-20241022-v2:0`.
//!
//! The default map skews toward Ollama / Llama because that's the
//! most common local-LLM setup. Users override anything they need
//! via a TOML file passed with `--bedrock-model-map`.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Deserialize, Default, Clone)]
pub struct ModelMap {
    /// `bedrock-id → backend-model-tag` for chat / completion / Converse
    /// dispatch. Used by `InvokeModel`, `InvokeModelWithResponseStream`,
    /// `Converse`, `ConverseStream`.
    #[serde(default)]
    pub invoke: HashMap<String, String>,
    /// `bedrock-id → backend-model-tag` for `/v1/embeddings` dispatch.
    /// Used by `InvokeModel` when the bedrock id is an embedding model
    /// (Titan Embed, Cohere Embed).
    #[serde(default)]
    pub embed: HashMap<String, String>,
}

#[derive(Debug, Error)]
pub enum ModelMapError {
    #[error("reading model map {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("parsing model map {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
}

impl ModelMap {
    /// Built-in defaults. Errs toward small Llama-family models so
    /// `ollama pull llama3.1:8b nomic-embed-text` is enough to make
    /// every Bedrock-id-the-SDKs-will-throw-at-us land somewhere.
    pub fn defaults() -> Self {
        let invoke = [
            // Anthropic
            ("anthropic.claude-3-5-sonnet-20241022-v2:0", "llama3.1:8b"),
            ("anthropic.claude-3-5-sonnet-20240620-v1:0", "llama3.1:8b"),
            ("anthropic.claude-3-5-haiku-20241022-v1:0", "llama3.1:8b"),
            ("anthropic.claude-3-haiku-20240307-v1:0", "llama3.1:8b"),
            ("anthropic.claude-3-opus-20240229-v1:0", "llama3.1:8b"),
            ("anthropic.claude-3-sonnet-20240229-v1:0", "llama3.1:8b"),
            ("anthropic.claude-v2:1", "llama3.1:8b"),
            ("anthropic.claude-v2", "llama3.1:8b"),
            ("anthropic.claude-instant-v1", "llama3.1:8b"),
            // Meta Llama
            ("meta.llama3-1-405b-instruct-v1:0", "llama3.1:8b"),
            ("meta.llama3-1-70b-instruct-v1:0", "llama3.1:70b"),
            ("meta.llama3-1-8b-instruct-v1:0", "llama3.1:8b"),
            ("meta.llama3-70b-instruct-v1:0", "llama3:70b"),
            ("meta.llama3-8b-instruct-v1:0", "llama3:8b"),
            ("meta.llama2-70b-chat-v1", "llama2:70b"),
            ("meta.llama2-13b-chat-v1", "llama2:13b"),
            // Amazon Titan
            ("amazon.titan-text-express-v1", "llama3.1:8b"),
            ("amazon.titan-text-lite-v1", "llama3.1:8b"),
            ("amazon.titan-text-premier-v1:0", "llama3.1:8b"),
            // Mistral
            ("mistral.mistral-7b-instruct-v0:2", "mistral:7b"),
            ("mistral.mixtral-8x7b-instruct-v0:1", "mixtral:8x7b"),
            ("mistral.mistral-large-2402-v1:0", "mistral:7b"),
            ("mistral.mistral-large-2407-v1:0", "mistral:7b"),
            // Cohere Command
            ("cohere.command-r-v1:0", "llama3.1:8b"),
            ("cohere.command-r-plus-v1:0", "llama3.1:8b"),
            ("cohere.command-text-v14", "llama3.1:8b"),
            ("cohere.command-light-text-v14", "llama3.1:8b"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        let embed = [
            // Amazon Titan Embed
            ("amazon.titan-embed-text-v1", "nomic-embed-text"),
            ("amazon.titan-embed-text-v2:0", "nomic-embed-text"),
            ("amazon.titan-embed-image-v1", "nomic-embed-text"),
            // Cohere Embed
            ("cohere.embed-english-v3", "nomic-embed-text"),
            ("cohere.embed-multilingual-v3", "nomic-embed-text"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        Self { invoke, embed }
    }

    /// Parse user overrides and merge on top of the defaults. User
    /// keys win over built-in keys so a single override doesn't
    /// require restating every other mapping.
    pub fn from_toml_str_with_defaults(toml_src: &str) -> Result<Self, toml::de::Error> {
        let user: ModelMap = toml::from_str(toml_src)?;
        Ok(Self::merge(Self::defaults(), user))
    }

    /// Same as `from_toml_str_with_defaults` but reads from a file path.
    pub fn from_toml_file_with_defaults(path: &Path) -> Result<Self, ModelMapError> {
        let raw = std::fs::read_to_string(path).map_err(|e| ModelMapError::Io {
            path: path.display().to_string(),
            source: e,
        })?;
        let user: ModelMap = toml::from_str(&raw).map_err(|e| ModelMapError::Parse {
            path: path.display().to_string(),
            source: e,
        })?;
        Ok(Self::merge(Self::defaults(), user))
    }

    fn merge(mut base: Self, user: Self) -> Self {
        for (k, v) in user.invoke {
            base.invoke.insert(k, v);
        }
        for (k, v) in user.embed {
            base.embed.insert(k, v);
        }
        base
    }

    /// Resolve a Bedrock id to a backend model tag. Tries `embed` first
    /// when `for_embedding` is true so an `amazon.titan-embed-…` id
    /// doesn't accidentally fall through to a chat-tier mapping.
    pub fn lookup(&self, bedrock_id: &str, for_embedding: bool) -> Option<&str> {
        if for_embedding {
            self.embed
                .get(bedrock_id)
                .or_else(|| self.invoke.get(bedrock_id))
                .map(|s| s.as_str())
        } else {
            self.invoke
                .get(bedrock_id)
                .or_else(|| self.embed.get(bedrock_id))
                .map(|s| s.as_str())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_common_ids() {
        let m = ModelMap::defaults();
        assert_eq!(
            m.lookup("anthropic.claude-3-5-sonnet-20241022-v2:0", false),
            Some("llama3.1:8b")
        );
        assert_eq!(
            m.lookup("amazon.titan-embed-text-v2:0", true),
            Some("nomic-embed-text")
        );
    }

    #[test]
    fn user_overrides_win_over_defaults() {
        let toml_src = r#"
[invoke]
"anthropic.claude-3-5-sonnet-20241022-v2:0" = "qwen2.5:32b"

[embed]
"amazon.titan-embed-text-v2:0" = "mxbai-embed-large"
"#;
        let m = ModelMap::from_toml_str_with_defaults(toml_src).unwrap();
        assert_eq!(
            m.lookup("anthropic.claude-3-5-sonnet-20241022-v2:0", false),
            Some("qwen2.5:32b")
        );
        assert_eq!(
            m.lookup("amazon.titan-embed-text-v2:0", true),
            Some("mxbai-embed-large")
        );
        // Unrelated default still present
        assert_eq!(
            m.lookup("anthropic.claude-3-haiku-20240307-v1:0", false),
            Some("llama3.1:8b")
        );
    }

    #[test]
    fn unknown_id_returns_none() {
        let m = ModelMap::defaults();
        assert_eq!(m.lookup("not.a.real-model", false), None);
    }

    #[test]
    fn embed_lookup_prefers_embed_table() {
        // If the same id sat in both tables (shouldn't, but defensive),
        // an embedding caller picks the embed mapping.
        let mut m = ModelMap::defaults();
        m.invoke.insert(
            "amazon.titan-embed-text-v1".to_string(),
            "wrong".to_string(),
        );
        assert_eq!(
            m.lookup("amazon.titan-embed-text-v1", true),
            Some("nomic-embed-text")
        );
    }
}
