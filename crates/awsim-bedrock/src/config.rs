//! TOML config loader for the Bedrock proxy.
//!
//! A single `--bedrock-config` file can declare multiple
//! OpenAI-compatible backends and pin individual Bedrock model ids to
//! specific backends, so one awsim instance can fan out across
//! Ollama (local), Groq (hosted), OpenAI (embeddings), etc.
//!
//! Full example covering every section the loader understands:
//!
//! ```toml
//! default_backend = "ollama"
//!
//! # Reusable credentials. One secret can back multiple backends.
//! [credentials.groq]
//! api_key_env = "GROQ_API_KEY"
//!
//! # Backends. The `provider` field is a catalog key that the UI
//! # uses for logos / templates; the runtime never branches on it.
//! [backends.ollama]
//! provider = "ollama"
//! endpoint = "http://localhost:11434/v1"
//!
//! [backends.groq]
//! provider = "groq"
//! endpoint = "https://api.groq.com/openai/v1"
//! credential = "groq"
//!
//! # Multi-target alias groups, keyed by Bedrock id. The resolver
//! # checks aliases before the legacy [invoke] / [embed] tables and
//! # walks targets in declaration order; the first whose backend is
//! # configured and not currently marked Down wins. On a retriable
//! # upstream error (5xx / 408 / 429 / network), the runtime rolls
//! # forward to the next target automatically. Per-target overrides
//! # (timeout_ms / max_tokens / temperature) shape the upstream
//! # request only for that target.
//! [aliases."anthropic.claude-3-5-sonnet-20241022-v2:0"]
//! kind = "chat"            # "chat" | "embed"
//! strategy = "first"
//! targets = [
//!   { backend = "groq",   tag = "llama-3.3-70b-versatile", timeout_ms = 8000 },
//!   { backend = "ollama", tag = "llama3.1:8b", temperature = 0.2 },
//! ]
//!
//! # Legacy single-target mappings still work. The runtime prefers
//! # aliases when both exist for the same id.
//! [invoke]
//! "anthropic.claude-3-haiku-20240307-v1:0" = "llama3.1:8b"
//!
//! [embed]
//! "amazon.titan-embed-text-v2:0" = "nomic-embed-text"
//! ```
//!
//! Inline `api_key` on a backend is supported but discouraged;
//! prefer a `[credentials.*]` block with `api_key_env` so secrets
//! stay out of the config file.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::aliases::AliasSpec;
use crate::backend::{BedrockBackend, BedrockBackends};
use crate::model_map::{ModelEntry, ModelMap};

/// Declarative spec describing a multi-backend Bedrock proxy setup.
/// Used both as the on-disk TOML config schema and as the JSON-shaped
/// payload for the runtime-config API. Build into a live registry
/// with [`build_from_spec`].
#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct BedrockSpec {
    /// Name of the `[backends.<name>]` entry to fall back to when an
    /// `[invoke]` / `[embed]` entry is just a bare backend tag.
    /// Optional — without it, bare-tag entries don't route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_backend: Option<String>,
    /// Named, reusable API-key credentials. A single credential can be
    /// referenced from multiple `[backends.*]` blocks via the
    /// `credential = "<name>"` field, so a fan-out setup (e.g. two
    /// Groq backends with different endpoints but the same key)
    /// doesn't have to restate the same secret in two places.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub credentials: HashMap<String, CredentialSpec>,
    #[serde(default)]
    pub backends: HashMap<String, BackendSpec>,
    /// Multi-target alias groups keyed by Bedrock id. The resolver
    /// checks these before falling through to the legacy
    /// `[invoke]` / `[embed]` tables, so a user can layer a
    /// primary + fallback ordering on top of a stable Bedrock id
    /// without rewriting the existing single-target mappings.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub aliases: HashMap<String, AliasSpec>,
    #[serde(default)]
    pub invoke: HashMap<String, ModelEntry>,
    #[serde(default)]
    pub embed: HashMap<String, ModelEntry>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BackendSpec {
    pub endpoint: String,
    /// Catalog key (e.g. "ollama", "openai", "groq", "custom") this
    /// backend was wired from. Pure metadata — the runtime never
    /// branches on it; the UI uses it to render the right logo,
    /// notes, and curated model list. Absent on backends configured
    /// via the legacy CLI flags or hand-edited TOML.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Reference into the top-level `[credentials.*]` table. When set,
    /// the resolved credential's `api_key` is used. Mutually exclusive
    /// with the legacy inline `api_key` / `api_key_env` fields on this
    /// backend; setting both yields a hard error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
    /// Inline API key. Legacy shape, still supported for back-compat;
    /// prefer `credential` once Phase 1 credentials exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Name of an env var holding the API key. Resolved at build time;
    /// missing env var is a hard error so misconfigured backends fail
    /// fast rather than silently sending unauthenticated requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
}

