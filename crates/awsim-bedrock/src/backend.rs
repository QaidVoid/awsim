//! HTTP client + config for OpenAI-compatible LLM backends.
//!
//! `BedrockBackend` represents one endpoint (URL + optional bearer
//! key + reqwest client). `BedrockBackends` is a named registry —
//! one or many — plus the model map and a default name. The runtime
//! takes a registry handle and resolves `(backend, tag)` per request,
//! so a single awsim instance can fan out across Ollama, Groq,
//! OpenAI, etc. simultaneously.
//!
//! When no backend is configured at all, the runtime falls back to
//! deterministic canned responses so SDK code keeps working in CI.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::aliases::{AliasKind, AliasSpec, alias_view};
use crate::model_map::{ModelEntry, ModelMap};

const DEFAULT_BACKEND_NAME: &str = "default";

/// A single OpenAI-compatible endpoint. Cheap-to-clone Arc handle.
#[derive(Clone)]
pub struct BedrockBackend(Arc<BackendInner>);

struct BackendInner {
    name: String,
    client: reqwest::Client,
    /// Base URL ending in `/v1` (OpenAI compat). Trailing slash optional.
    endpoint: String,
    /// `Authorization: Bearer …` value when set; absent for backends
    /// like a default Ollama install that don't require auth.
    api_key: Option<String>,
}

impl BedrockBackend {
    pub fn new(name: String, endpoint: String, api_key: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            // Backends like Ollama load the first request slowly while
            // the model warms up. Keep the timeout generous so first-
            // request loads don't surface as backend errors.
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("reqwest::Client::build with default config should not fail");
        Self(Arc::new(BackendInner {
            name,
            client,
            endpoint: endpoint.trim_end_matches('/').to_string(),
            api_key,
        }))
    }

    pub fn name(&self) -> &str {
        &self.0.name
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
}

/// Named-backend registry. Owns the model map. The runtime asks
/// `resolve_invoke` / `resolve_embed` for each request and gets back
/// the right `(BedrockBackend, tag)` pair — even when different
/// Bedrock ids point at different backends.
#[derive(Clone)]
pub struct BedrockBackends(Arc<BackendsInner>);

struct BackendsInner {
    backends: HashMap<String, BedrockBackend>,
    default_name: Option<String>,
    model_map: ModelMap,
    /// Multi-target alias groups, checked before the legacy
    /// `model_map`. Empty in the legacy single-backend setup.
    aliases: HashMap<String, AliasSpec>,
}

impl BedrockBackends {
    /// Construct a registry with one backend wired as the default.
    /// Used when only `--bedrock-backend` is supplied (no TOML).
    pub fn single(backend: BedrockBackend, model_map: ModelMap) -> Self {
        let name = backend.name().to_string();
        let mut backends = HashMap::new();
        backends.insert(name.clone(), backend);
        Self(Arc::new(BackendsInner {
            backends,
            default_name: Some(name),
            model_map,
            aliases: HashMap::new(),
        }))
    }

    /// Construct a registry from a pre-built map. Used by the TOML
    /// loader in commit B.
    pub fn new(
        backends: HashMap<String, BedrockBackend>,
        default_name: Option<String>,
        model_map: ModelMap,
    ) -> Self {
        Self(Arc::new(BackendsInner {
            backends,
            default_name,
            model_map,
            aliases: HashMap::new(),
        }))
    }

    /// Construct a registry with explicit alias groups. Used by
    /// `build_from_spec` once Phase 3 adds the `[aliases]` section.
    pub fn new_with_aliases(
        backends: HashMap<String, BedrockBackend>,
        default_name: Option<String>,
        model_map: ModelMap,
        aliases: HashMap<String, AliasSpec>,
    ) -> Self {
        Self(Arc::new(BackendsInner {
            backends,
            default_name,
            model_map,
            aliases,
        }))
    }

    pub fn model_map(&self) -> &ModelMap {
        &self.0.model_map
    }

