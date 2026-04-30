//! Hot-reloadable runtime configuration.
//!
//! Settings that should be editable from the admin UI without a
//! restart live here. The store wraps the live config in an
//! [`ArcSwap`] so request-path readers don't take a lock; the
//! `apply` path validates, swaps, persists, and runs registered
//! reload hooks for services that need to rebuild internal state
//! (e.g. Bedrock backends).
//!
//! Persistence is gated on `--data-dir`. Without one, the config is
//! in-memory only and resets on each run; CLI flags seed defaults.
//! With one, the file at `<data_dir>/runtime-config.json` is the
//! source of truth on subsequent runs (CLI flags only seed initial
//! values when the file doesn't exist yet).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

use arc_swap::ArcSwap;
use awsim_bedrock::BedrockSpec;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default SES outbox retention when nothing else is set. Mirrors
/// the `--ses-retention-hours` CLI default so the runtime config can
/// reset to the same baseline as a clean install.
pub const DEFAULT_SES_RETENTION_HOURS: u64 = 720;

/// On-disk config filename. Stored under `<data_dir>/`.
pub const CONFIG_FILENAME: &str = "runtime-config.json";

/// Top-level runtime config. Sections grow as more settings become
/// editable. Anything not in here is pure CLI-flag territory and
/// requires a restart to change.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub bedrock: BedrockSection,
    #[serde(default)]
    pub ses: SesSection,
}

/// Bedrock proxy config. `enabled = false` puts the proxy into
/// canned-response mode regardless of what's in `spec`. Letting
/// users persist a full backend config and still flip the kill
/// switch from the UI is more useful than forcing them to clear
/// the spec on every disable.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct BedrockSection {
    /// Master switch. When false, the runtime serves canned
    /// responses even if `spec` has backends defined.
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub spec: BedrockSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SesSection {
    /// Hours to retain captured outbound emails. `0` disables the
    /// sweep entirely so emails accumulate indefinitely.
    pub retention_hours: u64,
}

impl Default for SesSection {
    fn default() -> Self {
        Self {
            retention_hours: DEFAULT_SES_RETENTION_HOURS,
        }
    }
}

#[derive(Debug, Error)]
pub enum RuntimeConfigError {
    #[error("reading runtime config {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("parsing runtime config {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error(transparent)]
    Bedrock(#[from] awsim_bedrock::BedrockConfigError),
}

/// A hook called after a successful config swap. Receives the new
/// config so it can rebuild internal state (e.g. Bedrock backends).
/// Errors are logged but don't roll back the swap — by the time
/// hooks run, the config has already been validated and persisted.
pub type ReloadHook = Box<dyn Fn(&RuntimeConfig) + Send + Sync>;

pub struct RuntimeConfigStore {
    inner: ArcSwap<RuntimeConfig>,
    path: Option<PathBuf>,
    hooks: Mutex<Vec<ReloadHook>>,
}

impl RuntimeConfigStore {
    /// Build the store. When `path` is set and exists, its contents
    /// override `seed`. When `path` is set but missing, `seed` is
    /// written as the initial file. When `path` is None, we run in
    /// memory only.
    pub fn load_or_seed(
        seed: RuntimeConfig,
        path: Option<PathBuf>,
    ) -> Result<Self, RuntimeConfigError> {
        let initial = match path.as_deref() {
            Some(p) if p.exists() => read_from_disk(p)?,
            Some(p) => {
                write_to_disk(p, &seed)?;
                seed
            }
            None => seed,
        };
        Ok(Self {
            inner: ArcSwap::from_pointee(initial),
            path,
            hooks: Mutex::new(Vec::new()),
        })
    }

    /// Snapshot of the live config. Cheap — the inner `Arc` is
    /// reference-counted, so reads don't allocate.
    pub fn current(&self) -> Arc<RuntimeConfig> {
        self.inner.load_full()
    }

    /// Whether this store is disk-backed.
    pub fn is_persistent(&self) -> bool {
        self.path.is_some()
    }

    /// Path the config persists to, if any. Used by the admin
    /// endpoint to indicate whether changes survive a restart.
    pub fn config_path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Register a hook that fires after every successful config
    /// swap. Hooks see the new config; they're synchronous so
    /// don't do heavy work — spawn a task if needed.
    #[allow(dead_code)] // wired up in slice 2 (Bedrock hot-reload)
    pub fn on_change(&self, hook: ReloadHook) {
        self.hooks
            .lock()
            .expect("runtime config hook lock poisoned")
            .push(hook);
    }

