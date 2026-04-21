use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{ApiGatewayState, RouteSettings, Stage};
use crate::util::now_iso8601;

pub fn create_stage(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["ApiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Missing required field: ApiId"))?;

    let stage_name = input["StageName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Missing required field: StageName"))?
        .to_string();

    let auto_deploy = input["AutoDeploy"].as_bool().unwrap_or(false);
    let description = input["Description"].as_str().unwrap_or("").to_string();
    let deployment_id = input["DeploymentId"].as_str().map(|s| s.to_string());

    let now = now_iso8601();
    let stage = Stage {
        stage_name: stage_name.clone(),
        auto_deploy,
        description: description.clone(),
        deployment_id: deployment_id.clone(),
        created_date: now.clone(),
        last_updated_date: now.clone(),
        default_route_settings: RouteSettings::default(),
    };

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("API with ID {api_id} not found"))
    })?;

    api.stages.insert(stage_name.clone(), stage);

    Ok(json!({
        "StageName": stage_name,
        "AutoDeploy": auto_deploy,
        "Description": description,
        "DeploymentId": deployment_id,
        "CreatedDate": now,
        "LastUpdatedDate": now,
    }))
}

pub fn get_stage(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["ApiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Missing required field: ApiId"))?;

    let stage_name = input["StageName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Missing required field: StageName"))?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("API with ID {api_id} not found"))
    })?;

    let stage = api.stages.get(stage_name).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("Stage {stage_name} not found"))
    })?;

    Ok(stage_to_json(stage))
}

pub fn get_stages(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["ApiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Missing required field: ApiId"))?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("API with ID {api_id} not found"))
    })?;

    let items: Vec<Value> = api.stages.values().map(stage_to_json).collect();

    Ok(json!({ "Items": items }))
}

pub fn delete_stage(
    state: &Arc<ApiGatewayState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["ApiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Missing required field: ApiId"))?;

    let stage_name = input["StageName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Missing required field: StageName"))?;

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("API with ID {api_id} not found"))
    })?;

    api.stages.remove(stage_name).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("Stage {stage_name} not found"))
    })?;

    Ok(json!({}))
}

fn stage_to_json(s: &Stage) -> Value {
    json!({
        "StageName": s.stage_name,
        "AutoDeploy": s.auto_deploy,
        "Description": s.description,
        "DeploymentId": s.deployment_id,
        "CreatedDate": s.created_date,
        "LastUpdatedDate": s.last_updated_date,
    })
}