    pub fn default_name(&self) -> Option<&str> {
        self.0.default_name.as_deref()
    }

    pub fn backend_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.0.backends.keys().cloned().collect();
        names.sort();
        names
    }

    pub fn get_backend(&self, name: &str) -> Option<&BedrockBackend> {
        self.0.backends.get(name)
    }

    /// Resolve a Bedrock invoke id. Returns the backend handle plus
    /// the backend-side model tag, or `None` if there's no mapping.
    ///
    /// Aliases take precedence over the legacy `[invoke]` table:
    /// when an alias is declared for this id, the resolver walks
    /// its `targets` in declaration order under the `First`
    /// strategy and returns the first one whose backend exists.
    /// That gives users a static fallback (skip targets whose
    /// backend was removed) without requiring runtime error
    /// fallback, which lands in Phase 4.
    pub fn resolve_invoke(&self, bedrock_id: &str) -> Option<(&BedrockBackend, &str)> {
        if let Some(hit) = self.resolve_alias(bedrock_id, AliasKind::Chat) {
            return Some(hit);
        }
        let entry = self.0.model_map.lookup(bedrock_id, false)?;
        self.resolve_entry(entry)
    }

    /// Resolve a Bedrock embedding id. Same as `resolve_invoke` but
    /// hits the embed-only mappings first.
    pub fn resolve_embed(&self, bedrock_id: &str) -> Option<(&BedrockBackend, &str)> {
        if let Some(hit) = self.resolve_alias(bedrock_id, AliasKind::Embed) {
            return Some(hit);
        }
        let entry = self.0.model_map.lookup(bedrock_id, true)?;
        self.resolve_entry(entry)
    }

    fn resolve_alias(
        &self,
        bedrock_id: &str,
        wanted_kind: AliasKind,
    ) -> Option<(&BedrockBackend, &str)> {
        let alias = self.0.aliases.get(bedrock_id)?;
        if alias.kind != wanted_kind {
            return None;
        }
        for target in &alias.targets {
            if let Some(backend) = self.0.backends.get(&target.backend) {
                return Some((backend, target.tag.as_str()));
            }
        }
        None
    }

    fn resolve_entry<'a>(&'a self, entry: &'a ModelEntry) -> Option<(&'a BedrockBackend, &'a str)> {
        let name = entry.backend().or(self.0.default_name.as_deref())?;
        let backend = self.0.backends.get(name)?;
        Some((backend, entry.tag()))
    }

    /// Render the live registry as JSON for the admin endpoint /
    /// UI surface. API keys are reported as a `hasApiKey` boolean
    /// rather than the secret itself so this is safe to expose.
    pub fn redacted_view(&self) -> Value {
        let mut backends: Vec<Value> = self
            .0
            .backends
            .values()
            .map(|b| {
                json!({
                    "name": b.name(),
                    "endpoint": b.endpoint(),
                    "hasApiKey": b.api_key().is_some(),
                })
            })
            .collect();
        backends.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));

        let mut invoke: Vec<Value> = self
            .0
            .model_map
            .invoke
            .iter()
            .map(|(id, e)| entry_view(id, e))
            .collect();
        invoke.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

        let mut embed: Vec<Value> = self
            .0
            .model_map
            .embed
            .iter()
            .map(|(id, e)| entry_view(id, e))
            .collect();
        embed.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

        let mut aliases: Vec<Value> = self
            .0
            .aliases
            .iter()
            .map(|(id, a)| alias_view(id, a))
            .collect();
        aliases.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

        json!({
            "defaultBackend": self.0.default_name,
            "backends": backends,
            "invoke": invoke,
            "embed": embed,
            "aliases": aliases,
        })
    }
}

fn entry_view(id: &str, entry: &ModelEntry) -> Value {
    json!({
        "id": id,
        "tag": entry.tag(),
        "backend": entry.backend(),
    })
}

