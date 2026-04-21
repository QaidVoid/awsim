use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{ApiGatewayState, Deployment};
use crate::util::now_iso8601;

pub fn create_deployment(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["ApiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Missing required field: ApiId"))?;

    let description = input["Description"].as_str().map(|s| s.to_string());

    let deployment_id = Uuid::new_v4().to_string().replace('-', "")[..10].to_string();
    let created_date = now_iso8601();

    let deployment = Deployment {
        deployment_id: deployment_id.clone(),
        deployment_status: "DEPLOYED".to_string(),
        created_date: created_date.clone(),
        description: description.clone(),
    };

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("API with ID {api_id} not found"))
    })?;

    api.deployments.insert(deployment_id.clone(), deployment);

    Ok(json!({
        "DeploymentId": deployment_id,
        "DeploymentStatus": "DEPLOYED",
        "CreatedDate": created_date,
        "Description": description,
    }))
}

pub fn get_deployment(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["ApiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Missing required field: ApiId"))?;

    let deployment_id = input["DeploymentId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Missing required field: DeploymentId"))?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("API with ID {api_id} not found"))
    })?;

    let deployment = api.deployments.get(deployment_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Deployment with ID {deployment_id} not found"),
        )
    })?;

    Ok(deployment_to_json(deployment))
}

fn deployment_to_json(d: &Deployment) -> Value {
    json!({
        "DeploymentId": d.deployment_id,
        "DeploymentStatus": d.deployment_status,
        "CreatedDate": d.created_date,
        "Description": d.description,
    })
}
