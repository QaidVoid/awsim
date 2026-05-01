use serde_json::{Value, json};

use crate::state::{IndexMeta, OpenSearchState};

/// Create an index.
pub fn create_index(state: &OpenSearchState, index_name: &str, body: &Value) -> (u16, Value) {
    if state.index_exists(index_name) {
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

    if let Err(e) = state.create_index_meta(
        index_name,
        IndexMeta {
            mappings,
            settings,
            created_at: crate::util::now_iso8601(),
        },
    ) {
        return (500, storage_error(&e));
    }

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
    match state.delete_index_meta(index_name) {
        Ok(true) => (200, json!({ "acknowledged": true })),
        Ok(false) => (404, index_not_found(index_name)),
        Err(e) => (500, storage_error(&e)),
    }
}

/// Get index mapping.
pub fn get_mapping(state: &OpenSearchState, index_name: &str) -> (u16, Value) {
    match state.get_index_meta(index_name) {
        Some(meta) => (200, json!({ index_name: { "mappings": meta.mappings } })),
        None => (404, index_not_found(index_name)),
    }
}

/// Get index settings.
pub fn get_index(state: &OpenSearchState, index_name: &str) -> (u16, Value) {
    match state.get_index_meta(index_name) {
        Some(meta) => (
            200,
            json!({
                index_name: {
                    "aliases": {},
                    "mappings": meta.mappings,
                    "settings": {
                        "index": {
                            "creation_date": meta.created_at,
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
    if state.index_exists(index_name) {
        200
    } else {
        404
    }
}

/// List all indices (cat/indices).
pub fn cat_indices(state: &OpenSearchState) -> (u16, Value) {
    let indices: Vec<Value> = state
        .list_indices()
        .into_iter()
        .map(|(name, _meta)| {
            let count = state.count_docs(&name).unwrap_or(0);
            json!({
                "index": name,
                "health": "green",
                "status": "open",
                "pri": "1",
                "rep": "1",
                "docs.count": count.to_string(),
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

pub(crate) fn storage_error(e: &impl std::fmt::Display) -> Value {
    json!({
        "error": {
            "type": "internal_server_error",
            "reason": e.to_string(),
        },
        "status": 500,
    })
}