/// Convenience for the common single-endpoint setup driven by
/// `--bedrock-backend` / `--bedrock-api-key`.
pub fn single_default(
    endpoint: String,
    api_key: Option<String>,
    model_map: ModelMap,
) -> BedrockBackends {
    let backend = BedrockBackend::new(DEFAULT_BACKEND_NAME.to_string(), endpoint, api_key);
    BedrockBackends::single(backend, model_map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_default_routes_via_default_backend() {
        let bs = single_default(
            "http://localhost:11434/v1/".to_string(),
            None,
            ModelMap::defaults(),
        );
        let (backend, tag) = bs
            .resolve_invoke("anthropic.claude-3-5-sonnet-20241022-v2:0")
            .unwrap();
        assert_eq!(backend.endpoint(), "http://localhost:11434/v1");
        assert_eq!(tag, "llama3.1:8b");
    }

    #[test]
    fn routed_entry_picks_named_backend() {
        let mut backends = HashMap::new();
        backends.insert(
            "ollama".to_string(),
            BedrockBackend::new("ollama".into(), "http://o/v1".into(), None),
        );
        backends.insert(
            "groq".to_string(),
            BedrockBackend::new("groq".into(), "http://g/v1".into(), Some("gsk-test".into())),
        );
        let mut map = ModelMap::defaults();
        map.invoke.insert(
            "anthropic.claude-3-5-sonnet-20241022-v2:0".into(),
            ModelEntry::Routed {
                backend: "groq".into(),
                tag: "llama-3.3-70b".into(),
            },
        );
        let bs = BedrockBackends::new(backends, Some("ollama".into()), map);
        let (backend, tag) = bs
            .resolve_invoke("anthropic.claude-3-5-sonnet-20241022-v2:0")
            .unwrap();
        assert_eq!(backend.name(), "groq");
        assert_eq!(tag, "llama-3.3-70b");

        // Other ids fall through to default backend.
        let (backend, _) = bs
            .resolve_invoke("anthropic.claude-3-haiku-20240307-v1:0")
            .unwrap();
        assert_eq!(backend.name(), "ollama");
    }

    #[test]
    fn redacted_view_omits_api_key_secrets() {
        let mut backends = HashMap::new();
        backends.insert(
            "groq".to_string(),
            BedrockBackend::new(
                "groq".into(),
                "https://api.groq.com/v1".into(),
                Some("gsk-secret".into()),
            ),
        );
        backends.insert(
            "ollama".to_string(),
            BedrockBackend::new("ollama".into(), "http://localhost:11434/v1".into(), None),
        );
        let mut map = ModelMap::defaults();
        map.invoke.insert(
            "anthropic.claude-v2".into(),
            ModelEntry::Routed {
                backend: "groq".into(),
                tag: "llama-3.3-70b".into(),
            },
        );
        let bs = BedrockBackends::new(backends, Some("ollama".into()), map);
        let view = bs.redacted_view();
        assert_eq!(view["defaultBackend"], "ollama");

        let groq = view["backends"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["name"] == "groq")
            .unwrap();
        assert_eq!(groq["hasApiKey"], true);
        assert!(view.to_string().find("gsk-secret").is_none());

        let ollama = view["backends"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["name"] == "ollama")
            .unwrap();
        assert_eq!(ollama["hasApiKey"], false);

        let routed = view["invoke"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == "anthropic.claude-v2")
            .unwrap();
        assert_eq!(routed["backend"], "groq");
        assert_eq!(routed["tag"], "llama-3.3-70b");
    }

    #[test]
    fn unknown_named_backend_returns_none() {
        let backends = HashMap::new();
        let mut map = ModelMap::defaults();
        map.invoke.insert(
            "anthropic.claude-v2".into(),
            ModelEntry::Routed {
                backend: "ghost".into(),
                tag: "x".into(),
            },
        );
        let bs = BedrockBackends::new(backends, Some("default".into()), map);
        assert!(bs.resolve_invoke("anthropic.claude-v2").is_none());
    }
}
