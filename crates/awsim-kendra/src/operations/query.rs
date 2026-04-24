use std::collections::HashMap;

use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::{DocumentAttribute, DocumentAttributeValue, IndexedDocument, KendraState};

/// Query — full-text search across indexed documents.
///
/// Does simple substring matching against document content and titles.
/// Real Kendra uses ML-based semantic search; this is a functional stub.
pub fn query(state: &KendraState, input: &Value) -> Result<Value, AwsError> {
    let index_id = input["IndexId"]
        .as_str()
        .ok_or_else(|| AwsError::validation("IndexId is required"))?;
    let query_text = input["QueryText"]
        .as_str()
        .ok_or_else(|| AwsError::validation("QueryText is required"))?;
    let page_size = input["PageSize"].as_u64().unwrap_or(10) as usize;

    let index = state.indexes.get(index_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Index {index_id} not found"),
        )
    })?;

    let query_lower = query_text.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

    // Collect matching documents (with scores) before pagination
    struct ScoredDoc<'a> {
        doc: &'a IndexedDocument,
        score: f64,
    }

    let mut scored: Vec<ScoredDoc<'_>> = Vec::new();

    for doc in &index.documents {
        let content_lower = doc.content.to_lowercase();
        let title_lower = doc.title.as_deref().unwrap_or("").to_lowercase();

        // Score based on term matches
        let mut score: f64 = 0.0;
        for term in &query_terms {
            if content_lower.contains(term) {
                score += 0.3;
            }
            if title_lower.contains(term) {
                score += 0.5;
            }
        }

        // When QueryText is empty or blank, include all documents with a neutral score
        if query_terms.is_empty() {
            score = 0.1;
        }

        if score > 0.0 {
            // Apply AttributeFilter if present
            if let Some(filter) = input.get("AttributeFilter")
                && !evaluate_attribute_filter(filter, &doc.attributes) {
                    continue;
                }
            scored.push(ScoredDoc { doc, score });
        }
    }

    // Sort by SortingConfiguration if provided, otherwise by score descending
    if let Some(sorting) = input.get("SortingConfiguration") {
        let attr_key = sorting["DocumentAttributeKey"].as_str().unwrap_or("");
        let order = sorting["SortOrder"].as_str().unwrap_or("DESC");
        scored.sort_by(|a, b| {
            let va = attribute_sort_key(a.doc.attributes.get(attr_key));
            let vb = attribute_sort_key(b.doc.attributes.get(attr_key));
            let cmp = va.cmp(&vb);
            if order == "ASC" { cmp } else { cmp.reverse() }
        });
    } else {
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    // Build facet results before truncating (use all matching docs)
    let facet_results: Vec<Value> =
        if let Some(facets) = input.get("Facets").and_then(|f| f.as_array()) {
            facets
                .iter()
                .map(|facet| {
                    let key = facet["DocumentAttributeKey"].as_str().unwrap_or("");
                    let mut value_counts: HashMap<String, u32> = HashMap::new();
                    for sd in &scored {
                        if let Some(attr) = sd.doc.attributes.get(key) {
                            let val_str = attribute_value_to_string(&attr.value);
                            *value_counts.entry(val_str).or_default() += 1;
                        }
                    }
                    json!({
                        "DocumentAttributeKey": key,
                        "DocumentAttributeValueCountPairs": value_counts.iter().map(|(v, c)| json!({
                            "DocumentAttributeValue": {"StringValue": v},
                            "Count": c,
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

    let total = scored.len();
    scored.truncate(page_size);

    let results: Vec<Value> = scored
        .iter()
        .map(|sd| {
            let doc = sd.doc;
            let snippet = extract_snippet(&doc.content, &query_terms, 200);
            json!({
                "Id": doc.id,
                "Type": "DOCUMENT",
                "DocumentId": doc.id,
                "DocumentTitle": {
                    "Text": doc.title.as_deref().unwrap_or(""),
                    "Highlights": [],
                },
                "DocumentExcerpt": {
                    "Text": snippet,
                    "Highlights": [],
                },
                "DocumentURI": null,
                "ScoreAttributes": {
                    "ScoreConfidence": if sd.score > 0.5 { "VERY_HIGH" } else { "MEDIUM" },
                },
                "RelevanceScore": sd.score.min(1.0),
            })
        })
        .collect();

    Ok(json!({
        "QueryId": uuid::Uuid::new_v4().to_string(),
        "ResultItems": results,
        "TotalNumberOfResults": total,
        "FacetResults": facet_results,
    }))
}

/// Evaluate an AttributeFilter against a document's attribute map.
fn evaluate_attribute_filter(filter: &Value, attrs: &HashMap<String, DocumentAttribute>) -> bool {
    let Some(obj) = filter.as_object() else {
        return true;
    };

    // EqualsTo
    if let Some(eq) = obj.get("EqualsTo") {
        let key = eq["Key"].as_str().unwrap_or("");
        return match_attribute_value(attrs.get(key), &eq["Value"]);
    }
    // ContainsAll
    if let Some(ca) = obj.get("ContainsAll") {
        let key = ca["Key"].as_str().unwrap_or("");
        return match_contains_all(attrs.get(key), &ca["Value"]);
    }
    // ContainsAny
    if let Some(ca) = obj.get("ContainsAny") {
        let key = ca["Key"].as_str().unwrap_or("");
        return match_contains_any(attrs.get(key), &ca["Value"]);
    }
    // AndAllFilters
    if let Some(filters) = obj.get("AndAllFilters").and_then(|f| f.as_array()) {
        return filters.iter().all(|f| evaluate_attribute_filter(f, attrs));
    }
    // OrAllFilters
    if let Some(filters) = obj.get("OrAllFilters").and_then(|f| f.as_array()) {
        return filters.iter().any(|f| evaluate_attribute_filter(f, attrs));
    }
    // NotFilter
    if let Some(not_filter) = obj.get("NotFilter") {
        return !evaluate_attribute_filter(not_filter, attrs);
    }
    // GreaterThan
    if let Some(gt) = obj.get("GreaterThan") {
        let key = gt["Key"].as_str().unwrap_or("");
        let val = gt["Value"]["LongValue"].as_i64().unwrap_or(0);
        return attrs.get(key).is_some_and(|a| match &a.value {
            DocumentAttributeValue::LongValue(v) => *v > val,
            _ => false,
        });
    }
    // LessThan
    if let Some(lt) = obj.get("LessThan") {
        let key = lt["Key"].as_str().unwrap_or("");
        let val = lt["Value"]["LongValue"].as_i64().unwrap_or(0);
        return attrs.get(key).is_some_and(|a| match &a.value {
            DocumentAttributeValue::LongValue(v) => *v < val,
            _ => false,
        });
    }

    true // Unknown filter clause = pass
}

/// Check whether a document attribute equals an expected JSON value.
fn match_attribute_value(attr: Option<&DocumentAttribute>, expected: &Value) -> bool {
    let Some(attr) = attr else {
        return false;
    };
    match &attr.value {
        DocumentAttributeValue::StringValue(s) => {
            expected["StringValue"].as_str().is_some_and(|e| e == s)
        }
        DocumentAttributeValue::LongValue(n) => {
            expected["LongValue"].as_i64() == Some(*n)
        }
        DocumentAttributeValue::DateValue(d) => {
            expected["DateValue"].as_str().is_some_and(|e| e == d)
        }
        DocumentAttributeValue::StringListValue(list) => {
            if let Some(arr) = expected["StringListValue"].as_array() {
                let expected_strings: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                list.iter().any(|s| expected_strings.contains(&s.as_str()))
            } else {
                false
            }
        }
    }
}

/// ContainsAll — all expected list values must be present in the attribute.
fn match_contains_all(attr: Option<&DocumentAttribute>, expected: &Value) -> bool {
    let Some(attr) = attr else {
        return false;
    };
    if let Some(arr) = expected["StringListValue"].as_array() {
        let expected_strings: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
        match &attr.value {
            DocumentAttributeValue::StringListValue(list) => {
                expected_strings.iter().all(|e| list.iter().any(|s| s == e))
            }
            DocumentAttributeValue::StringValue(s) => expected_strings.iter().all(|e| s == e),
            _ => false,
        }
    } else {
        match_attribute_value(Some(attr), expected)
    }
}

/// ContainsAny — at least one expected value must be present in the attribute.
fn match_contains_any(attr: Option<&DocumentAttribute>, expected: &Value) -> bool {
    let Some(attr) = attr else {
        return false;
    };
    if let Some(arr) = expected["StringListValue"].as_array() {
        let expected_strings: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
        match &attr.value {
            DocumentAttributeValue::StringListValue(list) => {
                expected_strings.iter().any(|e| list.iter().any(|s| s == e))
            }
            DocumentAttributeValue::StringValue(s) => expected_strings.iter().any(|e| s == e),
            _ => false,
        }
    } else {
        match_attribute_value(Some(attr), expected)
    }
}

/// Convert a DocumentAttributeValue to a string for facet counting and sort keys.
fn attribute_value_to_string(value: &DocumentAttributeValue) -> String {
    match value {
        DocumentAttributeValue::StringValue(s) => s.clone(),
        DocumentAttributeValue::LongValue(n) => n.to_string(),
        DocumentAttributeValue::DateValue(d) => d.clone(),
        DocumentAttributeValue::StringListValue(list) => list.join(","),
    }
}

/// Produce a sort key string from an optional attribute (missing attrs sort last).
fn attribute_sort_key(attr: Option<&DocumentAttribute>) -> String {
    attr.map(|a| attribute_value_to_string(&a.value))
        .unwrap_or_else(|| "\u{FFFF}".to_string()) // sort missing values last
}

/// Retrieve — passage-level retrieval from indexed documents.
///
/// Similar to Query but returns individual passages rather than full documents.
pub fn retrieve(state: &KendraState, input: &Value) -> Result<Value, AwsError> {
    let index_id = input["IndexId"]
        .as_str()
        .ok_or_else(|| AwsError::validation("IndexId is required"))?;
    let query_text = input["QueryText"]
        .as_str()
        .ok_or_else(|| AwsError::validation("QueryText is required"))?;
    let page_size = input["PageSize"].as_u64().unwrap_or(10) as usize;

    let index = state.indexes.get(index_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Index {index_id} not found"),
        )
    })?;

    let query_lower = query_text.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

    let mut results: Vec<Value> = Vec::new();

    for doc in &index.documents {
        let content_lower = doc.content.to_lowercase();

        if query_terms.iter().any(|term| content_lower.contains(term)) {
            let snippet = extract_snippet(&doc.content, &query_terms, 300);

            results.push(json!({
                "Id": uuid::Uuid::new_v4().to_string(),
                "DocumentId": doc.id,
                "DocumentTitle": doc.title.as_deref().unwrap_or(""),
                "Content": snippet,
                "DocumentURI": null,
                "ScoreAttributes": {
                    "ScoreConfidence": "MEDIUM",
                },
            }));
        }
    }

    results.truncate(page_size);

    Ok(json!({
        "QueryId": uuid::Uuid::new_v4().to_string(),
        "ResultItems": results,
    }))
}

/// SubmitFeedback — submit relevance feedback for a query result.
///
/// Stub — stores nothing but returns success.
pub fn submit_feedback(_state: &KendraState, input: &Value) -> Result<Value, AwsError> {
    let _index_id = input["IndexId"]
        .as_str()
        .ok_or_else(|| AwsError::validation("IndexId is required"))?;
    let _query_id = input["QueryId"]
        .as_str()
        .ok_or_else(|| AwsError::validation("QueryId is required"))?;

    // Accept feedback silently — no ML model to update in dev emulator
    Ok(json!({}))
}

/// Extract a relevant snippet from content around matching terms.
fn extract_snippet(content: &str, terms: &[&str], max_len: usize) -> String {
    let content_lower = content.to_lowercase();

    // Find the first matching term's position
    let mut best_pos = 0;
    for term in terms {
        if let Some(pos) = content_lower.find(term) {
            best_pos = pos;
            break;
        }
    }

    // Extract a window around the match
    let start = best_pos.saturating_sub(max_len / 4);
    let end = (start + max_len).min(content.len());

    // Align to word boundaries
    let start = if start > 0 {
        content[start..]
            .find(' ')
            .map(|p| start + p + 1)
            .unwrap_or(start)
    } else {
        0
    };

    let snippet = &content[start..end];
    let snippet = snippet.trim();

    if start > 0 {
        format!("...{snippet}")
    } else if end < content.len() {
        format!("{snippet}...")
    } else {
        snippet.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{IndexedDocument, KendraIndex};

    fn test_state() -> KendraState {
        let state = KendraState::default();
        state.indexes.insert(
            "test-idx".to_string(),
            KendraIndex {
                id: "test-idx".to_string(),
                name: "Test".to_string(),
                arn: "arn:aws:kendra:us-east-1:000000000000:index/test-idx".to_string(),
                description: String::new(),
                role_arn: String::new(),
                edition: "DEVELOPER_EDITION".to_string(),
                status: "ACTIVE".to_string(),
                created_at: "2026-01-01T00:00:00Z".to_string(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
                documents: vec![
                    IndexedDocument {
                        id: "doc1".to_string(),
                        title: Some("Getting Started with Rust".to_string()),
                        content: "Rust is a systems programming language focused on safety and performance.".to_string(),
                        content_type: "PLAIN_TEXT".to_string(),
                        attributes: Default::default(),
                        created_at: "2026-01-01T00:00:00Z".to_string(),
                    },
                    IndexedDocument {
                        id: "doc2".to_string(),
                        title: Some("AWS Lambda Guide".to_string()),
                        content: "AWS Lambda lets you run code without managing servers. It supports Node.js, Python, and Rust.".to_string(),
                        content_type: "PLAIN_TEXT".to_string(),
                        attributes: Default::default(),
                        created_at: "2026-01-01T00:00:00Z".to_string(),
                    },
                ],
                data_sources: Default::default(),
                faqs: Default::default(),
            },
        );
        state
    }

    #[test]
    fn test_query_finds_matching_docs() {
        let state = test_state();
        let result = query(
            &state,
            &json!({"IndexId": "test-idx", "QueryText": "Rust programming"}),
        )
        .unwrap();
        let items = result["ResultItems"].as_array().unwrap();
        assert!(!items.is_empty());
        assert_eq!(items[0]["DocumentId"], "doc1");
    }

    #[test]
    fn test_query_no_results() {
        let state = test_state();
        let result = query(
            &state,
            &json!({"IndexId": "test-idx", "QueryText": "xyz_nonexistent_term"}),
        )
        .unwrap();
        let items = result["ResultItems"].as_array().unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_retrieve_returns_passages() {
        let state = test_state();
        let result = retrieve(
            &state,
            &json!({"IndexId": "test-idx", "QueryText": "Lambda"}),
        )
        .unwrap();
        let items = result["ResultItems"].as_array().unwrap();
        assert!(!items.is_empty());
        assert_eq!(items[0]["DocumentId"], "doc2");
    }

    #[test]
    fn test_submit_feedback_succeeds() {
        let state = test_state();
        let result = submit_feedback(&state, &json!({"IndexId": "test-idx", "QueryId": "q-123"}));
        assert!(result.is_ok());
    }

    #[test]
    fn test_query_missing_index() {
        let state = test_state();
        let result = query(
            &state,
            &json!({"IndexId": "nonexistent", "QueryText": "test"}),
        );
        assert!(result.is_err());
    }
}
