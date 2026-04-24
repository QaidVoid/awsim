use serde_json::{Value, json};

use crate::state::OpenSearchState;

/// Process a bulk request (NDJSON format).
///
/// Each action line is followed by an optional source document line.
/// Actions: index, create, update, delete
pub fn bulk(state: &OpenSearchState, body: &str) -> (u16, Value) {
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut items: Vec<Value> = Vec::new();
    let errors = false;
    let mut i = 0;

    while i < lines.len() {
        let action_line: Value = match serde_json::from_str(lines[i]) {
            Ok(v) => v,
            Err(_) => {
                i += 1;
                continue;
            }
        };

        let (action, meta) = if let Some(obj) = action_line.as_object() {
            if let Some((k, v)) = obj.iter().next() {
                (k.clone(), v.clone())
            } else {
                i += 1;
                continue;
            }
        } else {
            i += 1;
            continue;
        };

        let index_name = meta["_index"].as_str().unwrap_or("default");
        let doc_id = meta["_id"].as_str().map(String::from);

        match action.as_str() {
            "index" | "create" => {
                i += 1;
                if i >= lines.len() {
                    break;
                }
                let source: Value = serde_json::from_str(lines[i]).unwrap_or(json!({}));
                let (_, result) =
                    super::document::index_document(state, index_name, doc_id.as_deref(), &source);
                items.push(json!({ action: result }));
            }
            "delete" => {
                if let Some(id) = &doc_id {
                    let (_, result) = super::document::delete_document(state, index_name, id);
                    items.push(json!({ "delete": result }));
                }
            }
            "update" => {
                i += 1;
                if i >= lines.len() {
                    break;
                }
                let update_body: Value = serde_json::from_str(lines[i]).unwrap_or(json!({}));
                if let Some(id) = &doc_id {
                    // Simple: treat update as full replacement with doc
                    let doc = update_body
                        .get("doc")
                        .cloned()
                        .unwrap_or(update_body.clone());
                    let (_, result) =
                        super::document::index_document(state, index_name, Some(id), &doc);
                    items.push(json!({ "update": result }));
                }
            }
            _ => {}
        }

        i += 1;
    }

    (
        200,
        json!({
            "took": 1,
            "errors": errors,
            "items": items,
        }),
    )
}
