use serde_json::{Value, json};
use std::collections::HashMap;

use crate::state::OpenSearchState;

/// GET /_cluster/health
pub fn cluster_health(state: &OpenSearchState) -> (u16, Value) {
    let index_count = state.indices.len();

    (
        200,
        json!({
            "cluster_name": "awsim",
            "status": "green",
            "timed_out": false,
            "number_of_nodes": 1,
            "number_of_data_nodes": 1,
            "active_primary_shards": index_count,
            "active_shards": index_count,
            "relocating_shards": 0,
            "initializing_shards": 0,
            "unassigned_shards": 0,
            "delayed_unassigned_shards": 0,
            "number_of_pending_tasks": 0,
            "number_of_in_flight_fetch": 0,
            "task_max_waiting_in_queue_millis": 0,
            "active_shards_percent_as_number": 100.0,
        }),
    )
}

/// GET /_tasks/{task_id}
///
/// Returns a fake completed task. All tasks in the emulator are synchronous,
/// so any task ID that is queried will be reported as completed.
pub fn get_task(task_id: &str) -> (u16, Value) {
    (
        200,
        json!({
            "completed": true,
            "task": {
                "node": "awsim-node-1",
                "id": 1,
                "type": "transport",
                "action": "indices:data/write/reindex",
                "description": task_id,
                "start_time_in_millis": 0,
                "running_time_in_nanos": 0,
                "status": {
                    "total": 0,
                    "updated": 0,
                    "created": 0,
                    "deleted": 0,
                    "batches": 1,
                    "version_conflicts": 0,
                    "noops": 0,
                    "throttled_millis": 0,
                    "requests_per_second": -1.0,
                    "throttled_until_millis": 0,
                }
            },
            "response": {
                "took": 1,
                "timed_out": false,
                "total": 0,
                "updated": 0,
                "created": 0,
                "deleted": 0,
                "batches": 1,
                "version_conflicts": 0,
                "noops": 0,
                "throttled_millis": 0,
                "requests_per_second": -1.0,
                "throttled_until_millis": 0,
                "failures": [],
            }
        }),
    )
}

/// POST /_reindex
///
/// Copies all documents from source index to destination index.
/// Creates the destination index if it does not exist.
pub fn reindex(state: &OpenSearchState, body: &Value, wait_for_completion: bool) -> (u16, Value) {
    let source_index = body
        .pointer("/source/index")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let dest_index = body
        .pointer("/dest/index")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if source_index.is_empty() || dest_index.is_empty() {
        return (
            400,
            json!({
                "error": "source.index and dest.index are required",
                "status": 400,
            }),
        );
    }

    // Collect documents from source
    let docs: HashMap<String, Value> = if let Some(idx) = state.indices.get(source_index) {
        idx.documents.clone()
    } else {
        HashMap::new()
    };

    let count = docs.len();

    // Auto-create dest index if needed
    if !state.indices.contains_key(dest_index) {
        // Copy mappings/settings from source if available
        let (mappings, settings) = if let Some(src) = state.indices.get(source_index) {
            (src.mappings.clone(), src.settings.clone())
        } else {
            (json!({}), json!({}))
        };

        state.indices.insert(
            dest_index.to_string(),
            crate::state::OpenSearchIndex {
                name: dest_index.to_string(),
                mappings,
                settings,
                documents: HashMap::new(),
                created_at: crate::util::now_iso8601(),
            },
        );
    }

    // Copy documents
    if let Some(mut dest) = state.indices.get_mut(dest_index) {
        for (id, doc) in docs {
            dest.documents.insert(id, doc);
        }
    }

    let task_id = format!("awsim-task-{}", uuid::Uuid::new_v4());

    if wait_for_completion {
        (
            200,
            json!({
                "took": 1,
                "timed_out": false,
                "total": count,
                "updated": 0,
                "created": count,
                "deleted": 0,
                "batches": 1,
                "version_conflicts": 0,
                "noops": 0,
                "throttled_millis": 0,
                "requests_per_second": -1.0,
                "throttled_until_millis": 0,
                "failures": [],
            }),
        )
    } else {
        (200, json!({ "task": task_id }))
    }
}

/// POST /_aliases
///
/// Processes alias add/remove actions.
pub fn update_aliases(state: &OpenSearchState, body: &Value) -> (u16, Value) {
    let actions = match body.get("actions").and_then(|a| a.as_array()) {
        Some(a) => a.clone(),
        None => {
            return (
                400,
                json!({
                    "error": "actions array is required",
                    "status": 400,
                }),
            );
        }
    };

    for action in &actions {
        if let Some(add) = action.get("add") {
            let index = add["index"].as_str().unwrap_or("").to_string();
            let alias = add["alias"].as_str().unwrap_or("").to_string();
            if !index.is_empty() && !alias.is_empty() {
                state
                    .aliases
                    .entry(alias)
                    .and_modify(|v| {
                        if !v.contains(&index) {
                            v.push(index.clone());
                        }
                    })
                    .or_insert_with(|| vec![index]);
            }
        }

        if let Some(remove) = action.get("remove") {
            let index = remove["index"].as_str().unwrap_or("");
            let alias = remove["alias"].as_str().unwrap_or("").to_string();
            if !alias.is_empty() {
                if let Some(mut entry) = state.aliases.get_mut(&alias) {
                    entry.retain(|i| i != index);
                }
                // Remove the alias entry entirely if empty
                let is_empty = state
                    .aliases
                    .get(&alias)
                    .map(|v| v.is_empty())
                    .unwrap_or(false);
                if is_empty {
                    state.aliases.remove(&alias);
                }
            }
        }
    }

    (200, json!({ "acknowledged": true }))
}

/// POST /_msearch or POST /{index}/_msearch
///
/// Processes an NDJSON body of alternating header/body pairs.
/// Returns `{"responses": [...]}` where each element is a search result.
pub fn msearch(state: &OpenSearchState, default_index: Option<&str>, body: &str) -> (u16, Value) {
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut responses: Vec<Value> = Vec::new();

    let mut i = 0;
    while i + 1 < lines.len() {
        let header: Value = match serde_json::from_str(lines[i]) {
            Ok(v) => v,
            Err(_) => {
                i += 2;
                continue;
            }
        };
        let search_body: Value = match serde_json::from_str(lines[i + 1]) {
            Ok(v) => v,
            Err(_) => {
                i += 2;
                continue;
            }
        };

        // Resolve index from header, fallback to default
        let index = header["index"].as_str().or(default_index).unwrap_or("_all");

        let (_, result) = super::search::search(state, index, &search_body);
        responses.push(result);

        i += 2;
    }

    (200, json!({ "responses": responses }))
}
