use std::path::PathBuf;

use tracing::{error, info};

/// Manages JSON snapshot-based persistence for service state.
///
/// When a `data_dir` is configured, each service can serialize its state to a JSON
/// file under `{data_dir}/snapshots/{service}.json`.  On startup those snapshots are
/// loaded and passed back to each service so it can rebuild its in-memory state.
pub struct PersistenceManager {
    data_dir: PathBuf,
}

impl PersistenceManager {
    /// Create a new `PersistenceManager` rooted at `data_dir`.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    /// Save a service's state snapshot to `{data_dir}/snapshots/{service_name}.json`.
    ///
    /// Uses atomic write (write to temp file, then rename) to prevent corruption
    /// if the process is killed mid-write.
    pub fn save_snapshot(&self, service_name: &str, data: &[u8]) -> std::io::Result<()> {
        let dir = self.data_dir.join("snapshots");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{service_name}.json"));
        let tmp_path = dir.join(format!("{service_name}.json.tmp"));
        // Write to temp file first
        std::fs::write(&tmp_path, data)?;
        // Atomic rename — either the old file remains or the new one replaces it
        std::fs::rename(&tmp_path, &path)?;
        info!(service = service_name, path = %path.display(), "Saved snapshot");
        Ok(())
    }

    /// Load a service's state snapshot from disk.  Returns `None` if no snapshot exists.
    pub fn load_snapshot(&self, service_name: &str) -> Option<Vec<u8>> {
        let path = self
            .data_dir
            .join("snapshots")
            .join(format!("{service_name}.json"));
        match std::fs::read(&path) {
            Ok(data) => {
                info!(service = service_name, path = %path.display(), "Loaded snapshot");
                Some(data)
            }
            Err(_) => None,
        }
    }

    /// List the names of all saved snapshots (without the `.json` suffix).
    pub fn list_snapshots(&self) -> Vec<String> {
        let dir = self.data_dir.join("snapshots");
        std::fs::read_dir(&dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        name.strip_suffix(".json").map(|s| s.to_string())
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Save snapshots for all services that support it.
    pub fn save_all(
        &self,
        services: &std::collections::HashMap<String, std::sync::Arc<dyn crate::ServiceHandler>>,
    ) {
        for (name, handler) in services {
            if let Some(data) = handler.snapshot()
                && let Err(e) = self.save_snapshot(name, &data)
            {
                error!(service = %name, error = %e, "Failed to save snapshot");
            }
        }
    }

    /// Restore snapshots for all services that support it.
    ///
    /// Runs in two passes: every service is `restore`d first, then
    /// every service's `rehydrate` is invoked. The split lets a
    /// service that reads peer state at rehydration time (Lambda
    /// event-source-mapping consulting the restored SQS queues,
    /// Pipes restarting STARTING workers, ACM re-arming the 72h
    /// validation timer, etc.) see a fully-restored neighbour.
    pub fn restore_all(
        &self,
        services: &std::collections::HashMap<String, std::sync::Arc<dyn crate::ServiceHandler>>,
    ) {
        for (name, handler) in services {
            if let Some(data) = self.load_snapshot(name)
                && let Err(e) = handler.restore(&data)
            {
                tracing::warn!(service = %name, error = %e, "Failed to restore snapshot");
            }
        }
        for (name, handler) in services {
            if let Err(e) = handler.rehydrate() {
                tracing::warn!(service = %name, error = %e, "Rehydrate hook failed");
            }
        }
    }
}
