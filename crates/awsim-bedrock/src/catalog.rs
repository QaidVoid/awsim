//! Bundled catalog of LLM providers + their well-known models.
//!
//! Powers the Model Gateway UI: the "Add backend" wizard offers a
//! provider picker (Ollama, OpenAI, Groq, ...) which then pre-fills
//! the endpoint, the auth field labels, and the env-var hint. Each
//! provider entry carries a curated list of model ids so users can
//! pick from a dropdown instead of typing strings.
//!
//! The JSON ships with the binary via `include_str!`. Parsing
//! happens once on first access and is cached for the lifetime of
//! the process — a malformed bundle panics at first request, which
//! is what we want: it's a compile-time-fixable bug, not a runtime
//! condition we should silently degrade through.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const RAW: &str = include_str!("../catalog/llm-providers.json");

/// Top-level catalog document. Versioned so we can evolve the
/// shape without ambiguity if a fork ever pins the file.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProviderCatalog {
    pub schema_version: u32,
    pub providers: Vec<Provider>,
}

/// One LLM backend the UI knows how to template. The `key` is the
/// stable identifier referenced from `BackendSpec.provider` once
/// Phase 2 lands.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Provider {
    pub key: String,
    pub name: String,
    /// Lucide icon name (e.g. "server", "sparkles", "zap"). The UI
    /// maps this onto a `@lucide/svelte` component.
    pub icon: String,
    pub kind: ProviderKind,
    /// Default endpoint URL the "Add backend" wizard fills in.
    /// Always ends in the OpenAI-compatible `/v1` (or `/openai/v1`)
    /// where applicable.
    pub endpoint_template: String,
    pub auth: AuthKind,
    /// Suggested environment-variable name for the API key. Surfaces
    /// as a placeholder in the env-var input field.
    #[serde(default)]
    pub env_hint: Option<String>,
    #[serde(default)]
    pub docs_url: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub models: Vec<CatalogModel>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// Runs locally (Ollama, LM Studio, vLLM, llama.cpp, LocalAI).
    Local,
    /// Hosted API (OpenAI, Anthropic, Groq, ...).
    Hosted,
    /// AWS-native (real Bedrock passthrough).
    Aws,
    /// Free-form OpenAI-compatible endpoint not covered by the catalog.
    Custom,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthKind {
    None,
    Bearer,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CatalogModel {
    pub id: String,
    /// Context window in tokens. Best-effort; some providers don't
    /// publish a canonical number.
    pub context: u32,
    /// e.g. `["text"]` or `["text", "image"]`.
    pub modalities: Vec<String>,
    /// `"chat"` for completion / Converse targets, `"embed"` for
    /// embedding endpoints. Drives which side of the model map
    /// (`invoke` vs `embed`) the UI suggests when wiring a mapping.
    pub kind: String,
}

/// Parse-once view of the bundled catalog. Panics on first call if
/// the bundled JSON is malformed — that's a compile-time-fixable
/// bug, so failing loudly at startup beats degrading silently.
pub fn catalog() -> &'static ProviderCatalog {
    static CELL: OnceLock<ProviderCatalog> = OnceLock::new();
    CELL.get_or_init(|| serde_json::from_str(RAW).expect("bundled llm-providers.json must parse"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_parses() {
        let c = catalog();
        assert_eq!(c.schema_version, 1);
        assert!(!c.providers.is_empty(), "expected at least one provider");
    }

    #[test]
    fn provider_keys_are_unique() {
        let c = catalog();
        let mut seen = std::collections::HashSet::new();
        for p in &c.providers {
            assert!(seen.insert(&p.key), "duplicate provider key: {}", p.key);
        }
    }

    #[test]
    fn well_known_providers_present() {
        let c = catalog();
        let keys: std::collections::HashSet<&str> =
            c.providers.iter().map(|p| p.key.as_str()).collect();
        for required in ["ollama", "openai", "groq", "bedrock", "custom"] {
            assert!(keys.contains(required), "missing provider: {required}");
        }
    }

    #[test]
    fn model_kinds_are_chat_or_embed() {
        let c = catalog();
        for p in &c.providers {
            for m in &p.models {
                assert!(
                    matches!(m.kind.as_str(), "chat" | "embed"),
                    "provider {} model {} has unexpected kind {}",
                    p.key,
                    m.id,
                    m.kind
                );
            }
        }
    }
}
