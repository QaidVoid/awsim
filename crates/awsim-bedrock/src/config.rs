//! TOML config loader for the Bedrock proxy.
//!
//! A single `--bedrock-config` file can declare multiple
//! OpenAI-compatible backends and pin individual Bedrock model ids to
//! specific backends — so one awsim instance can fan out across
//! Ollama (local), Groq (hosted), OpenAI (embeddings), etc.
//!
//! Example file:
//!
//! ```toml
//! default_backend = "ollama"
//!
//! [backends.ollama]
//! endpoint = "http://localhost:11434/v1"
//!
//! [backends.groq]
//! endpoint = "https://api.groq.com/openai/v1"
//! api_key_env = "GROQ_API_KEY"
//!
//! [invoke]
//! "anthropic.claude-3-5-sonnet-20241022-v2:0" = { backend = "groq", tag = "llama-3.3-70b-versatile" }
//! "anthropic.claude-3-haiku-20240307-v1:0" = "llama3.1:8b"
//!
//! [embed]
//! "amazon.titan-embed-text-v2:0" = "nomic-embed-text"
//! ```
//!
//! Inline `api_key` is supported but discouraged; prefer
//! `api_key_env` so secrets stay out of the config file.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

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
    #[serde(default)]
    pub invoke: HashMap<String, ModelEntry>,
    #[serde(default)]
    pub embed: HashMap<String, ModelEntry>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BackendSpec {
    pub endpoint: String,
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

    let mut model_map = ModelMap::defaults();
    for (k, v) in spec.invoke {
        model_map.invoke.insert(k, v);
    }
    for (k, v) in spec.embed {
        model_map.embed.insert(k, v);
    }

    Ok(BedrockBackends::new(built, spec.default_backend, model_map))
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
}
