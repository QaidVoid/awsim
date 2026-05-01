use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Global OpenSearch state (not per-account — OpenSearch domains are accessed directly).
#[derive(Default)]
pub struct OpenSearchState {
    /// Index name → Index data
    pub indices: DashMap<String, OpenSearchIndex>,
    /// Alias name → list of index names the alias points to
    pub aliases: DashMap<String, Vec<String>>,
}

/// An OpenSearch/Elasticsearch index.
#[derive(Clone, Serialize, Deserialize)]
pub struct OpenSearchIndex {
    pub name: String,
    pub mappings: Value,
    pub settings: Value,
    /// Document ID → document source
    pub documents: HashMap<String, Value>,
    pub created_at: String,
}

/// Serializable snapshot — drained on shutdown, restored on startup.
/// Reads / writes still hit the in-memory DashMap; persistence is
/// only on the load/save boundary so the hot path stays fast.
#[derive(Serialize, Deserialize, Default)]
pub struct OpenSearchSnapshot {
    pub indices: Vec<OpenSearchIndex>,
    pub aliases: Vec<(String, Vec<String>)>,
}

impl OpenSearchState {
    /// Materialise the in-memory state into the snapshot wire format.
    pub fn snapshot(&self) -> Vec<u8> {
        let snap = OpenSearchSnapshot {
            indices: self.indices.iter().map(|e| e.value().clone()).collect(),
            aliases: self
                .aliases
                .iter()
                .map(|e| (e.key().clone(), e.value().clone()))
                .collect(),
        };
        serde_json::to_vec(&snap).unwrap_or_default()
    }

    /// Replace the in-memory state with the contents of a previously
    /// saved snapshot. Failures here are recoverable (we just return
    /// the parse error and start with an empty store).
    pub fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: OpenSearchSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        self.indices.clear();
        for idx in snap.indices {
            self.indices.insert(idx.name.clone(), idx);
        }
        self.aliases.clear();
        for (k, v) in snap.aliases {
            self.aliases.insert(k, v);
        }
        Ok(())
    }
}
