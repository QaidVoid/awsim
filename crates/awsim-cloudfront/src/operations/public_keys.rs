use awsim_core::AwsError;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    ids::{new_etag, now_iso8601},
    state::{CloudFrontState, PublicKey},
};

fn not_found(id: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchPublicKey",
        format!("The specified public key does not exist: {id}"),
    )
}

fn pk_to_value(p: &PublicKey) -> Value {
    json!({
        "Id": p.id,
        "CreatedTime": p.created_at,
        "PublicKeyConfig": {
            "CallerReference": p.caller_reference,
            "Name": p.name,
            "EncodedKey": p.encoded_key,
            "Comment": p.comment,
        }
    })
}

pub fn create_public_key(state: &CloudFrontState, input: &Value) -> Result<Value, AwsError> {
    let cfg = input.get("PublicKeyConfig").unwrap_or(input);
    let name = cfg.get("Name").and_then(|v| v.as_str()).unwrap_or("default").to_string();
    let caller_reference = cfg
        .get("CallerReference")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let encoded_key = cfg
        .get("EncodedKey")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let comment = cfg.get("Comment").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let id = Uuid::new_v4().to_string();
    let etag = new_etag();
    let pk = PublicKey {
        id: id.clone(),
        name,
        encoded_key,
        caller_reference,
        comment,
        created_at: now_iso8601(),
        etag: etag.clone(),
    };

    let result = pk_to_value(&pk);
    state.public_keys.insert(id, pk);

    Ok(json!({ "PublicKey": result, "ETag": etag }))
}

pub fn get_public_key(state: &CloudFrontState, id: &str) -> Result<Value, AwsError> {
    let pk = state.public_keys.get(id).ok_or_else(|| not_found(id))?;
    let etag = pk.etag.clone();
    let result = pk_to_value(&pk);
    Ok(json!({ "PublicKey": result, "ETag": etag }))
}

pub fn delete_public_key(state: &CloudFrontState, id: &str) -> Result<Value, AwsError> {
    if state.public_keys.remove(id).is_none() {
        return Err(not_found(id));
    }
    Ok(json!({}))
}

pub fn list_public_keys(state: &CloudFrontState) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .public_keys
        .iter()
        .map(|e| pk_to_value(e.value()))
        .collect();
    let qty = items.len();
    Ok(json!({
        "PublicKeyList": {
            "MaxItems": 100,
            "Quantity": qty,
            "Items": { "PublicKeySummary": items }
        }
    }))
}
