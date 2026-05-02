use serde_json::{Value, json};

use super::index::{index_not_found, storage_error};
use crate::state::{DocVersion, IndexMeta, OpenSearchState};

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
                uuid: uuid::Uuid::new_v4().to_string(),
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

    let (created, seq) = match state.put_doc(index_name, &id, body) {
        Ok(c) => c,
        Err(e) => return (500, storage_error(&e)),
    };

    let version = if created { 1 } else { seq };
    let status = if created { 201 } else { 200 };

    (
        status,
        json!({
            "_index": index_name,
            "_id": id,
            "_version": version,
            "result": if created { "created" } else { "updated" },
            "_shards": { "total": 2, "successful": 1, "failed": 0 },
            "_seq_no": seq,
            "_primary_term": 1,
        }),
    )
}

/// Get a document by ID.
pub fn get_document(state: &OpenSearchState, index_name: &str, doc_id: &str) -> (u16, Value) {
    if !state.index_exists(index_name) {
        return (404, index_not_found(index_name));
    }

    match state.get_doc(index_name, doc_id) {
        Ok(Some(doc)) => {
            let ver = state
                .get_doc_version(index_name, doc_id)
                .ok()
                .flatten()
                .unwrap_or(DocVersion {
                    version: 1,
                    seq_no: 0,
                    primary_term: 1,
                });
            (
                200,
                json!({
                    "_index": index_name,
                    "_id": doc_id,
                    "_version": ver.version,
                    "_seq_no": ver.seq_no,
                    "_primary_term": ver.primary_term,
                    "found": true,
                    "_source": doc,
                }),
            )
        }
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
        deep_merge(&mut existing, &partial);
        let (_, seq) = match state.put_doc(index_name, doc_id, &existing) {
            Ok(s) => s,
            Err(e) => return (500, storage_error(&e)),
        };
        (
            200,
            json!({
                "_index": index_name,
                "_id": doc_id,
                "_version": seq,
                "result": "updated",
                "_shards": { "total": 2, "successful": 1, "failed": 0 },
                "_seq_no": seq,
                "_primary_term": 1,
            }),
        )
    } else if doc_as_upsert {
        let (_, seq) = match state.put_doc(index_name, doc_id, &partial) {
            Ok(s) => s,
            Err(e) => return (500, storage_error(&e)),
        };
        (
            201,
            json!({
                "_index": index_name,
                "_id": doc_id,
                "_version": seq,
                "result": "created",
                "_shards": { "total": 2, "successful": 1, "failed": 0 },
                "_seq_no": seq,
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

/// Recursively merge `partial` into `target`. Nested objects are
/// merged rather than replaced.
fn deep_merge(target: &mut Value, partial: &Value) {
    if let (Some(target_obj), Some(partial_obj)) = (target.as_object_mut(), partial.as_object()) {
        for (k, v) in partial_obj {
            let entry = target_obj.entry(k.clone());
            if v.is_object() {
                let existing = entry.or_insert_with(|| json!({}));
                deep_merge(existing, v);
            } else {
                entry.and_modify(|e| *e = v.clone()).or_insert(v.clone());
            }
        }
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
/// - `ctx._source.nested.field = params.paramName` (dot-notation paths)
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
            remove_nested_field(doc, field);
            continue;
        }

        // ctx._source.field = ...
        if let Some(rest) = stmt.strip_prefix("ctx._source.")
            && let Some(eq_pos) = rest.find('=')
        {
            let field_path = rest[..eq_pos].trim().to_string();
            let rhs = rest[eq_pos + 1..].trim();

            // params.paramName
            if let Some(param_path) = rhs.strip_prefix("params.") {
                let param_name = param_path.trim();
                if let Some(val) = params.get(param_name) {
                    set_nested_field(doc, &field_path, val.clone());
                }
                continue;
            }

            // String literal 'value' or "value"
            if (rhs.starts_with('\'') && rhs.ends_with('\''))
                || (rhs.starts_with('"') && rhs.ends_with('"'))
            {
                let literal = &rhs[1..rhs.len() - 1];
                set_nested_field(doc, &field_path, json!(literal));
                continue;
            }

            // Numeric literal
            if let Ok(n) = rhs.parse::<i64>() {
                set_nested_field(doc, &field_path, json!(n));
                continue;
            }
            if let Ok(f) = rhs.parse::<f64>() {
                set_nested_field(doc, &field_path, json!(f));
            }
        }
    }
}

/// Set a nested field using dot-notation (e.g. `"nested.field"`).
fn set_nested_field(doc: &mut Value, path: &str, value: Value) {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return;
    }
    let mut current = doc;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            if let Some(obj) = current.as_object_mut() {
                obj.insert(part.to_string(), value.clone());
            }
        } else {
            if (current.get(*part).is_none() || !current[*part].is_object())
                && let Some(obj) = current.as_object_mut()
            {
                obj.insert(part.to_string(), json!({}));
            }
            current = current.get_mut(*part).unwrap();
        }
    }
}

/// Remove a nested field using dot-notation.
fn remove_nested_field(doc: &mut Value, path: &str) {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return;
    }
    let mut current = doc;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            if let Some(obj) = current.as_object_mut() {
                obj.remove(*part);
            }
        } else {
            if current.get(*part).is_none() {
                return;
            }
            current = current.get_mut(*part).unwrap();
        }
    }
}

