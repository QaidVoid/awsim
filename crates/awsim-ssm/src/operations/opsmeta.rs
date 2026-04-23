use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{SsmOpsMetadata, SsmState};

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn build_arn(ctx: &RequestContext, resource_id: &str) -> String {
    format!(
        "arn:aws:ssm:{}:{}:opsmetadata/{}",
        ctx.region,
        ctx.account_id,
        resource_id.trim_start_matches('/')
    )
}

pub fn create_ops_metadata(
    state: &SsmState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_id = input["ResourceId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceId is required"))?
        .to_string();

    let arn = build_arn(ctx, &resource_id);
    if state.ops_metadata.contains_key(&arn) {
        return Err(AwsError::conflict(
            "OpsMetadataAlreadyExistsException",
            format!("OpsMetadata for '{resource_id}' already exists"),
        ));
    }

    let metadata = input["Metadata"].clone();
    let now = now_epoch_secs();

    let om = SsmOpsMetadata {
        ops_metadata_arn: arn.clone(),
        resource_id,
        metadata,
        creation_date: now,
        last_modified_date: now,
        last_modified_user: "awsim".to_string(),
    };

    state.ops_metadata.insert(arn.clone(), om);

    Ok(json!({ "OpsMetadataArn": arn }))
}

pub fn get_ops_metadata(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["OpsMetadataArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "OpsMetadataArn is required"))?;

    let om = state.ops_metadata.get(arn).ok_or_else(|| {
        AwsError::not_found(
            "OpsMetadataNotFoundException",
            format!("OpsMetadata '{arn}' not found"),
        )
    })?;

    Ok(json!({
        "ResourceId": om.resource_id,
        "Metadata": om.metadata,
    }))
}

pub fn update_ops_metadata(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["OpsMetadataArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "OpsMetadataArn is required"))?;

    let mut om = state.ops_metadata.get_mut(arn).ok_or_else(|| {
        AwsError::not_found(
            "OpsMetadataNotFoundException",
            format!("OpsMetadata '{arn}' not found"),
        )
    })?;

    if let Some(upd) = input.get("MetadataToUpdate") {
        om.metadata = upd.clone();
    }
    om.last_modified_date = now_epoch_secs();

    Ok(json!({ "OpsMetadataArn": arn }))
}

pub fn delete_ops_metadata(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["OpsMetadataArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "OpsMetadataArn is required"))?;

    if state.ops_metadata.remove(arn).is_none() {
        return Err(AwsError::not_found(
            "OpsMetadataNotFoundException",
            format!("OpsMetadata '{arn}' not found"),
        ));
    }

    Ok(json!({}))
}

pub fn list_ops_metadata(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let max_results = input["MaxResults"].as_u64().unwrap_or(50) as usize;

    let items: Vec<Value> = state
        .ops_metadata
        .iter()
        .map(|e| {
            let om = e.value();
            json!({
                "OpsMetadataArn": om.ops_metadata_arn,
                "ResourceId": om.resource_id,
                "CreationDate": om.creation_date,
                "LastModifiedDate": om.last_modified_date,
                "LastModifiedUser": om.last_modified_user,
            })
        })
        .take(max_results)
        .collect();

    Ok(json!({ "OpsMetadataList": items }))
}

pub fn get_ops_summary(
    state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let entities: Vec<Value> = state
        .ops_items
        .iter()
        .map(|e| {
            let i = e.value();
            json!({
                "Id": i.ops_item_id,
                "Data": {
                    "AWS:OpsItem": {
                        "CaptureTime": i.created_time.to_string(),
                        "Content": [{
                            "OpsItemId": i.ops_item_id,
                            "Title": i.title,
                            "Status": i.status,
                            "Severity": i.severity,
                        }]
                    }
                }
            })
        })
        .collect();

    Ok(json!({ "Entities": entities }))
}

pub fn delete_ops_item(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ops_item_id = input["OpsItemId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "OpsItemId is required"))?;

    if state.ops_items.remove(ops_item_id).is_none() {
        return Err(AwsError::not_found(
            "OpsItemNotFoundException",
            format!("OpsItem '{ops_item_id}' not found"),
        ));
    }

    Ok(json!({}))
}

pub fn list_ops_item_events(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "Summaries": [] }))
}

pub fn list_ops_item_related_items(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "Summaries": [] }))
}

pub fn associate_ops_item_related_item(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "AssociationId": uuid::Uuid::new_v4().to_string() }))
}

pub fn disassociate_ops_item_related_item(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({}))
}
