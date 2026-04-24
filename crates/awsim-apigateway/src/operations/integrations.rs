use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{ApiGatewayState, Integration};

pub fn create_integration(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["ApiId"].as_str().ok_or_else(|| {
        AwsError::bad_request("BadRequestException", "Missing required field: ApiId")
    })?;

    let integration_type = input["IntegrationType"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request(
                "BadRequestException",
                "Missing required field: IntegrationType",
            )
        })?
        .to_string();

    let integration_uri = input["IntegrationUri"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request(
                "BadRequestException",
                "Missing required field: IntegrationUri",
            )
        })?
        .to_string();

    let payload_format_version = input["PayloadFormatVersion"]
        .as_str()
        .unwrap_or("2.0")
        .to_string();

    let integration_method = input["IntegrationMethod"].as_str().map(|s| s.to_string());
    let description = input["Description"].as_str().map(|s| s.to_string());
    let timeout_in_millis = input["TimeoutInMillis"].as_u64().unwrap_or(29000) as u32;

    let integration_id = format!("i{}", &Uuid::new_v4().to_string().replace('-', "")[..8]);

    let integration = Integration {
        integration_id: integration_id.clone(),
        integration_type: integration_type.clone(),
        integration_uri: integration_uri.clone(),
        payload_format_version: payload_format_version.clone(),
        integration_method: integration_method.clone(),
        description: description.clone(),
        timeout_in_millis,
    };

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("API with ID {api_id} not found"),
        )
    })?;

    api.integrations.insert(integration_id.clone(), integration);

    Ok(json!({
        "IntegrationId": integration_id,
        "IntegrationType": integration_type,
        "IntegrationUri": integration_uri,
        "PayloadFormatVersion": payload_format_version,
        "IntegrationMethod": integration_method,
        "Description": description,
        "TimeoutInMillis": timeout_in_millis,
    }))
}

pub fn get_integration(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["ApiId"].as_str().ok_or_else(|| {
        AwsError::bad_request("BadRequestException", "Missing required field: ApiId")
    })?;

    let integration_id = input["IntegrationId"].as_str().ok_or_else(|| {
        AwsError::bad_request(
            "BadRequestException",
            "Missing required field: IntegrationId",
        )
    })?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("API with ID {api_id} not found"),
        )
    })?;

    let integration = api.integrations.get(integration_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Integration with ID {integration_id} not found"),
        )
    })?;

    Ok(integration_to_json(integration))
}

pub fn delete_integration(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["ApiId"].as_str().ok_or_else(|| {
        AwsError::bad_request("BadRequestException", "Missing required field: ApiId")
    })?;

    let integration_id = input["IntegrationId"].as_str().ok_or_else(|| {
        AwsError::bad_request(
            "BadRequestException",
            "Missing required field: IntegrationId",
        )
    })?;

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("API with ID {api_id} not found"),
        )
    })?;

    api.integrations.remove(integration_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Integration with ID {integration_id} not found"),
        )
    })?;

    Ok(json!({}))
}

fn integration_to_json(i: &Integration) -> Value {
    json!({
        "IntegrationId": i.integration_id,
        "IntegrationType": i.integration_type,
        "IntegrationUri": i.integration_uri,
        "PayloadFormatVersion": i.payload_format_version,
        "IntegrationMethod": i.integration_method,
        "Description": i.description,
        "TimeoutInMillis": i.timeout_in_millis,
    })
}