/// Delete a document by ID.
pub fn delete_document(state: &OpenSearchState, index_name: &str, doc_id: &str) -> (u16, Value) {
    if !state.index_exists(index_name) {
        return (404, index_not_found(index_name));
    }

    let ver = state
        .get_doc_version(index_name, doc_id)
        .ok()
        .flatten()
        .unwrap_or(DocVersion {
            version: 1,
            seq_no: 0,
            primary_term: 1,
        });

    let found = match state.delete_doc(index_name, doc_id) {
        Ok(b) => b,
        Err(e) => return (500, storage_error(&e)),
    };

    let seq = state.global_seq_no();
    (
        if found { 200 } else { 404 },
        json!({
            "_index": index_name,
            "_id": doc_id,
            "_version": ver.version + if found { 1 } else { 0 },
            "result": if found { "deleted" } else { "not_found" },
            "_shards": { "total": 2, "successful": 1, "failed": 0 },
            "_seq_no": if found { seq } else { ver.seq_no },
            "_primary_term": 1,
        }),
    )
}

/// Multi-get: retrieve multiple documents by ID.
pub fn mget(
    state: &OpenSearchState,
    index_name: &str,
    body: &Value,
) -> (u16, Value) {
    if !state.index_exists(index_name) {
        return (404, index_not_found(index_name));
    }

    let ids: Vec<String> = body
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let docs: Vec<String> = body
        .get("docs")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|d| d.get("_id").and_then(|id| id.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let all_ids: Vec<&str> = ids.iter().map(|s| s.as_str()).chain(docs.iter().map(|s| s.as_str())).collect();

    let mut results: Vec<Value> = Vec::new();
    for id in &all_ids {
        match state.get_doc(index_name, id) {
            Ok(Some(doc)) => {
                let ver = state
                    .get_doc_version(index_name, id)
                    .ok()
                    .flatten()
                    .unwrap_or(DocVersion {
                        version: 1,
                        seq_no: 0,
                        primary_term: 1,
                    });
                results.push(json!({
                    "_index": index_name,
                    "_id": id,
                    "_version": ver.version,
                    "_seq_no": ver.seq_no,
                    "_primary_term": ver.primary_term,
                    "found": true,
                    "_source": doc,
                }));
            }
            _ => {
                results.push(json!({
                    "_index": index_name,
                    "_id": id,
                    "found": false,
                }));
            }
        }
    }

    (200, json!({ "docs": results }))
}

/// Delete documents matching a query.
pub fn delete_by_query(
    state: &OpenSearchState,
    index_name: &str,
    body: &Value,
) -> (u16, Value) {
    if !state.index_exists(index_name) {
        return (404, index_not_found(index_name));
    }

    let query = body
        .get("query")
        .cloned()
        .unwrap_or(json!({"match_all": {}}));

    let mut matching: Vec<String> = Vec::new();
    let _ = state.for_each_doc(index_name, |id, doc| {
        if super::search::match_score(&query, doc) > 0.0 {
            matching.push(id.to_string());
        }
        true
    });

    let deleted = matching.len();
    for id in &matching {
        let _ = state.delete_doc(index_name, id);
    }

    let failures: Vec<Value> = Vec::new();

    (
        200,
        json!({
            "took": 1,
            "timed_out": false,
            "total": deleted,
            "deleted": deleted,
            "batches": 1,
            "version_conflicts": 0,
            "noops": 0,
            "retries": { "bulk": 0, "search": 0 },
            "throttled_millis": 0,
            "requests_per_second": -1.0,
            "throttled_until_millis": 0,
            "failures": failures,
        }),
    )
}
