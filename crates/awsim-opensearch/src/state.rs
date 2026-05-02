//! Disk-backed OpenSearch storage.
//!
//! Documents live in a redb table keyed by `(index, doc_id)` so the
//! working set is bounded by disk, not RAM — important for vector
//! workloads where each embedding can be a few KB. Index metadata
//! (mappings, settings) and aliases are small and access-heavy, so
//! they're mirrored in `DashMap` for lock-free reads on the hot path
//! and written through to redb on every change.
//!
//! There is no shutdown snapshot to write or startup snapshot to
//! restore: redb commits each transaction durably.

// `redb::Error` is ~160 bytes; clippy would have us box it. Internal
// storage code only — callers wrap into JSON 500s, never propagate
// the raw `Err` up the stack — so the size doesn't actually matter.
#![allow(clippy::result_large_err)]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile::TempDir;

/// Per-index metadata. Documents are stored separately in the
/// `DOCUMENTS` table and looked up via `(index, doc_id)` keys.
#[derive(Clone, Serialize, Deserialize)]
pub struct IndexMeta {
    pub mappings: Value,
    pub settings: Value,
    pub created_at: String,
    pub uuid: String,
}

/// Tracks `_version`, `_seq_no`, `_primary_term` for a document.
#[derive(Clone, Serialize, Deserialize)]
pub struct DocVersion {
    pub version: u64,
    pub seq_no: u64,
    pub primary_term: u64,
}

/// `(index_name, doc_id)` → document JSON bytes.
const DOCUMENTS: TableDefinition<(&str, &str), &[u8]> = TableDefinition::new("documents");
/// `index_name` → `IndexMeta` JSON bytes. Persisted so we can rebuild
/// the in-memory cache on startup.
const INDEX_META: TableDefinition<&str, &[u8]> = TableDefinition::new("index_meta");
/// `alias_name` → JSON bytes of `Vec<String>` (member indices).
const ALIASES: TableDefinition<&str, &[u8]> = TableDefinition::new("aliases");

/// `(index_name, doc_id)` → `DocVersion` JSON bytes.
const DOC_VERSIONS: TableDefinition<(&str, &str), &[u8]> = TableDefinition::new("doc_versions");

/// Disk-backed OpenSearch state.
pub struct OpenSearchState {
    db: Arc<Database>,
    /// Mirrors `INDEX_META` for hot-path lookups (no disk read on
    /// every search). Always written through to redb.
    pub indices: DashMap<String, IndexMeta>,
    /// Mirrors `ALIASES`.
    pub aliases: DashMap<String, Vec<String>>,
    /// Monotonic counter for `_seq_no`.
    global_seq_no: AtomicU64,
    /// When no `--data-dir` is supplied, awsim runs against an
    /// ephemeral redb file in a tempdir. The handle is held here so
    /// the directory lives as long as the state does.
    _tempdir: Option<TempDir>,
}

impl OpenSearchState {
    /// Open (or create) a redb database at `path` and rebuild the
    /// in-memory caches by scanning `INDEX_META` and `ALIASES`.
    pub fn open(path: &Path) -> Result<Self, redb::Error> {
        let db = Database::create(path)?;
        Self::from_db(db, None)
    }

    /// Open an ephemeral OpenSearch state under a fresh tempdir.
    /// Used when awsim is run without `--data-dir`.
    pub fn ephemeral() -> Result<Self, redb::Error> {
        let tempdir = tempfile::tempdir().map_err(|e| {
            redb::Error::from(redb::StorageError::Io(std::io::Error::other(e.to_string())))
        })?;
        let path = tempdir.path().join("opensearch.redb");
        let db = Database::create(&path)?;
        Self::from_db(db, Some(tempdir))
    }

