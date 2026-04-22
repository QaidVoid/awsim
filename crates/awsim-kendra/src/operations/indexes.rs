use awsim_core::{AwsError, RequestContext};
use serde_json::{json, Value};

use crate::state::{DataSource, KendraIndex, KendraState};

pub fn create_index(
    state: &KendraState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::validation("Name is required"))?;
    let role_arn = input["RoleArn"]
        .as_str()
        .unwrap_or("arn:aws:iam::000000000000:role/KendraRole");
    let description = input["Description"].as_str().unwrap_or("");
    let edition = input["Edition"].as_str().unwrap_or("DEVELOPER_EDITION");

    let id = uuid::Uuid::new_v4().to_string();
    let arn = format!(
        "arn:aws:kendra:{}:{}:index/{}",
        ctx.region, ctx.account_id, id
    );
    let now = crate::util::now_iso8601();

    let index = KendraIndex {
        id: id.clone(),
        name: name.to_string(),
        arn: arn.clone(),
        description: description.to_string(),
        role_arn: role_arn.to_string(),
        edition: edition.to_string(),
        status: "ACTIVE".to_string(),
        created_at: now.clone(),
        updated_at: now,
        documents: Vec::new(),
        data_sources: Default::default(),
        faqs: Default::default(),
    };

    state.indexes.insert(id.clone(), index);

    Ok(json!({ "Id": id }))
}

pub fn describe_index(state: &KendraState, input: &Value) -> Result<Value, AwsError> {
    let id = input["Id"]
        .as_str()
        .ok_or_else(|| AwsError::validation("Id is required"))?;

    let index = state
        .indexes
        .get(id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Index {id} not found")))?;

    Ok(json!({
        "Id": index.id,
        "Name": index.name,
        "Arn": index.arn,
        "Description": index.description,
        "RoleArn": index.role_arn,
        "Edition": index.edition,
        "Status": index.status,
        "CreatedAt": index.created_at,
        "UpdatedAt": index.updated_at,
        "DocumentMetadataConfigurations": [],
    }))
}

pub fn list_indices(state: &KendraState) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .indexes
        .iter()
        .map(|entry| {
            let idx = entry.value();
            json!({
                "Id": idx.id,
                "Name": idx.name,
                "Edition": idx.edition,
                "Status": idx.status,
                "CreatedAt": idx.created_at,
                "UpdatedAt": idx.updated_at,
            })
        })
        .collect();

    Ok(json!({ "IndexConfigurationSummaryItems": items }))
}

pub fn delete_index(state: &KendraState, input: &Value) -> Result<Value, AwsError> {
    let id = input["Id"]
        .as_str()
        .ok_or_else(|| AwsError::validation("Id is required"))?;

    state.indexes.remove(id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("Index {id} not found"))
    })?;

    Ok(json!({}))
}

pub fn update_index(state: &KendraState, input: &Value) -> Result<Value, AwsError> {
    let id = input["Id"]
        .as_str()
        .ok_or_else(|| AwsError::validation("Id is required"))?;

    let mut index = state
        .indexes
        .get_mut(id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Index {id} not found")))?;

    if let Some(name) = input["Name"].as_str() {
        index.name = name.to_string();
    }
    if let Some(desc) = input["Description"].as_str() {
        index.description = desc.to_string();
    }
    if let Some(role) = input["RoleArn"].as_str() {
        index.role_arn = role.to_string();
    }
    index.updated_at = crate::util::now_iso8601();

    Ok(json!({}))
}

pub fn create_data_source(
    state: &KendraState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let index_id = input["IndexId"]
        .as_str()
        .ok_or_else(|| AwsError::validation("IndexId is required"))?;
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::validation("Name is required"))?;
    let ds_type = input["Type"].as_str().unwrap_or("CUSTOM");
    let role_arn = input["RoleArn"].as_str().unwrap_or("");
    let configuration = input.get("Configuration").cloned().unwrap_or(json!({}));

    let mut index = state
        .indexes
        .get_mut(index_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Index {index_id} not found")))?;

    let ds_id = uuid::Uuid::new_v4().to_string();

    index.data_sources.insert(
        ds_id.clone(),
        DataSource {
            id: ds_id.clone(),
            name: name.to_string(),
            ds_type: ds_type.to_string(),
            configuration,
            role_arn: role_arn.to_string(),
            status: "ACTIVE".to_string(),
            created_at: crate::util::now_iso8601(),
        },
    );

    Ok(json!({ "Id": ds_id }))
}

pub fn list_data_sources(state: &KendraState, input: &Value) -> Result<Value, AwsError> {
    let index_id = input["IndexId"]
        .as_str()
        .ok_or_else(|| AwsError::validation("IndexId is required"))?;

    let index = state
        .indexes
        .get(index_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Index {index_id} not found")))?;

    let items: Vec<Value> = index
        .data_sources
        .values()
        .map(|ds| {
            json!({
                "Id": ds.id,
                "Name": ds.name,
                "Type": ds.ds_type,
                "Status": ds.status,
                "CreatedAt": ds.created_at,
            })
        })
        .collect();

    Ok(json!({ "SummaryItems": items }))
}

pub fn delete_data_source(state: &KendraState, input: &Value) -> Result<Value, AwsError> {
    let index_id = input["IndexId"]
        .as_str()
        .ok_or_else(|| AwsError::validation("IndexId is required"))?;
    let ds_id = input["Id"]
        .as_str()
        .ok_or_else(|| AwsError::validation("Id is required"))?;

    let mut index = state
        .indexes
        .get_mut(index_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Index {index_id} not found")))?;

    index.data_sources.remove(ds_id);

    Ok(json!({}))
}
