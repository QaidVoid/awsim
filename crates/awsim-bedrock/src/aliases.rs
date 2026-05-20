//! Alias groups: bedrock_id → ordered list of (backend, tag)
//! targets. The resolver picks the first target whose backend
//! exists, so aliases double as "static fallback" for a missing
//! backend even without runtime error fallback (which lands in
//! Phase 4).
//!
//! Aliases are a strict superset of the legacy `[invoke]` / `[embed]`
//! single-target mappings — a single-target alias is equivalent to a
//! `Routed { backend, tag }` entry. The runtime checks aliases first
//! and falls through to the legacy tables, so existing TOMLs keep
//! working unchanged while new mappings get the multi-target shape.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// One alias entry. Keyed externally by bedrock_id in the
/// `BedrockSpec.aliases` map.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AliasSpec {
    /// Which side of the runtime this alias serves. "chat" matches
    /// InvokeModel / Converse paths; "embed" matches the embedding
    /// model invocations.
    #[serde(default)]
    pub kind: AliasKind,
    /// Selection strategy across `targets`. Phase 3 ships `First`:
    /// pick the first target whose backend resolves, skipping
    /// targets whose backend is missing from the registry. Future
    /// phases will add round-robin, weighted, and least-latency.
    #[serde(default)]
    pub strategy: AliasStrategy,
    /// Ordered list of (backend, tag) candidates. Phase 3 walks
    /// these in declaration order under the `First` strategy.
    pub targets: Vec<AliasTarget>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AliasKind {
    #[default]
    Chat,
    Embed,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AliasStrategy {
    /// Walk `targets` in declaration order, return the first whose
    /// backend exists.
    #[default]
    First,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct AliasTarget {
    /// Name of an entry in `[backends.<name>]`.
    pub backend: String,
    /// Backend-side model tag passed in the upstream chat /
    /// embeddings request.
    pub tag: String,
}

/// JSON view of one alias for the admin endpoint. Sorted target
/// ordering preserves the declared priority.
pub(crate) fn alias_view(id: &str, alias: &AliasSpec) -> Value {
    let targets: Vec<Value> = alias
        .targets
        .iter()
        .map(|t| json!({ "backend": t.backend, "tag": t.tag }))
        .collect();
    json!({
        "id": id,
        "kind": match alias.kind { AliasKind::Chat => "chat", AliasKind::Embed => "embed" },
        "strategy": match alias.strategy { AliasStrategy::First => "first" },
        "targets": targets,
    })
}
