use awsim_core::AwsError;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    ids::{new_etag, now_iso8601},
    state::{CloudFrontState, FieldLevelEncryptionConfig},
};

fn not_found(id: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchFieldLevelEncryptionConfig",
        format!("The specified field level encryption config does not exist: {id}"),
    )
}

fn fle_to_value(f: &FieldLevelEncryptionConfig) -> Value {
    json!({
        "Id": f.id,
        "LastModifiedTime": f.created_at,
        "FieldLevelEncryptionConfig": {
            "CallerReference": f.caller_reference,
            "Comment": f.comment,
            "QueryArgProfileConfig": { "ForwardWhenQueryArgProfileIsUnknown": true },
            "ContentTypeProfileConfig": { "ForwardWhenContentTypeIsUnknown": true },
        }
    })
}

pub fn create_field_level_encryption_config(
    state: &CloudFrontState,
    input: &Value,
) -> Result<Value, AwsError> {
    let cfg = input.get("FieldLevelEncryptionConfig").unwrap_or(input);
    let caller_reference = cfg
        .get("CallerReference")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let comment = cfg.get("Comment").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let id = Uuid::new_v4().to_string();
    let etag = new_etag();
    let fle = FieldLevelEncryptionConfig {
        id: id.clone(),
        comment,
        caller_reference,
        created_at: now_iso8601(),
        etag: etag.clone(),
    };

    let result = fle_to_value(&fle);
    state.field_level_encryption_configs.insert(id, fle);

    Ok(json!({ "FieldLevelEncryption": result, "ETag": etag }))
}

pub fn get_field_level_encryption_config(
    state: &CloudFrontState,
    id: &str,
) -> Result<Value, AwsError> {
    let f = state.field_level_encryption_configs.get(id).ok_or_else(|| not_found(id))?;
    let etag = f.etag.clone();
    let result = fle_to_value(&f);
    Ok(json!({ "FieldLevelEncryption": result, "ETag": etag }))
}

pub fn delete_field_level_encryption_config(
    state: &CloudFrontState,
    id: &str,
) -> Result<Value, AwsError> {
    if state.field_level_encryption_configs.remove(id).is_none() {
        return Err(not_found(id));
    }
    Ok(json!({}))
}

pub fn list_field_level_encryption_configs(state: &CloudFrontState) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .field_level_encryption_configs
        .iter()
        .map(|e| fle_to_value(e.value()))
        .collect();
    let qty = items.len();
    Ok(json!({
        "FieldLevelEncryptionList": {
            "MaxItems": 100,
            "Quantity": qty,
            "Items": { "FieldLevelEncryptionSummary": items }
        }
    }))
}
