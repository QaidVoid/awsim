use awsim_core::AwsError;
use serde_json::{json, Value};

use crate::state::{IndexedDocument, KendraState};

/// BatchPutDocument — add documents to a Kendra index.
pub fn batch_put_document(state: &KendraState, input: &Value) -> Result<Value, AwsError> {
    let index_id = input["IndexId"]
        .as_str()
        .ok_or_else(|| AwsError::validation("IndexId is required"))?;
    let documents = input["Documents"]
        .as_array()
        .ok_or_else(|| AwsError::validation("Documents is required"))?;

    let mut index = state
        .indexes
        .get_mut(index_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Index {index_id} not found")))?;

    let mut failed: Vec<Value> = Vec::new();

    for doc in documents {
        let doc_id = match doc["Id"].as_str() {
            Some(id) => id.to_string(),
            None => {
                failed.push(json!({
                    "Id": doc["Id"],
                    "ErrorCode": "InvalidDocument",
                    "ErrorMessage": "Document Id is required",
                }));
                continue;
            }
        };

        let title = doc["Title"].as_str().map(String::from);

        // Content can come from Blob (base64), S3Path, or inline
        let content = doc["Blob"]
            .as_str()
            .map(|b| {
                // Decode base64
                String::from_utf8(
                    base64_decode(b).unwrap_or_default()
                ).unwrap_or_default()
            })
            .or_else(|| doc["Content"].as_str().map(String::from))
            .unwrap_or_default();

        let content_type = doc["ContentType"]
            .as_str()
            .unwrap_or("PLAIN_TEXT")
            .to_string();

        // Remove existing document with same ID
        index.documents.retain(|d| d.id != doc_id);

        index.documents.push(IndexedDocument {
            id: doc_id,
            title,
            content,
            content_type,
            attributes: Default::default(),
            created_at: crate::util::now_iso8601(),
        });
    }

    Ok(json!({
        "FailedDocuments": failed,
    }))
}

/// BatchDeleteDocument — remove documents from a Kendra index.
pub fn batch_delete_document(state: &KendraState, input: &Value) -> Result<Value, AwsError> {
    let index_id = input["IndexId"]
        .as_str()
        .ok_or_else(|| AwsError::validation("IndexId is required"))?;
    let doc_ids = input["DocumentIdList"]
        .as_array()
        .ok_or_else(|| AwsError::validation("DocumentIdList is required"))?;

    let mut index = state
        .indexes
        .get_mut(index_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Index {index_id} not found")))?;

    let ids_to_remove: Vec<String> = doc_ids
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    index.documents.retain(|d| !ids_to_remove.contains(&d.id));

    Ok(json!({
        "FailedDocuments": [],
    }))
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    // Simple base64 decoder
    let chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;

    for c in input.chars() {
        if c == '=' {
            break;
        }
        if let Some(val) = chars.find(c) {
            buffer = (buffer << 6) | val as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                result.push((buffer >> bits) as u8);
                buffer &= (1 << bits) - 1;
            }
        }
    }

    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::KendraIndex;

    fn empty_index() -> KendraState {
        let state = KendraState::default();
        state.indexes.insert(
            "idx-1".to_string(),
            KendraIndex {
                id: "idx-1".to_string(),
                name: "Test".to_string(),
                arn: String::new(),
                description: String::new(),
                role_arn: String::new(),
                edition: "DEVELOPER_EDITION".to_string(),
                status: "ACTIVE".to_string(),
                created_at: String::new(),
                updated_at: String::new(),
                documents: Vec::new(),
                data_sources: Default::default(),
                faqs: Default::default(),
            },
        );
        state
    }

    #[test]
    fn test_batch_put_and_delete() {
        let state = empty_index();

        // Put
        let result = batch_put_document(&state, &json!({
            "IndexId": "idx-1",
            "Documents": [
                {"Id": "d1", "Title": "Doc One", "Content": "Hello world"},
                {"Id": "d2", "Title": "Doc Two", "Content": "Goodbye world"},
            ]
        })).unwrap();
        assert_eq!(result["FailedDocuments"].as_array().unwrap().len(), 0);

        // Verify
        let idx = state.indexes.get("idx-1").unwrap();
        assert_eq!(idx.documents.len(), 2);
        drop(idx);

        // Delete
        let result = batch_delete_document(&state, &json!({
            "IndexId": "idx-1",
            "DocumentIdList": ["d1"]
        })).unwrap();
        assert_eq!(result["FailedDocuments"].as_array().unwrap().len(), 0);

        let idx = state.indexes.get("idx-1").unwrap();
        assert_eq!(idx.documents.len(), 1);
        assert_eq!(idx.documents[0].id, "d2");
    }
}