    fn from_db(db: Database, tempdir: Option<TempDir>) -> Result<Self, redb::Error> {
        // Touch each table once so subsequent read transactions don't
        // fail on an empty database.
        {
            let tx = db.begin_write()?;
            tx.open_table(DOCUMENTS)?;
            tx.open_table(INDEX_META)?;
            tx.open_table(ALIASES)?;
            tx.open_table(DOC_VERSIONS)?;
            tx.commit()?;
        }

        let indices = DashMap::new();
        let aliases = DashMap::new();
        {
            let tx = db.begin_read()?;
            let meta_tbl = tx.open_table(INDEX_META)?;
            for entry in meta_tbl.iter()? {
                let (k, v) = entry?;
                if let Ok(meta) = serde_json::from_slice::<IndexMeta>(v.value()) {
                    indices.insert(k.value().to_string(), meta);
                }
            }
            let alias_tbl = tx.open_table(ALIASES)?;
            for entry in alias_tbl.iter()? {
                let (k, v) = entry?;
                if let Ok(list) = serde_json::from_slice::<Vec<String>>(v.value()) {
                    aliases.insert(k.value().to_string(), list);
                }
            }
        }

        Ok(Self {
            db: Arc::new(db),
            indices,
            aliases,
            global_seq_no: AtomicU64::new(0),
            _tempdir: tempdir,
        })
    }

    // ---- Index meta ----

    pub fn create_index_meta(&self, name: &str, meta: IndexMeta) -> Result<(), redb::Error> {
        let bytes = serde_json::to_vec(&meta).map_err(|_| {
            redb::Error::from(redb::StorageError::Io(std::io::Error::other(
                "IndexMeta serialization failed",
            )))
        })?;
        let tx = self.db.begin_write()?;
        {
            let mut tbl = tx.open_table(INDEX_META)?;
            tbl.insert(name, bytes.as_slice())?;
        }
        tx.commit()?;
        self.indices.insert(name.to_string(), meta);
        Ok(())
    }

    pub fn delete_index_meta(&self, name: &str) -> Result<bool, redb::Error> {
        let tx = self.db.begin_write()?;
        let removed;
        {
            let mut meta_tbl = tx.open_table(INDEX_META)?;
            removed = meta_tbl.remove(name)?.is_some();
            let mut docs = tx.open_table(DOCUMENTS)?;
            let mut ver_tbl = tx.open_table(DOC_VERSIONS)?;
            let mut keys: Vec<String> = Vec::new();
            for entry in docs.range::<(&str, &str)>((name, "")..)? {
                let (k, _) = entry?;
                let (idx, doc_id) = k.value();
                if idx != name {
                    break;
                }
                keys.push(doc_id.to_string());
            }
            for doc_id in &keys {
                docs.remove((name, doc_id.as_str()))?;
                ver_tbl.remove((name, doc_id.as_str()))?;
            }
        }
        tx.commit()?;
        if removed {
            self.indices.remove(name);
            // BUG-16: Remove this index from any aliases that reference it.
            let alias_names: Vec<String> = self.aliases.iter().map(|e| e.key().clone()).collect();
            for alias_name in alias_names {
                if let Some(mut members) = self.aliases.get_mut(&alias_name) {
                    let before = members.len();
                    members.retain(|i| i != name);
                    if members.len() != before {
                        let updated = members.clone();
                        drop(members);
                        let _ = self.put_alias(&alias_name, updated);
                    }
                }
            }
        }
        Ok(removed)
    }

    pub fn index_exists(&self, name: &str) -> bool {
        self.indices.contains_key(name)
    }

    pub fn global_seq_no(&self) -> u64 {
        self.global_seq_no.load(Ordering::Relaxed)
    }

    pub fn get_index_meta(&self, name: &str) -> Option<IndexMeta> {
        self.indices.get(name).map(|e| e.value().clone())
    }

    pub fn list_indices(&self) -> Vec<(String, IndexMeta)> {
        self.indices
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect()
    }

    // ---- Documents ----

    /// Insert or update a document. Returns `(created, new_version)`.
    pub fn put_doc(
        &self,
        index: &str,
        doc_id: &str,
        doc: &Value,
    ) -> Result<(bool, u64), redb::Error> {
        let bytes = serde_json::to_vec(doc).map_err(|_| {
            redb::Error::from(redb::StorageError::Io(std::io::Error::other(
                "doc serialization failed",
            )))
        })?;
        let seq = self.global_seq_no.fetch_add(1, Ordering::Relaxed) + 1;
        let ver_bytes = serde_json::to_vec(&DocVersion {
            version: seq,
            seq_no: seq,
            primary_term: 1,
        })
        .map_err(|_| {
            redb::Error::from(redb::StorageError::Io(std::io::Error::other(
                "version serialization failed",
            )))
        })?;
        let tx = self.db.begin_write()?;
        let created;
        {
            let mut tbl = tx.open_table(DOCUMENTS)?;
            let mut ver_tbl = tx.open_table(DOC_VERSIONS)?;
            created = tbl.insert((index, doc_id), bytes.as_slice())?.is_none();
            ver_tbl.insert((index, doc_id), ver_bytes.as_slice())?;
        }
        tx.commit()?;
        Ok((created, seq))
    }

