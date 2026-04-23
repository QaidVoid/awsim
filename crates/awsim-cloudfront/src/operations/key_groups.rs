use awsim_core::AwsError;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    ids::{new_etag, now_iso8601},
    state::{CloudFrontState, KeyGroup},
};

fn not_found(id: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchResource",
        format!("The specified key group does not exist: {id}"),
    )
}

fn extract_items(items: Option<&Value>) -> Vec<String> {
    let Some(items) = items else { return vec![]; };
    let inner = items.get("PublicKey").or_else(|| items.get("Items")).unwrap_or(items);
    match inner {
        Value::Array(arr) => arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect(),
        Value::String(s) => vec![s.clone()],
        _ => vec![],
    }
}

fn kg_to_value(kg: &KeyGroup) -> Value {
    let items: Vec<Value> = kg.items.iter().map(|s| Value::String(s.clone())).collect();
    let qty = items.len();
    json!({
        "Id": kg.id,
        "LastModifiedTime": kg.created_at,
        "KeyGroupConfig": {
            "Name": kg.name,
            "Items": { "Quantity": qty, "Items": { "PublicKey": items } },
            "Comment": kg.comment,
        }
    })
}

pub fn create_key_group(state: &CloudFrontState, input: &Value) -> Result<Value, AwsError> {
    let cfg = input.get("KeyGroupConfig").unwrap_or(input);
    let name = cfg.get("Name").and_then(|v| v.as_str()).unwrap_or("default").to_string();
    let comment = cfg.get("Comment").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let items = extract_items(cfg.get("Items"));

    let id = Uuid::new_v4().to_string();
    let etag = new_etag();

    let kg = KeyGroup {
        id: id.clone(),
        name,
        items,
        comment,
        created_at: now_iso8601(),
        etag: etag.clone(),
    };

    let result = kg_to_value(&kg);
    state.key_groups.insert(id, kg);

    Ok(json!({ "KeyGroup": result, "ETag": etag }))
}

pub fn get_key_group(state: &CloudFrontState, id: &str) -> Result<Value, AwsError> {
    let kg = state.key_groups.get(id).ok_or_else(|| not_found(id))?;
    let etag = kg.etag.clone();
    let result = kg_to_value(&kg);
    Ok(json!({ "KeyGroup": result, "ETag": etag }))
}

pub fn delete_key_group(state: &CloudFrontState, id: &str) -> Result<Value, AwsError> {
    if state.key_groups.remove(id).is_none() {
        return Err(not_found(id));
    }
    Ok(json!({}))
}

pub fn list_key_groups(state: &CloudFrontState) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .key_groups
        .iter()
        .map(|e| json!({ "KeyGroup": kg_to_value(e.value()) }))
        .collect();
    let qty = items.len();
    Ok(json!({
        "KeyGroupList": {
            "MaxItems": 100,
            "Quantity": qty,
            "Items": { "KeyGroupSummary": items }
        }
    }))
}
