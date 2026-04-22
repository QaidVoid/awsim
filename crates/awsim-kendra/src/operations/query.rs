use awsim_core::AwsError;
use serde_json::{json, Value};

use crate::state::KendraState;

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

    let index = state
        .indexes
        .get(index_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Index {index_id} not found")))?;

    let query_lower = query_text.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

    let mut results: Vec<Value> = Vec::new();

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

        if score > 0.0 {
            // Extract a relevant snippet
            let snippet = extract_snippet(&doc.content, &query_terms, 200);

            results.push(json!({
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
                    "ScoreConfidence": if score > 0.5 { "VERY_HIGH" } else { "MEDIUM" },
                },
                "RelevanceScore": score.min(1.0),
            }));
        }
    }

    // Sort by score descending
    results.sort_by(|a, b| {
        let sa = a["RelevanceScore"].as_f64().unwrap_or(0.0);
        let sb = b["RelevanceScore"].as_f64().unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    results.truncate(page_size);

    Ok(json!({
        "QueryId": uuid::Uuid::new_v4().to_string(),
        "ResultItems": results,
        "TotalNumberOfResults": results.len(),
        "FacetResults": [],
    }))
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

    let index = state
        .indexes
        .get(index_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Index {index_id} not found")))?;

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
        content[start..].find(' ').map(|p| start + p + 1).unwrap_or(start)
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
        let result = query(&state, &json!({"IndexId": "test-idx", "QueryText": "Rust programming"})).unwrap();
        let items = result["ResultItems"].as_array().unwrap();
        assert!(!items.is_empty());
        assert_eq!(items[0]["DocumentId"], "doc1");
    }

    #[test]
    fn test_query_no_results() {
        let state = test_state();
        let result = query(&state, &json!({"IndexId": "test-idx", "QueryText": "xyz_nonexistent_term"})).unwrap();
        let items = result["ResultItems"].as_array().unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_retrieve_returns_passages() {
        let state = test_state();
        let result = retrieve(&state, &json!({"IndexId": "test-idx", "QueryText": "Lambda"})).unwrap();
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
        let result = query(&state, &json!({"IndexId": "nonexistent", "QueryText": "test"}));
        assert!(result.is_err());
    }
}