    /// Validate, persist, swap, and notify. Returns the new live
    /// config. Validation failures don't touch disk or in-memory
    /// state, so a bad PUT leaves the running system untouched.
    pub fn apply(&self, next: RuntimeConfig) -> Result<Arc<RuntimeConfig>, RuntimeConfigError> {
        validate(&next)?;
        if let Some(p) = self.path.as_deref() {
            write_to_disk(p, &next)?;
        }
        let arc = Arc::new(next);
        self.inner.store(Arc::clone(&arc));

        let hooks = self
            .hooks
            .lock()
            .expect("runtime config hook lock poisoned");
        for hook in hooks.iter() {
            hook(&arc);
        }
        Ok(arc)
    }
}

fn read_from_disk(path: &Path) -> Result<RuntimeConfig, RuntimeConfigError> {
    let raw = std::fs::read_to_string(path).map_err(|e| RuntimeConfigError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    serde_json::from_str(&raw).map_err(|e| RuntimeConfigError::Parse {
        path: path.display().to_string(),
        source: e,
    })
}

fn write_to_disk(path: &Path, cfg: &RuntimeConfig) -> Result<(), RuntimeConfigError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| RuntimeConfigError::Io {
            path: parent.display().to_string(),
            source: e,
        })?;
    }
    let body = serde_json::to_vec_pretty(cfg).expect("RuntimeConfig serialization is infallible");
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, &body) {
        return Err(RuntimeConfigError::Io {
            path: tmp.display().to_string(),
            source: e,
        });
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        // Don't leak the tmp file on rename failure.
        let _ = std::fs::remove_file(&tmp);
        return Err(RuntimeConfigError::Io {
            path: path.display().to_string(),
            source: e,
        });
    }
    Ok(())
}

fn validate(cfg: &RuntimeConfig) -> Result<(), RuntimeConfigError> {
    // Bedrock spec validation only runs when the user has actually
    // declared backends and flipped the switch on. Empty spec +
    // disabled is a valid state — it means canned responses.
    if cfg.bedrock.enabled && !cfg.bedrock.spec.backends.is_empty() {
        // Re-validate using the bedrock loader. This catches missing
        // backends, env-var mismatches, etc. before we swap.
        let spec_clone = cfg.bedrock.spec.clone();
        awsim_bedrock::build_from_spec(spec_clone, |v| std::env::var(v).ok())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use awsim_bedrock::BackendSpec;
    use std::collections::HashMap;

    fn temp_path() -> PathBuf {
        let dir = std::env::temp_dir();
        dir.join(format!(
            "awsim-runtime-config-{}.json",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn seed_writes_file_on_first_run() {
        let path = temp_path();
        assert!(!path.exists());
        let store = RuntimeConfigStore::load_or_seed(
            RuntimeConfig {
                ses: SesSection {
                    retention_hours: 12,
                },
                ..Default::default()
            },
            Some(path.clone()),
        )
        .unwrap();
        assert!(path.exists());
        assert_eq!(store.current().ses.retention_hours, 12);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn existing_file_overrides_seed() {
        let path = temp_path();
        write_to_disk(
            &path,
            &RuntimeConfig {
                ses: SesSection {
                    retention_hours: 99,
                },
                ..Default::default()
            },
        )
        .unwrap();
        let store = RuntimeConfigStore::load_or_seed(
            RuntimeConfig {
                ses: SesSection {
                    retention_hours: 12,
                },
                ..Default::default()
            },
            Some(path.clone()),
        )
        .unwrap();
        assert_eq!(store.current().ses.retention_hours, 99);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn apply_swaps_and_persists() {
        let path = temp_path();
        let store =
            RuntimeConfigStore::load_or_seed(RuntimeConfig::default(), Some(path.clone())).unwrap();

        store
            .apply(RuntimeConfig {
                ses: SesSection { retention_hours: 5 },
                ..Default::default()
            })
            .unwrap();
        assert_eq!(store.current().ses.retention_hours, 5);

        let on_disk = read_from_disk(&path).unwrap();
        assert_eq!(on_disk.ses.retention_hours, 5);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn apply_runs_hooks_with_new_config() {
        let store = RuntimeConfigStore::load_or_seed(RuntimeConfig::default(), None).unwrap();
        let saw = Arc::new(Mutex::new(0u64));
        let saw_c = Arc::clone(&saw);
        store.on_change(Box::new(move |cfg| {
            *saw_c.lock().unwrap() = cfg.ses.retention_hours;
        }));

        store
            .apply(RuntimeConfig {
                ses: SesSection {
                    retention_hours: 42,
                },
                ..Default::default()
            })
            .unwrap();
        assert_eq!(*saw.lock().unwrap(), 42);
    }

    #[test]
    fn invalid_bedrock_spec_does_not_swap() {
        let store = RuntimeConfigStore::load_or_seed(RuntimeConfig::default(), None).unwrap();

        // Default backend names a missing block — should fail
        // validation before any swap happens.
        let mut backends = HashMap::new();
        backends.insert(
            "ollama".to_string(),
            BackendSpec {
                endpoint: "http://localhost".into(),
                api_key: None,
                api_key_env: None,
            },
        );
        let bad = RuntimeConfig {
            bedrock: BedrockSection {
                enabled: true,
                spec: BedrockSpec {
                    default_backend: Some("ghost".into()),
                    backends,
                    ..Default::default()
                },
            },
            ..Default::default()
        };
        let err = store.apply(bad).unwrap_err();
        assert!(matches!(err, RuntimeConfigError::Bedrock(_)));
        // Live config unchanged.
        assert!(!store.current().bedrock.enabled);
    }
}