    pub fn get_doc(&self, index: &str, doc_id: &str) -> Result<Option<Value>, redb::Error> {
        let tx = self.db.begin_read()?;
        let tbl = tx.open_table(DOCUMENTS)?;
        let Some(v) = tbl.get((index, doc_id))? else {
            return Ok(None);
        };
        let val: Value = serde_json::from_slice(v.value()).unwrap_or(Value::Null);
        Ok(Some(val))
    }

    pub fn get_doc_version(
        &self,
        index: &str,
        doc_id: &str,
    ) -> Result<Option<DocVersion>, redb::Error> {
        let tx = self.db.begin_read()?;
        let tbl = tx.open_table(DOC_VERSIONS)?;
        let Some(v) = tbl.get((index, doc_id))? else {
            return Ok(None);
        };
        let ver: DocVersion = serde_json::from_slice(v.value()).unwrap_or(DocVersion {
            version: 1,
            seq_no: 0,
            primary_term: 1,
        });
        Ok(Some(ver))
    }

    pub fn delete_doc(&self, index: &str, doc_id: &str) -> Result<bool, redb::Error> {
        let tx = self.db.begin_write()?;
        let removed;
        {
            let mut tbl = tx.open_table(DOCUMENTS)?;
            let mut ver_tbl = tx.open_table(DOC_VERSIONS)?;
            removed = tbl.remove((index, doc_id))?.is_some();
            ver_tbl.remove((index, doc_id))?;
        }
        tx.commit()?;
        Ok(removed)
    }

    /// Stream every document in `index` to `f`. Returning `false`
    /// from the callback stops iteration early.
    ///
    /// Reads happen inside a single read transaction so callers see a
    /// consistent snapshot even if writes happen concurrently.
    pub fn for_each_doc<F: FnMut(&str, &Value) -> bool>(
        &self,
        index: &str,
        mut f: F,
    ) -> Result<(), redb::Error> {
        let tx = self.db.begin_read()?;
        let tbl = tx.open_table(DOCUMENTS)?;
        for entry in tbl.range::<(&str, &str)>((index, "")..)? {
            let (k, v) = entry?;
            let (idx, doc_id) = k.value();
            // The range with `(index, "")..` keeps walking past `index`
            // lex-wise, so bail when the prefix changes.
            if idx != index {
                break;
            }
            let doc: Value = match serde_json::from_slice(v.value()) {
                Ok(d) => d,
                Err(_) => continue,
            };
            if !f(doc_id, &doc) {
                break;
            }
        }
        Ok(())
    }

    pub fn count_docs(&self, index: &str) -> Result<usize, redb::Error> {
        let mut n = 0usize;
        self.for_each_doc(index, |_, _| {
            n += 1;
            true
        })?;
        Ok(n)
    }

    // ---- Aliases ----

    pub fn put_alias(&self, name: &str, members: Vec<String>) -> Result<(), redb::Error> {
        let bytes = serde_json::to_vec(&members).map_err(|_| {
            redb::Error::from(redb::StorageError::Io(std::io::Error::other(
                "alias serialization failed",
            )))
        })?;
        let tx = self.db.begin_write()?;
        {
            let mut tbl = tx.open_table(ALIASES)?;
            tbl.insert(name, bytes.as_slice())?;
        }
        tx.commit()?;
        self.aliases.insert(name.to_string(), members);
        Ok(())
    }

    pub fn delete_alias(&self, name: &str) -> Result<(), redb::Error> {
        let tx = self.db.begin_write()?;
        {
            let mut tbl = tx.open_table(ALIASES)?;
            tbl.remove(name)?;
        }
        tx.commit()?;
        self.aliases.remove(name);
        Ok(())
    }

    /// Resolve a name to the actual indices to read. If `name` matches
    /// an alias, returns its member indices; otherwise returns the
    /// name as a single-element list.
    pub fn resolve_alias(&self, name: &str) -> Vec<String> {
        match self.aliases.get(name) {
            Some(v) => v.clone(),
            None => vec![name.to_string()],
        }
    }
}
