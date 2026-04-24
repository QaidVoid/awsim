use serde_json::{Value, json};

use crate::state::OpenSearchState;

/// Search documents in an index (or multiple indices).
///
/// Supports:
/// - `match` queries (single field)
/// - `multi_match` queries (multiple fields)
/// - `match_all` queries
/// - `bool` queries with `must`, `should`, `filter`
/// - `term` queries (exact match)
/// - `query_string` queries
pub fn search(state: &OpenSearchState, index_pattern: &str, body: &Value) -> (u16, Value) {
    let size = body["size"].as_u64().unwrap_or(10) as usize;
    let from = body["from"].as_u64().unwrap_or(0) as usize;
    let query = body
        .get("query")
        .cloned()
        .unwrap_or(json!({"match_all": {}}));

    // Resolve index pattern (support wildcards like "captify-*")
    let matching_indices: Vec<String> = if index_pattern.contains('*') {
        let prefix = index_pattern.trim_end_matches('*');
        state
            .indices
            .iter()
            .filter(|e| e.key().starts_with(prefix))
            .map(|e| e.key().clone())
            .collect()
    } else {
        // Could be comma-separated; resolve aliases as well
        index_pattern
            .split(',')
            .flat_map(|s| {
                let name = s.trim().to_string();
                // If the name matches an alias, expand to the aliased indices
                if let Some(aliased) = state.aliases.get(&name) {
                    aliased.clone()
                } else {
                    vec![name]
                }
            })
            .collect()
    };

    let mut hits: Vec<Value> = Vec::new();

    for idx_name in &matching_indices {
        if let Some(idx) = state.indices.get(idx_name) {
            for (doc_id, doc) in &idx.documents {
                let score = match_score(&query, doc);
                if score > 0.0 {
                    hits.push(json!({
                        "_index": idx_name,
                        "_id": doc_id,
                        "_score": score,
                        "_source": doc,
                    }));
                }
            }
        }
    }

    // Sort by score descending
    hits.sort_by(|a, b| {
        let sa = a["_score"].as_f64().unwrap_or(0.0);
        let sb = b["_score"].as_f64().unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    let total = hits.len();
    let paged: Vec<Value> = hits.into_iter().skip(from).take(size).collect();

    (
        200,
        json!({
            "took": 1,
            "timed_out": false,
            "_shards": { "total": 1, "successful": 1, "skipped": 0, "failed": 0 },
            "hits": {
                "total": { "value": total, "relation": "eq" },
                "max_score": paged.first().and_then(|h| h["_score"].as_f64()).unwrap_or(0.0),
                "hits": paged,
            }
        }),
    )
}

/// Count documents matching a query.
pub fn count(state: &OpenSearchState, index_name: &str, body: &Value) -> (u16, Value) {
    let query = body
        .get("query")
        .cloned()
        .unwrap_or(json!({"match_all": {}}));

    // Resolve alias if needed
    let resolved: Vec<String> = if let Some(aliased) = state.aliases.get(index_name) {
        aliased.clone()
    } else {
        vec![index_name.to_string()]
    };

    let count: usize = resolved
        .iter()
        .filter_map(|name| state.indices.get(name))
        .map(|idx| {
            idx.documents
                .values()
                .filter(|doc| match_score(&query, doc) > 0.0)
                .count()
        })
        .sum();

    (
        200,
        json!({
            "count": count,
            "_shards": { "total": 1, "successful": 1, "skipped": 0, "failed": 0 },
        }),
    )
}

/// Score a document against a query. Returns 0.0 for no match.
pub(crate) fn match_score(query: &Value, doc: &Value) -> f64 {
    if let Some(obj) = query.as_object() {
        if obj.contains_key("match_all") {
            return 1.0;
        }

        // match: { "field": "value" } or { "field": { "query": "value" } }
        if let Some(match_obj) = obj.get("match").and_then(|m| m.as_object()) {
            for (field, match_val) in match_obj {
                let query_text = match_val
                    .as_str()
                    .or_else(|| match_val.get("query").and_then(|q| q.as_str()))
                    .unwrap_or("");
                if let Some(field_val) = get_nested_field(doc, field) {
                    let field_str = value_to_string(&field_val);
                    return text_match_score(query_text, &field_str);
                }
            }
            return 0.0;
        }

        // multi_match: { "query": "text", "fields": ["f1", "f2"] }
        if let Some(mm) = obj.get("multi_match").and_then(|m| m.as_object()) {
            let query_text = mm.get("query").and_then(|q| q.as_str()).unwrap_or("");
            let fields = mm
                .get("fields")
                .and_then(|f| f.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                .unwrap_or_default();

            let mut best_score = 0.0;
            for field in &fields {
                // Handle boosted fields like "title^2"
                let (field_name, boost) = if let Some(pos) = field.find('^') {
                    let (name, b) = field.split_at(pos);
                    (name, b[1..].parse::<f64>().unwrap_or(1.0))
                } else {
                    (*field, 1.0)
                };

                if let Some(field_val) = get_nested_field(doc, field_name) {
                    let field_str = value_to_string(&field_val);
                    let score = text_match_score(query_text, &field_str) * boost;
                    if score > best_score {
                        best_score = score;
                    }
                }
            }
            return best_score;
        }

        // term: { "field": "exact_value" }
        if let Some(term_obj) = obj.get("term").and_then(|t| t.as_object()) {
            for (field, expected) in term_obj {
                let expected_str = expected
                    .as_str()
                    .or_else(|| expected.get("value").and_then(|v| v.as_str()))
                    .unwrap_or("");
                if let Some(field_val) = get_nested_field(doc, field) {
                    let field_str = value_to_string(&field_val);
                    if field_str == expected_str {
                        return 1.0;
                    }
                }
            }
            return 0.0;
        }

        // bool: { "must": [...], "should": [...], "filter": [...] }
        if let Some(bool_obj) = obj.get("bool").and_then(|b| b.as_object()) {
            let mut total_score = 0.0;
            let mut must_pass = true;

            if let Some(must) = bool_obj.get("must").and_then(|m| m.as_array()) {
                for clause in must {
                    let s = match_score(clause, doc);
                    if s <= 0.0 {
                        must_pass = false;
                        break;
                    }
                    total_score += s;
                }
            }

            if let Some(filter) = bool_obj.get("filter").and_then(|f| f.as_array()) {
                for clause in filter {
                    if match_score(clause, doc) <= 0.0 {
                        must_pass = false;
                        break;
                    }
                }
            }

            if !must_pass {
                return 0.0;
            }

            if let Some(should) = bool_obj.get("should").and_then(|s| s.as_array()) {
                for clause in should {
                    total_score += match_score(clause, doc);
                }
            }

            return if total_score > 0.0 {
                total_score
            } else if must_pass {
                1.0
            } else {
                0.0
            };
        }

        // query_string: { "query": "text" }
        if let Some(qs) = obj.get("query_string").and_then(|q| q.as_object()) {
            let query_text = qs.get("query").and_then(|q| q.as_str()).unwrap_or("");
            let doc_str = serde_json::to_string(doc)
                .unwrap_or_default()
                .to_lowercase();
            let query_lower = query_text.to_lowercase();
            return if query_lower
                .split_whitespace()
                .any(|term| doc_str.contains(term))
            {
                0.5
            } else {
                0.0
            };
        }
    }

    // Default: no match
    0.0
}

/// Simple text matching score.
fn text_match_score(query: &str, field: &str) -> f64 {
    let query_lower = query.to_lowercase();
    let field_lower = field.to_lowercase();
    let terms: Vec<&str> = query_lower.split_whitespace().collect();

    if terms.is_empty() {
        return 0.0;
    }

    let matched = terms
        .iter()
        .filter(|term| field_lower.contains(*term))
        .count();

    if matched == 0 {
        return 0.0;
    }

    (matched as f64) / (terms.len() as f64)
}

/// Get a nested field value from a JSON document using dot notation.
fn get_nested_field<'a>(doc: &'a Value, field: &str) -> Option<&'a Value> {
    let mut current = doc;
    for part in field.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

/// Convert a Value to a searchable string.
fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(arr) => arr
            .iter()
            .map(value_to_string)
            .collect::<Vec<_>>()
            .join(" "),
        Value::Object(obj) => obj
            .values()
            .map(value_to_string)
            .collect::<Vec<_>>()
            .join(" "),
        Value::Null => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::OpenSearchIndex;

    fn test_state() -> OpenSearchState {
        let state = OpenSearchState::default();
        let mut docs = std::collections::HashMap::new();
        docs.insert("1".to_string(), json!({"title": "Rust Programming", "body": "Learn Rust for systems programming", "tags": ["rust", "systems"]}));
        docs.insert("2".to_string(), json!({"title": "Python Guide", "body": "Python is great for data science", "tags": ["python", "data"]}));
        docs.insert("3".to_string(), json!({"title": "AWS Lambda", "body": "Serverless computing with AWS Lambda and Rust", "tags": ["aws", "lambda", "rust"]}));

        state.indices.insert(
            "articles".to_string(),
            OpenSearchIndex {
                name: "articles".to_string(),
                mappings: json!({}),
                settings: json!({}),
                documents: docs,
                created_at: "2026-01-01".to_string(),
            },
        );
        state
    }

    #[test]
    fn test_match_all() {
        let state = test_state();
        let (status, result) = search(&state, "articles", &json!({"query": {"match_all": {}}}));
        assert_eq!(status, 200);
        assert_eq!(result["hits"]["total"]["value"], 3);
    }

    #[test]
    fn test_match_query() {
        let state = test_state();
        let (_, result) = search(
            &state,
            "articles",
            &json!({"query": {"match": {"title": "Rust"}}}),
        );
        let hits = result["hits"]["hits"].as_array().unwrap();
        assert!(!hits.is_empty());
        assert!(
            hits.iter()
                .any(|h| h["_source"]["title"].as_str().unwrap().contains("Rust"))
        );
    }

    #[test]
    fn test_multi_match() {
        let state = test_state();
        let (_, result) = search(
            &state,
            "articles",
            &json!({
                "query": {"multi_match": {"query": "Rust", "fields": ["title^2", "body"]}}
            }),
        );
        let hits = result["hits"]["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 2); // "Rust Programming" and "AWS Lambda" (body mentions Rust)
    }

    #[test]
    fn test_bool_must() {
        let state = test_state();
        let (_, result) = search(
            &state,
            "articles",
            &json!({
                "query": {"bool": {"must": [{"match": {"body": "Rust"}}, {"match": {"body": "Lambda"}}]}}
            }),
        );
        let hits = result["hits"]["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["_source"]["title"], "AWS Lambda");
    }

    #[test]
    fn test_wildcard_index() {
        let state = test_state();
        let (_, result) = search(&state, "art*", &json!({"query": {"match_all": {}}}));
        assert_eq!(result["hits"]["total"]["value"], 3);
    }

    #[test]
    fn test_pagination() {
        let state = test_state();
        let (_, result) = search(
            &state,
            "articles",
            &json!({"query": {"match_all": {}}, "size": 2, "from": 0}),
        );
        let hits = result["hits"]["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(result["hits"]["total"]["value"], 3);
    }
}