/// Reusable API-key credential. Same `(api_key | api_key_env)`
/// shape as the legacy per-backend fields, just lifted into a
/// named table so multiple backends can share one secret.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct CredentialSpec {
    /// Inline API key. Discouraged; prefer `api_key_env` so secrets
    /// stay out of the on-disk config file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Name of an env var holding the API key. Resolved at build
    /// time; missing env var is a hard error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
}

#[derive(Debug, Error)]
pub enum BedrockConfigError {
    #[error("reading bedrock config {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("parsing bedrock config {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("bedrock config backend '{backend}' uses both api_key and api_key_env — pick one")]
    KeyConflict { backend: String },
    #[error("bedrock config backend '{backend}' references env var ${var} but it is unset")]
    MissingEnvVar { backend: String, var: String },
    #[error(
        "bedrock config backend '{backend}' sets both credential = '{credential}' and a legacy api_key/api_key_env; pick one"
    )]
    CredentialAndLegacyKey { backend: String, credential: String },
    #[error(
        "bedrock config backend '{backend}' references credential '{credential}' but there is no matching [credentials.{credential}] entry"
    )]
    UnknownCredential { backend: String, credential: String },
    #[error("bedrock config credential '{credential}' sets both api_key and api_key_env; pick one")]
    CredentialKeyConflict { credential: String },
    #[error("bedrock config credential '{credential}' references env var ${var} but it is unset")]
    CredentialMissingEnvVar { credential: String, var: String },
    #[error("bedrock config default_backend = '{name}' has no matching [backends.{name}] section")]
    UnknownDefault { name: String },
    #[error(
        "bedrock config [{table}] entry '{id}' routes to backend '{backend}' but it has no [backends.{backend}] section"
    )]
    UnknownEntryBackend {
        table: &'static str,
        id: String,
        backend: String,
    },
    #[error("bedrock config [aliases.{id}] has no targets; declare at least one backend + tag")]
    AliasNoTargets { id: String },
    #[error(
        "bedrock config [aliases.{id}] target {index} routes to backend '{backend}' but it has no [backends.{backend}] section"
    )]
    AliasUnknownBackend {
        id: String,
        index: usize,
        backend: String,
    },
    #[error("bedrock config has no [backends.*] sections — at least one backend is required")]
    NoBackends,
}

