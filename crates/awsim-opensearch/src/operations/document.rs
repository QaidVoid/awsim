use serde_json::{json, Value};

use crate::state::OpenSearchState;

/// Index (PUT/POST) a document.
pub fn index_document(
    state: &OpenSearchState,
    index_name: &str,
    doc_id: Option<&str>,
    body: &Value,
) -> (u16, Value) {
    // Auto-create index if it doesn't exist
    if !state.indices.contains_key(index_name) {
        state.indices.insert(
            index_name.to_string(),
            crate::state::OpenSearchIndex {
                name: index_name.to_string(),
                mappings: json!({}),
                settings: json!({}),
                documents: Default::default(),
                created_at: crate::util::now_iso8601(),
            },
        );
    }

    let mut idx = state.indices.get_mut(index_name).unwrap();
    let id = doc_id
        .map(String::from)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let created = !idx.documents.contains_key(&id);
    idx.documents.insert(id.clone(), body.clone());

    let status = if created { 201 } else { 200 };

    (
        status,
        json!({
            "_index": index_name,
            "_id": id,
            "_version": 1,
            "result": if created { "created" } else { "updated" },
            "_shards": { "total": 2, "successful": 1, "failed": 0 },
            "_seq_no": 0,
            "_primary_term": 1,
        }),
    )
}

/// Get a document by ID.
pub fn get_document(
    state: &OpenSearchState,
    index_name: &str,
    doc_id: &str,
) -> (u16, Value) {
    let idx = match state.indices.get(index_name) {
        Some(idx) => idx,
        None => {
            return (
                404,
                json!({
                    "_index": index_name,
                    "_id": doc_id,
                    "found": false,
                }),
            )
        }
    };

    match idx.documents.get(doc_id) {
        Some(doc) => (
            200,
            json!({
                "_index": index_name,
                "_id": doc_id,
                "_version": 1,
                "_seq_no": 0,
                "_primary_term": 1,
                "found": true,
                "_source": doc,
            }),
        ),
        None => (
            404,
            json!({
                "_index": index_name,
                "_id": doc_id,
                "found": false,
            }),
        ),
    }
}

/// Delete a document by ID.
pub fn delete_document(
    state: &OpenSearchState,
    index_name: &str,
    doc_id: &str,
) -> (u16, Value) {
    let mut idx = match state.indices.get_mut(index_name) {
        Some(idx) => idx,
        None => {
            return (
                404,
                json!({
                    "_index": index_name,
                    "_id": doc_id,
                    "result": "not_found",
                }),
            )
        }
    };

    let found = idx.documents.remove(doc_id).is_some();

    (
        if found { 200 } else { 404 },
        json!({
            "_index": index_name,
            "_id": doc_id,
            "_version": 1,
            "result": if found { "deleted" } else { "not_found" },
            "_shards": { "total": 2, "successful": 1, "failed": 0 },
        }),
    )
}
