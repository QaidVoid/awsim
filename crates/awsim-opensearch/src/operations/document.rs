use serde_json::{Value, json};

use super::index::storage_error;
use crate::state::{IndexMeta, OpenSearchState};

/// Auto-create an empty index if it doesn't already exist. Used by
/// `index_document` and `update_document` to mirror Elasticsearch's
/// permissive write-creates-index behaviour.
fn ensure_index(state: &OpenSearchState, index_name: &str) -> Result<(), Value> {
    if state.index_exists(index_name) {
        return Ok(());
    }
    state
        .create_index_meta(
            index_name,
            IndexMeta {
                mappings: json!({}),
                settings: json!({}),
                created_at: crate::util::now_iso8601(),
            },
        )
        .map_err(|e| storage_error(&e))
}

/// Index (PUT/POST) a document.
pub fn index_document(
    state: &OpenSearchState,
    index_name: &str,
    doc_id: Option<&str>,
    body: &Value,
) -> (u16, Value) {
    if let Err(err) = ensure_index(state, index_name) {
        return (500, err);
    }

    let id = doc_id
        .map(String::from)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let created = match state.put_doc(index_name, &id, body) {
        Ok(c) => c,
        Err(e) => return (500, storage_error(&e)),
    };
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
    if !state.index_exists(index_name) {
        return (
            404,
            json!({
                "_index": index_name,
                "_id": doc_id,
                "found": false,
            }),
        );
    }

    match state.get_doc(index_name, doc_id) {
        Ok(Some(doc)) => (
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
        Ok(None) => (
            404,
            json!({
                "_index": index_name,
                "_id": doc_id,
                "found": false,
            }),
        ),
        Err(e) => (500, storage_error(&e)),
    }
}

/// Update a document by ID (partial update with `doc` field, supports `doc_as_upsert`).
pub fn update_document(
    state: &OpenSearchState,
    index_name: &str,
    doc_id: &str,
    body: &Value,
) -> (u16, Value) {
    let partial = body.get("doc").cloned().unwrap_or(json!({}));
    let doc_as_upsert = body["doc_as_upsert"].as_bool().unwrap_or(false);

    if let Err(err) = ensure_index(state, index_name) {
        return (500, err);
    }

    let existing = match state.get_doc(index_name, doc_id) {
        Ok(d) => d,
        Err(e) => return (500, storage_error(&e)),
    };

    if let Some(mut existing) = existing {
        // Merge partial fields into existing document.
        if let (Some(existing_obj), Some(partial_obj)) =
            (existing.as_object_mut(), partial.as_object())
        {
            for (k, v) in partial_obj {
                existing_obj.insert(k.clone(), v.clone());
            }
        }
        if let Err(e) = state.put_doc(index_name, doc_id, &existing) {
            return (500, storage_error(&e));
        }
        (
            200,
            json!({
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
        if let Err(e) = state.put_doc(index_name, doc_id, &partial) {
            return (500, storage_error(&e));
        }
        (
            201,
            json!({
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
            json!({
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
        .unwrap_or(json!({"match_all": {}}));
    let script_source = body
        .pointer("/script/source")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let params = body.pointer("/script/params").cloned().unwrap_or(json!({}));

    let resolved = state.resolve_alias(index_name);
    let mut updated: usize = 0;

    for name in &resolved {
        if !state.index_exists(name) {
            continue;
        }
        // Collect the doc snapshot first, then mutate. Holding the
        // read transaction open while issuing writes would deadlock.
        let mut matching: Vec<(String, Value)> = Vec::new();
        let _ = state.for_each_doc(name, |id, doc| {
            if super::search::match_score(&query, doc) > 0.0 {
                matching.push((id.to_string(), doc.clone()));
            }
            true
        });

        for (id, mut doc) in matching {
            apply_script(&mut doc, &script_source, &params);
            if state.put_doc(name, &id, &doc).is_ok() {
                updated += 1;
            }
        }
    }

    (
        200,
        json!({
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
fn apply_script(doc: &mut Value, source: &str, params: &Value) {
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
        if let Some(rest) = stmt.strip_prefix("ctx._source.")
            && let Some(eq_pos) = rest.find('=')
        {
            let field = rest[..eq_pos].trim().to_string();
            let rhs = rest[eq_pos + 1..].trim();

            // params.paramName
            if let Some(param_path) = rhs.strip_prefix("params.") {
                let param_name = param_path.trim();
                if let Some(val) = params.get(param_name)
                    && let Some(obj) = doc.as_object_mut()
                {
                    obj.insert(field, val.clone());
                }
                continue;
            }

            // String literal 'value' or "value"
            if (rhs.starts_with('\'') && rhs.ends_with('\''))
                || (rhs.starts_with('"') && rhs.ends_with('"'))
            {
                let literal = &rhs[1..rhs.len() - 1];
                if let Some(obj) = doc.as_object_mut() {
                    obj.insert(field, json!(literal));
                }
                continue;
            }

            // Numeric literal
            if let Ok(n) = rhs.parse::<i64>() {
                if let Some(obj) = doc.as_object_mut() {
                    obj.insert(field, json!(n));
                }
                continue;
            }
            if let Ok(f) = rhs.parse::<f64>()
                && let Some(obj) = doc.as_object_mut()
            {
                obj.insert(field, json!(f));
            }
        }
    }
}

/// Delete a document by ID.
pub fn delete_document(state: &OpenSearchState, index_name: &str, doc_id: &str) -> (u16, Value) {
    if !state.index_exists(index_name) {
        return (
            404,
            json!({
                "_index": index_name,
                "_id": doc_id,
                "result": "not_found",
            }),
        );
    }

    let found = match state.delete_doc(index_name, doc_id) {
        Ok(b) => b,
        Err(e) => return (500, storage_error(&e)),
    };

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
