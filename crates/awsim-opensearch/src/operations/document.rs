use serde_json::{Value, json};

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
pub fn get_document(state: &OpenSearchState, index_name: &str, doc_id: &str) -> (u16, Value) {
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
            );
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

/// Update a document by ID (partial update with `doc` field, supports `doc_as_upsert`).
pub fn update_document(
    state: &OpenSearchState,
    index_name: &str,
    doc_id: &str,
    body: &Value,
) -> (u16, Value) {
    let partial = body.get("doc").cloned().unwrap_or(serde_json::json!({}));
    let doc_as_upsert = body["doc_as_upsert"].as_bool().unwrap_or(false);

    // Auto-create index if it doesn't exist
    if !state.indices.contains_key(index_name) {
        state.indices.insert(
            index_name.to_string(),
            crate::state::OpenSearchIndex {
                name: index_name.to_string(),
                mappings: serde_json::json!({}),
                settings: serde_json::json!({}),
                documents: Default::default(),
                created_at: crate::util::now_iso8601(),
            },
        );
    }

    let mut idx = state.indices.get_mut(index_name).unwrap();

    if let Some(existing) = idx.documents.get_mut(doc_id) {
        // Merge partial fields into existing document
        if let (Some(existing_obj), Some(partial_obj)) =
            (existing.as_object_mut(), partial.as_object())
        {
            for (k, v) in partial_obj {
                existing_obj.insert(k.clone(), v.clone());
            }
        }
        (
            200,
            serde_json::json!({
                "_index": index_name,
                "_id": doc_id,
                "_version": 1,
                "result": "updated",
                "_shards": { "total": 2, "successful": 1, "failed": 0 },
                "_seq_no": 0,
                "_primary_term": 1,
            }),
        )
    } else if doc_as_upsert {
        // Create the document from partial
        idx.documents.insert(doc_id.to_string(), partial);
        (
            201,
            serde_json::json!({
                "_index": index_name,
                "_id": doc_id,
                "_version": 1,
                "result": "created",
                "_shards": { "total": 2, "successful": 1, "failed": 0 },
                "_seq_no": 0,
                "_primary_term": 1,
            }),
        )
    } else {
        (
            404,
            serde_json::json!({
                "_index": index_name,
                "_id": doc_id,
                "found": false,
            }),
        )
    }
}

/// Update documents matching a query using a simple script.
///
/// Supported script patterns:
/// - `ctx._source.remove('fieldName')` — remove a field
/// - `ctx._source.fieldName = params.value` — set a field from params
pub fn update_by_query(state: &OpenSearchState, index_name: &str, body: &Value) -> (u16, Value) {
    let query = body
        .get("query")
        .cloned()
        .unwrap_or(serde_json::json!({"match_all": {}}));
    let script_source = body
        .pointer("/script/source")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let params = body
        .pointer("/script/params")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    // Resolve alias if needed
    let resolved: Vec<String> = if let Some(aliased) = state.aliases.get(index_name) {
        aliased.clone()
    } else {
        vec![index_name.to_string()]
    };

    let mut updated: usize = 0;

    for name in &resolved {
        if let Some(mut idx) = state.indices.get_mut(name) {
            // Collect IDs of matching documents first (borrow checker)
            let matching_ids: Vec<String> = idx
                .documents
                .iter()
                .filter(|(_, doc)| super::search::match_score(&query, doc) > 0.0)
                .map(|(id, _)| id.clone())
                .collect();

            for id in matching_ids {
                if let Some(doc) = idx.documents.get_mut(&id) {
                    apply_script(doc, &script_source, &params);
                    updated += 1;
                }
            }
        }
    }

    (
        200,
        serde_json::json!({
            "updated": updated,
            "failures": [],
            "_shards": { "total": 1, "successful": 1, "failed": 0 },
        }),
    )
}

/// Apply a simple Painless-style script to a document source.
///
/// Supported patterns (one per semicolon-separated statement):
/// - `ctx._source.remove('fieldName')` / `ctx._source.remove("fieldName")`
/// - `ctx._source.fieldName = params.paramName`
/// - `ctx._source.fieldName = 'literal'` / `ctx._source.fieldName = "literal"`
fn apply_script(doc: &mut serde_json::Value, source: &str, params: &serde_json::Value) {
    for stmt in source.split(';') {
        let stmt = stmt.trim();
        if stmt.is_empty() {
            continue;
        }

        // ctx._source.remove('field') or ctx._source.remove("field")
        if let Some(rest) = stmt.strip_prefix("ctx._source.remove(") {
            let rest = rest.trim_end_matches(')');
            let field = rest.trim().trim_matches('\'').trim_matches('"');
            if let Some(obj) = doc.as_object_mut() {
                obj.remove(field);
            }
            continue;
        }

        // ctx._source.field = ...
        if let Some(rest) = stmt.strip_prefix("ctx._source.") {
            if let Some(eq_pos) = rest.find('=') {
                let field = rest[..eq_pos].trim().to_string();
                let rhs = rest[eq_pos + 1..].trim();

                // params.paramName
                if let Some(param_path) = rhs.strip_prefix("params.") {
                    let param_name = param_path.trim();
                    if let Some(val) = params.get(param_name) {
                        if let Some(obj) = doc.as_object_mut() {
                            obj.insert(field, val.clone());
                        }
                    }
                    continue;
                }

                // String literal 'value' or "value"
                if (rhs.starts_with('\'') && rhs.ends_with('\''))
                    || (rhs.starts_with('"') && rhs.ends_with('"'))
                {
                    let literal = &rhs[1..rhs.len() - 1];
                    if let Some(obj) = doc.as_object_mut() {
                        obj.insert(field, serde_json::json!(literal));
                    }
                    continue;
                }

                // Numeric literal
                if let Ok(n) = rhs.parse::<i64>() {
                    if let Some(obj) = doc.as_object_mut() {
                        obj.insert(field, serde_json::json!(n));
                    }
                    continue;
                }
                if let Ok(f) = rhs.parse::<f64>() {
                    if let Some(obj) = doc.as_object_mut() {
                        obj.insert(field, serde_json::json!(f));
                    }
                }
            }
        }
    }
}

/// Delete a document by ID.
pub fn delete_document(state: &OpenSearchState, index_name: &str, doc_id: &str) -> (u16, Value) {
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
            );
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