/// Parse a TOML config file and build a fully-resolved `BedrockBackends`
/// registry. Resolves `api_key_env` against the current process
/// environment, merges declared mappings on top of the built-in
/// defaults, and validates that every routed entry points at a real
/// backend block.
pub fn load_from_file(path: &Path) -> Result<BedrockBackends, BedrockConfigError> {
    let raw = std::fs::read_to_string(path).map_err(|e| BedrockConfigError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    load_from_str(&raw, path.display().to_string(), |var| {
        std::env::var(var).ok()
    })
}

fn load_from_str(
    raw: &str,
    path_for_errors: String,
    env_lookup: impl Fn(&str) -> Option<String>,
) -> Result<BedrockBackends, BedrockConfigError> {
    let spec: BedrockSpec = toml::from_str(raw).map_err(|e| BedrockConfigError::Parse {
        path: path_for_errors,
        source: e,
    })?;
    build_from_spec(spec, env_lookup)
}

/// Validate a [`BedrockSpec`] and build a live `BedrockBackends`
/// registry. Resolves `api_key_env` via the supplied closure (use
/// [`std::env::var`] in production; tests pass a stub). Reused by the
/// TOML loader and the runtime-config API.
pub fn build_from_spec(
    spec: BedrockSpec,
    env_lookup: impl Fn(&str) -> Option<String>,
) -> Result<BedrockBackends, BedrockConfigError> {
    if spec.backends.is_empty() {
        return Err(BedrockConfigError::NoBackends);
    }

    if let Some(name) = spec.default_backend.as_deref()
        && !spec.backends.contains_key(name)
    {
        return Err(BedrockConfigError::UnknownDefault {
            name: name.to_string(),
        });
    }

    let resolved_credentials = resolve_credentials(&spec.credentials, &env_lookup)?;

    let mut built: HashMap<String, BedrockBackend> = HashMap::new();
    for (name, bc) in spec.backends {
        let api_key = resolve_api_key(&name, &bc, &resolved_credentials, &env_lookup)?;
        built.insert(
            name.clone(),
            BedrockBackend::new(name, bc.endpoint, api_key),
        );
    }

    validate_entries("invoke", &spec.invoke, &built)?;
    validate_entries("embed", &spec.embed, &built)?;
    validate_aliases(&spec.aliases, &built)?;

    let mut model_map = ModelMap::defaults();
    for (k, v) in spec.invoke {
        model_map.invoke.insert(k, v);
    }
    for (k, v) in spec.embed {
        model_map.embed.insert(k, v);
    }

    Ok(BedrockBackends::new_with_aliases(
        built,
        spec.default_backend,
        model_map,
        spec.aliases,
    ))
}

fn resolve_credentials(
    credentials: &HashMap<String, CredentialSpec>,
    env_lookup: &impl Fn(&str) -> Option<String>,
) -> Result<HashMap<String, Option<String>>, BedrockConfigError> {
    let mut out = HashMap::with_capacity(credentials.len());
    for (name, cs) in credentials {
        let key = match (&cs.api_key, &cs.api_key_env) {
            (Some(_), Some(_)) => {
                return Err(BedrockConfigError::CredentialKeyConflict {
                    credential: name.clone(),
                });
            }
            (Some(k), None) => Some(k.clone()),
            (None, Some(var)) => Some(env_lookup(var).ok_or_else(|| {
                BedrockConfigError::CredentialMissingEnvVar {
                    credential: name.clone(),
                    var: var.clone(),
                }
            })?),
            (None, None) => None,
        };
        out.insert(name.clone(), key);
    }
    Ok(out)
}

fn resolve_api_key(
    name: &str,
    bc: &BackendSpec,
    credentials: &HashMap<String, Option<String>>,
    env_lookup: &impl Fn(&str) -> Option<String>,
) -> Result<Option<String>, BedrockConfigError> {
    if let Some(cred_name) = &bc.credential {
        if bc.api_key.is_some() || bc.api_key_env.is_some() {
            return Err(BedrockConfigError::CredentialAndLegacyKey {
                backend: name.to_string(),
                credential: cred_name.clone(),
            });
        }
        let key =
            credentials
                .get(cred_name)
                .ok_or_else(|| BedrockConfigError::UnknownCredential {
                    backend: name.to_string(),
                    credential: cred_name.clone(),
                })?;
        return Ok(key.clone());
    }

    match (&bc.api_key, &bc.api_key_env) {
        (Some(_), Some(_)) => Err(BedrockConfigError::KeyConflict {
            backend: name.to_string(),
        }),
        (Some(key), None) => Ok(Some(key.clone())),
        (None, Some(var)) => env_lookup(var)
            .ok_or_else(|| BedrockConfigError::MissingEnvVar {
                backend: name.to_string(),
                var: var.clone(),
            })
            .map(Some),
        (None, None) => Ok(None),
    }
}

fn validate_entries(
    table: &'static str,
    entries: &HashMap<String, ModelEntry>,
    backends: &HashMap<String, BedrockBackend>,
) -> Result<(), BedrockConfigError> {
    for (id, entry) in entries {
        if let Some(name) = entry.backend()
            && !backends.contains_key(name)
        {
            return Err(BedrockConfigError::UnknownEntryBackend {
                table,
                id: id.clone(),
                backend: name.to_string(),
            });
        }
    }
    Ok(())
}

fn validate_aliases(
    aliases: &HashMap<String, AliasSpec>,
    backends: &HashMap<String, BedrockBackend>,
) -> Result<(), BedrockConfigError> {
    for (id, alias) in aliases {
        if alias.targets.is_empty() {
            return Err(BedrockConfigError::AliasNoTargets { id: id.clone() });
        }
        for (index, target) in alias.targets.iter().enumerate() {
            if !backends.contains_key(&target.backend) {
                return Err(BedrockConfigError::AliasUnknownBackend {
                    id: id.clone(),
                    index,
                    backend: target.backend.clone(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn loads_multi_backend_with_routed_entry() {
        let toml_src = r#"
default_backend = "ollama"

[backends.ollama]
endpoint = "http://localhost:11434/v1"

[backends.groq]
endpoint = "https://api.groq.com/openai/v1"
api_key = "gsk-test"

[invoke]
"anthropic.claude-3-5-sonnet-20241022-v2:0" = { backend = "groq", tag = "llama-3.3-70b-versatile" }
"#;
        let bs = load_from_str(toml_src, "test".into(), empty_env).unwrap();
        let (backend, tag) = bs
            .resolve_invoke("anthropic.claude-3-5-sonnet-20241022-v2:0")
            .unwrap();
        assert_eq!(backend.name(), "groq");
        assert_eq!(tag, "llama-3.3-70b-versatile");
        assert_eq!(backend.api_key(), Some("gsk-test"));

        // A bare-tag entry from defaults still routes through the default backend.
        let (backend, _) = bs
            .resolve_invoke("anthropic.claude-3-haiku-20240307-v1:0")
            .unwrap();
        assert_eq!(backend.name(), "ollama");
        assert_eq!(bs.default_name(), Some("ollama"));
        assert_eq!(bs.backend_names(), vec!["groq", "ollama"]);
    }

    #[test]
    fn resolves_api_key_from_env() {
        let toml_src = r#"
[backends.groq]
endpoint = "https://api.groq.com/openai/v1"
api_key_env = "GROQ_API_KEY"
"#;
        let bs = load_from_str(toml_src, "test".into(), |v| {
            (v == "GROQ_API_KEY").then_some("env-key".to_string())
        })
        .unwrap();
        let backend = bs.get_backend("groq").unwrap();
        assert_eq!(backend.api_key(), Some("env-key"));
    }

    fn expect_err(result: Result<BedrockBackends, BedrockConfigError>) -> BedrockConfigError {
        match result {
            Ok(_) => panic!("expected an error, got Ok"),
            Err(e) => e,
        }
    }

    #[test]
    fn missing_env_var_is_error() {
        let toml_src = r#"
[backends.groq]
endpoint = "https://api.groq.com/openai/v1"
api_key_env = "MISSING_VAR"
"#;
        let err = expect_err(load_from_str(toml_src, "test".into(), empty_env));
        assert!(matches!(
            err,
            BedrockConfigError::MissingEnvVar { ref backend, ref var }
                if backend == "groq" && var == "MISSING_VAR"
        ));
    }

    #[test]
    fn both_api_key_fields_is_error() {
        let toml_src = r#"
[backends.x]
endpoint = "http://x"
api_key = "a"
api_key_env = "B"
"#;
        let err = expect_err(load_from_str(toml_src, "test".into(), empty_env));
        assert!(matches!(err, BedrockConfigError::KeyConflict { .. }));
    }

    #[test]
    fn no_backends_is_error() {
        let err = expect_err(load_from_str("", "test".into(), empty_env));
        assert!(matches!(err, BedrockConfigError::NoBackends));
    }

    #[test]
    fn unknown_default_backend_is_error() {
        let toml_src = r#"
default_backend = "ghost"

[backends.real]
endpoint = "http://r"
"#;
        let err = expect_err(load_from_str(toml_src, "test".into(), empty_env));
        assert!(matches!(err, BedrockConfigError::UnknownDefault { .. }));
    }

    #[test]
    fn entry_with_unknown_backend_is_error() {
        let toml_src = r#"
[backends.ollama]
endpoint = "http://o"

[invoke]
"anthropic.claude-v2" = { backend = "ghost", tag = "x" }
"#;
        let err = expect_err(load_from_str(toml_src, "test".into(), empty_env));
        assert!(matches!(
            err,
            BedrockConfigError::UnknownEntryBackend {
                table: "invoke",
                ..
            }
        ));
    }

    #[test]
    fn shared_credential_across_two_backends() {
        let toml_src = r#"
[credentials.groq]
api_key_env = "GROQ_API_KEY"

[backends.groq_a]
endpoint = "https://api.groq.com/openai/v1"
credential = "groq"

[backends.groq_b]
endpoint = "https://api.groq.com/openai/v1"
credential = "groq"
"#;
        let bs = load_from_str(toml_src, "test".into(), |v| {
            (v == "GROQ_API_KEY").then_some("env-key".to_string())
        })
        .unwrap();
        assert_eq!(bs.get_backend("groq_a").unwrap().api_key(), Some("env-key"));
        assert_eq!(bs.get_backend("groq_b").unwrap().api_key(), Some("env-key"));
    }

    #[test]
    fn inline_credential_resolves() {
        let toml_src = r#"
[credentials.openai]
api_key = "sk-test"

[backends.openai]
endpoint = "https://api.openai.com/v1"
credential = "openai"
"#;
        let bs = load_from_str(toml_src, "test".into(), empty_env).unwrap();
        assert_eq!(bs.get_backend("openai").unwrap().api_key(), Some("sk-test"));
    }

    #[test]
    fn credential_and_legacy_key_is_error() {
        let toml_src = r#"
[credentials.x]
api_key = "a"

[backends.x]
endpoint = "http://x"
credential = "x"
api_key = "b"
"#;
        let err = expect_err(load_from_str(toml_src, "test".into(), empty_env));
        assert!(matches!(
            err,
            BedrockConfigError::CredentialAndLegacyKey { ref backend, ref credential }
                if backend == "x" && credential == "x"
        ));
    }

    #[test]
    fn unknown_credential_is_error() {
        let toml_src = r#"
[backends.x]
endpoint = "http://x"
credential = "ghost"
"#;
        let err = expect_err(load_from_str(toml_src, "test".into(), empty_env));
        assert!(matches!(
            err,
            BedrockConfigError::UnknownCredential { ref backend, ref credential }
                if backend == "x" && credential == "ghost"
        ));
    }

    #[test]
    fn credential_with_both_key_fields_is_error() {
        let toml_src = r#"
[credentials.x]
api_key = "a"
api_key_env = "B"

[backends.b]
endpoint = "http://b"
credential = "x"
"#;
        let err = expect_err(load_from_str(toml_src, "test".into(), empty_env));
        assert!(matches!(
            err,
            BedrockConfigError::CredentialKeyConflict { ref credential } if credential == "x"
        ));
    }

    #[test]
    fn provider_field_round_trips_through_spec() {
        // Spec carries `provider` as opaque UI metadata; the resolver
        // never branches on it. Just verify it survives the loader so
        // the runtime-config GET surfaces what the user picked in the
        // wizard.
        let toml_src = r#"
[backends.openai]
endpoint = "https://api.openai.com/v1"
provider = "openai"
api_key = "sk-test"
"#;
        let spec: BedrockSpec = toml::from_str(toml_src).unwrap();
        assert_eq!(
            spec.backends
                .get("openai")
                .and_then(|b| b.provider.as_deref()),
            Some("openai")
        );
        let bs = build_from_spec(spec, empty_env).unwrap();
        assert_eq!(bs.get_backend("openai").unwrap().api_key(), Some("sk-test"));
    }

    #[test]
    fn credential_missing_env_var_is_error() {
        let toml_src = r#"
[credentials.x]
api_key_env = "MISSING"

[backends.b]
endpoint = "http://b"
credential = "x"
"#;
        let err = expect_err(load_from_str(toml_src, "test".into(), empty_env));
        assert!(matches!(
            err,
            BedrockConfigError::CredentialMissingEnvVar { ref credential, ref var }
                if credential == "x" && var == "MISSING"
        ));
    }

    #[test]
    fn alias_first_target_wins_when_resolvable() {
        let toml_src = r#"
[backends.ollama]
endpoint = "http://localhost:11434/v1"

[backends.groq]
endpoint = "https://api.groq.com/openai/v1"

[aliases."anthropic.claude-3-5-sonnet-20241022-v2:0"]
kind = "chat"
targets = [
  { backend = "groq", tag = "llama-3.3-70b-versatile" },
  { backend = "ollama", tag = "llama3.1:8b" },
]
"#;
        let bs = load_from_str(toml_src, "test".into(), empty_env).unwrap();
        let (backend, tag) = bs
            .resolve_invoke("anthropic.claude-3-5-sonnet-20241022-v2:0")
            .unwrap();
        assert_eq!(backend.name(), "groq");
        assert_eq!(tag, "llama-3.3-70b-versatile");
    }

    #[test]
    fn alias_falls_through_when_primary_backend_unconfigured() {
        // The "groq" target points at a backend the registry was
        // never asked to build, so the resolver must skip it and
        // take the next target ("ollama") under the First strategy.
        // Mirrors the case where a user removes a backend without
        // cleaning up the alias.
        let toml_src = r#"
[backends.ollama]
endpoint = "http://localhost:11434/v1"

[aliases."anthropic.claude-3-5-sonnet-20241022-v2:0"]
kind = "chat"
targets = [
  { backend = "groq", tag = "llama-3.3-70b-versatile" },
  { backend = "ollama", tag = "llama3.1:8b" },
]
"#;
        // The loader would reject the unknown target at validate
        // time, so we bypass it: build the registry by hand to
        // simulate a registry that was constructed before the user
        // removed `groq` from `[backends.*]`.
        let backends = {
            let mut m = HashMap::new();
            m.insert(
                "ollama".to_string(),
                BedrockBackend::new("ollama".into(), "http://localhost:11434/v1".into(), None),
            );
            m
        };
        let mut aliases = HashMap::new();
        aliases.insert(
            "anthropic.claude-3-5-sonnet-20241022-v2:0".to_string(),
            AliasSpec {
                kind: crate::aliases::AliasKind::Chat,
                strategy: crate::aliases::AliasStrategy::First,
                targets: vec![
                    crate::aliases::AliasTarget {
                        backend: "groq".into(),
                        tag: "llama-3.3-70b-versatile".into(),
                        timeout_ms: None,
                        max_tokens: None,
                        temperature: None,
                    },
                    crate::aliases::AliasTarget {
                        backend: "ollama".into(),
                        tag: "llama3.1:8b".into(),
                        timeout_ms: None,
                        max_tokens: None,
                        temperature: None,
                    },
                ],
            },
        );
        let bs = BedrockBackends::new_with_aliases(
            backends,
            Some("ollama".into()),
            ModelMap::defaults(),
            aliases,
        );
        let (backend, tag) = bs
            .resolve_invoke("anthropic.claude-3-5-sonnet-20241022-v2:0")
            .unwrap();
        assert_eq!(backend.name(), "ollama");
        assert_eq!(tag, "llama3.1:8b");
        // Suppress unused warning from toml_src above; this test
        // documents the intended TOML shape even though we build
        // the registry manually.
        let _ = toml_src;
    }

    #[test]
    fn alias_kind_must_match_call_side() {
        // An alias declared as "chat" should NOT be consulted on
        // an embed call, even if the bedrock_id technically appears
        // in a default embed map. Keeps chat-mapped ids from
        // accidentally hijacking embedding requests. With a default
        // backend set, the embed call falls through to the built-in
        // titan-embed → nomic-embed-text mapping rather than picking
        // up the alias's WRONG tag.
        let toml_src = r#"
default_backend = "ollama"

[backends.ollama]
endpoint = "http://localhost:11434/v1"

[aliases."amazon.titan-embed-text-v2:0"]
kind = "chat"
targets = [{ backend = "ollama", tag = "WRONG" }]
"#;
        let bs = load_from_str(toml_src, "test".into(), empty_env).unwrap();
        let (_backend, tag) = bs.resolve_embed("amazon.titan-embed-text-v2:0").unwrap();
        assert_eq!(tag, "nomic-embed-text"); // built-in default
    }

    #[test]
    fn alias_with_empty_targets_is_error() {
        let toml_src = r#"
[backends.ollama]
endpoint = "http://o"

[aliases."x"]
kind = "chat"
targets = []
"#;
        let err = expect_err(load_from_str(toml_src, "test".into(), empty_env));
        assert!(matches!(err, BedrockConfigError::AliasNoTargets { ref id } if id == "x"));
    }

    #[test]
    fn alias_target_overrides_round_trip() {
        // Inline overrides should make it through the loader so
        // the runtime layer can apply them at request-build time.
        let toml_src = r#"
[backends.ollama]
endpoint = "http://localhost:11434/v1"

[backends.groq]
endpoint = "https://api.groq.com/openai/v1"

[aliases."anthropic.claude-3-5-sonnet-20241022-v2:0"]
kind = "chat"
targets = [
  { backend = "groq",   tag = "llama-3.3-70b-versatile", timeout_ms = 8000, max_tokens = 1024, temperature = 0.7 },
  { backend = "ollama", tag = "llama3.1:8b", timeout_ms = 60000 },
]
"#;
        let spec: BedrockSpec = toml::from_str(toml_src).unwrap();
        let alias = spec
            .aliases
            .get("anthropic.claude-3-5-sonnet-20241022-v2:0")
            .unwrap();
        assert_eq!(alias.targets[0].timeout_ms, Some(8000));
        assert_eq!(alias.targets[0].max_tokens, Some(1024));
        assert_eq!(alias.targets[0].temperature, Some(0.7));
        assert_eq!(alias.targets[1].timeout_ms, Some(60000));
        assert_eq!(alias.targets[1].max_tokens, None);
        assert_eq!(alias.targets[1].temperature, None);
        // build still validates fine
        let _ = build_from_spec(spec, empty_env).unwrap();
    }

    #[test]
    fn alias_with_unknown_backend_is_error() {
        let toml_src = r#"
[backends.ollama]
endpoint = "http://o"

[aliases."x"]
kind = "chat"
targets = [
  { backend = "ollama", tag = "ok" },
  { backend = "ghost", tag = "bad" },
]
"#;
        let err = expect_err(load_from_str(toml_src, "test".into(), empty_env));
        assert!(matches!(
            err,
            BedrockConfigError::AliasUnknownBackend { ref id, index: 1, ref backend }
                if id == "x" && backend == "ghost"
        ));
    }
}
