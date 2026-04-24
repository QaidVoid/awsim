use serde_json::{Value, json};
use std::collections::HashMap;

use crate::state::{OpenSearchIndex, OpenSearchState};

/// Create an index.
pub fn create_index(state: &OpenSearchState, index_name: &str, body: &Value) -> (u16, Value) {
    if state.indices.contains_key(index_name) {
        return (
            400,
            json!({
                "error": {
                    "root_cause": [{"type": "resource_already_exists_exception", "reason": format!("index [{}] already exists", index_name)}],
                    "type": "resource_already_exists_exception",
                    "reason": format!("index [{}] already exists", index_name),
                },
                "status": 400,
            }),
        );
    }

    let mappings = body.get("mappings").cloned().unwrap_or(json!({}));
    let settings = body.get("settings").cloned().unwrap_or(json!({}));

    state.indices.insert(
        index_name.to_string(),
        OpenSearchIndex {
            name: index_name.to_string(),
            mappings,
            settings,
            documents: HashMap::new(),
            created_at: crate::util::now_iso8601(),
        },
    );

    (
        200,
        json!({
            "acknowledged": true,
            "shards_acknowledged": true,
            "index": index_name,
        }),
    )
}

/// Delete an index.
pub fn delete_index(state: &OpenSearchState, index_name: &str) -> (u16, Value) {
    if state.indices.remove(index_name).is_some() {
        (200, json!({ "acknowledged": true }))
    } else {
        (
            404,
            json!({
                "error": {
                    "root_cause": [{"type": "index_not_found_exception", "reason": format!("no such index [{}]", index_name)}],
                    "type": "index_not_found_exception",
                    "reason": format!("no such index [{}]", index_name),
                },
                "status": 404,
            }),
        )
    }
}

/// Get index mapping.
pub fn get_mapping(state: &OpenSearchState, index_name: &str) -> (u16, Value) {
    match state.indices.get(index_name) {
        Some(idx) => (200, json!({ index_name: { "mappings": idx.mappings } })),
        None => (404, index_not_found(index_name)),
    }
}

/// Get index settings.
pub fn get_index(state: &OpenSearchState, index_name: &str) -> (u16, Value) {
    match state.indices.get(index_name) {
        Some(idx) => (
            200,
            json!({
                index_name: {
                    "aliases": {},
                    "mappings": idx.mappings,
                    "settings": {
                        "index": {
                            "creation_date": idx.created_at,
                            "number_of_shards": "1",
                            "number_of_replicas": "1",
                            "uuid": uuid::Uuid::new_v4().to_string(),
                        }
                    }
                }
            }),
        ),
        None => (404, index_not_found(index_name)),
    }
}

/// Check if index exists (HEAD request).
pub fn index_exists(state: &OpenSearchState, index_name: &str) -> u16 {
    if state.indices.contains_key(index_name) {
        200
    } else {
        404
    }
}

/// List all indices (cat/indices).
pub fn cat_indices(state: &OpenSearchState) -> (u16, Value) {
    let indices: Vec<Value> = state
        .indices
        .iter()
        .map(|entry| {
            let idx = entry.value();
            json!({
                "index": idx.name,
                "health": "green",
                "status": "open",
                "pri": "1",
                "rep": "1",
                "docs.count": idx.documents.len().to_string(),
                "store.size": "1kb",
            })
        })
        .collect();

    (200, json!(indices))
}

fn index_not_found(name: &str) -> Value {
    json!({
        "error": {
            "root_cause": [{"type": "index_not_found_exception", "reason": format!("no such index [{}]", name)}],
            "type": "index_not_found_exception",
            "reason": format!("no such index [{}]", name),
        },
        "status": 404,
    })
}
