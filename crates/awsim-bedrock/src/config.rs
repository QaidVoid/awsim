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

use serde::Deserialize;
use thiserror::Error;

use crate::backend::{BedrockBackend, BedrockBackends};
use crate::model_map::{ModelEntry, ModelMap};

#[derive(Debug, Deserialize)]
struct ConfigFile {
    /// Name of the `[backends.<name>]` block to fall back to when an
    /// `[invoke]` / `[embed]` entry is just a bare backend tag.
    /// Optional — without it, bare-tag entries don't route.
    default_backend: Option<String>,
    #[serde(default)]
    backends: HashMap<String, BackendConfig>,
    #[serde(default)]
    invoke: HashMap<String, ModelEntry>,
    #[serde(default)]
    embed: HashMap<String, ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct BackendConfig {
    endpoint: String,
    /// Inline API key. Supported but discouraged; prefer `api_key_env`.
    api_key: Option<String>,
    /// Name of an env var holding the API key. Resolved at load time;
    /// missing env var is a hard error so misconfigured backends fail
    /// fast rather than silently sending unauthenticated requests.
    api_key_env: Option<String>,
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
    let cfg: ConfigFile = toml::from_str(raw).map_err(|e| BedrockConfigError::Parse {
        path: path_for_errors,
        source: e,
    })?;

    if cfg.backends.is_empty() {
        return Err(BedrockConfigError::NoBackends);
    }

    if let Some(name) = cfg.default_backend.as_deref()
        && !cfg.backends.contains_key(name)
    {
        return Err(BedrockConfigError::UnknownDefault {
            name: name.to_string(),
        });
    }

    let mut built: HashMap<String, BedrockBackend> = HashMap::new();
    for (name, bc) in cfg.backends {
        let api_key = resolve_api_key(&name, &bc, &env_lookup)?;
        built.insert(
            name.clone(),
            BedrockBackend::new(name, bc.endpoint, api_key),
        );
    }

    validate_entries("invoke", &cfg.invoke, &built)?;
    validate_entries("embed", &cfg.embed, &built)?;

    let mut model_map = ModelMap::defaults();
    for (k, v) in cfg.invoke {
        model_map.invoke.insert(k, v);
    }
    for (k, v) in cfg.embed {
        model_map.embed.insert(k, v);
    }

    Ok(BedrockBackends::new(built, cfg.default_backend, model_map))
}

fn resolve_api_key(
    name: &str,
    bc: &BackendConfig,
    env_lookup: &impl Fn(&str) -> Option<String>,
) -> Result<Option<String>, BedrockConfigError> {
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
}
