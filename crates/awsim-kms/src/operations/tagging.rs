use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::error;
use crate::state::KmsState;
use crate::operations::keys::resolve_key_id;

// ---------------------------------------------------------------------------
// TagResource
// ---------------------------------------------------------------------------

pub fn tag_resource(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let tags = input["Tags"]
        .as_array()
        .ok_or_else(|| error::missing_parameter("Tags"))?;

    let resolved_id = resolve_key_id(state, key_id_input)?;
    let mut key = state
        .keys
        .get_mut(&resolved_id)
        .ok_or_else(|| error::not_found("Key"))?;

    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&resolved_id));
    }

    for tag in tags {
        let k = tag["TagKey"]
            .as_str()
            .ok_or_else(|| error::missing_parameter("TagKey"))?
            .to_string();
        let v = tag["TagValue"]
            .as_str()
            .ok_or_else(|| error::missing_parameter("TagValue"))?
            .to_string();
        key.tags.insert(k, v);
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UntagResource
// ---------------------------------------------------------------------------

pub fn untag_resource(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let tag_keys = input["TagKeys"]
        .as_array()
        .ok_or_else(|| error::missing_parameter("TagKeys"))?;

    let resolved_id = resolve_key_id(state, key_id_input)?;
    let mut key = state
        .keys
        .get_mut(&resolved_id)
        .ok_or_else(|| error::not_found("Key"))?;

    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&resolved_id));
    }

    for tag_key in tag_keys {
        if let Some(k) = tag_key.as_str() {
            key.tags.remove(k);
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListResourceTags
// ---------------------------------------------------------------------------

pub fn list_resource_tags(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let resolved_id = resolve_key_id(state, key_id_input)?;
    let key = state
        .keys
        .get(&resolved_id)
        .ok_or_else(|| error::not_found("Key"))?;

    let tags: Vec<Value> = key
        .tags
        .iter()
        .map(|(k, v)| json!({ "TagKey": k, "TagValue": v }))
        .collect();

    Ok(json!({ "Tags": tags, "Truncated": false }))
}
