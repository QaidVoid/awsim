use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::models::{FOUNDATION_MODELS, model_to_json};
use crate::state::{BedrockState, CustomizationJob, Guardrail, now_iso};

// ── Foundation Models ─────────────────────────────────────────────────────────

pub fn list_foundation_models(_state: &BedrockState, _input: &Value) -> Result<Value, AwsError> {
    let models: Vec<Value> = FOUNDATION_MODELS.iter().map(model_to_json).collect();
    Ok(json!({ "modelSummaries": models }))
}

pub fn get_foundation_model(_state: &BedrockState, input: &Value) -> Result<Value, AwsError> {
    let model_id = input["modelIdentifier"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "modelIdentifier is required"))?;

    let model = FOUNDATION_MODELS
        .iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Model {} not found", model_id),
            )
        })?;

    Ok(json!({ "modelDetails": model_to_json(model) }))
}

// ── Model Customization Jobs ───────────────────────────────────────────────────

pub fn create_model_customization_job(
    state: &BedrockState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let job_name = input["jobName"]
        .as_str()
        .unwrap_or("customization-job");
    let base_model = input["baseModelIdentifier"]
        .as_str()
        .unwrap_or("anthropic.claude-v2:1");
    let custom_model_name = input["customModelName"]
        .as_str()
        .unwrap_or("custom-model");

    let job_id = Uuid::new_v4().to_string();
    let job_arn = format!(
        "arn:aws:bedrock:{}:{}:model-customization-job/{}",
        ctx.region, ctx.account_id, job_id
    );

    let job = CustomizationJob {
        job_arn: job_arn.clone(),
        base_model_identifier: base_model.to_string(),
        custom_model_name: custom_model_name.to_string(),
        status: "InProgress".to_string(),
        creation_time: now_iso(),
    };

    info!(job_id = %job_id, name = %job_name, "Created model customization job (stub)");
    state.customization_jobs.insert(job_id, job);

    Ok(json!({ "jobArn": job_arn }))
}

pub fn list_model_customization_jobs(
    state: &BedrockState,
    _input: &Value,
) -> Result<Value, AwsError> {
    let jobs: Vec<Value> = state
        .customization_jobs
        .iter()
        .map(|e| {
            let j = e.value();
            json!({
                "jobArn": j.job_arn,
                "baseModelArn": j.base_model_identifier,
                "customModelName": j.custom_model_name,
                "status": j.status,
                "creationTime": j.creation_time,
            })
        })
        .collect();
    Ok(json!({ "modelCustomizationJobSummaries": jobs }))
}

// ── Guardrails ─────────────────────────────────────────────────────────────────

pub fn create_guardrail(
    state: &BedrockState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "name is required"))?;
    let blocked_input = input["blockedInputMessaging"]
        .as_str()
        .unwrap_or("This input is blocked.")
        .to_string();
    let blocked_output = input["blockedOutputsMessaging"]
        .as_str()
        .unwrap_or("This output is blocked.")
        .to_string();

    let guardrail_id = Uuid::new_v4().to_string().replace('-', "")[..12].to_string();
    let arn = format!(
        "arn:aws:bedrock:{}:{}:guardrail/{}",
        ctx.region, ctx.account_id, guardrail_id
    );

    let guardrail = Guardrail {
        guardrail_id: guardrail_id.clone(),
        name: name.to_string(),
        arn: arn.clone(),
        blocked_input_messaging: blocked_input,
        blocked_outputs_messaging: blocked_output,
        status: "READY".to_string(),
        created_at: now_iso(),
        version: "DRAFT".to_string(),
    };

    info!(guardrail_id = %guardrail_id, name = %name, "Created guardrail");
    state.guardrails.insert(guardrail_id.clone(), guardrail);

    Ok(json!({
        "guardrailId": guardrail_id,
        "guardrailArn": arn,
        "version": "DRAFT",
        "createdAt": now_iso(),
    }))
}

pub fn get_guardrail(state: &BedrockState, input: &Value) -> Result<Value, AwsError> {
    let guardrail_id = input["guardrailIdentifier"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("MissingParameter", "guardrailIdentifier is required")
        })?;

    let g = state.guardrails.get(guardrail_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Guardrail {} not found", guardrail_id),
        )
    })?;

    Ok(json!({
        "guardrailId": g.guardrail_id,
        "name": g.name,
        "guardrailArn": g.arn,
        "blockedInputMessaging": g.blocked_input_messaging,
        "blockedOutputsMessaging": g.blocked_outputs_messaging,
        "status": g.status,
        "createdAt": g.created_at,
        "version": g.version,
    }))
}

pub fn list_guardrails(state: &BedrockState, _input: &Value) -> Result<Value, AwsError> {
    let guardrails: Vec<Value> = state
        .guardrails
        .iter()
        .map(|e| {
            let g = e.value();
            json!({
                "guardrailId": g.guardrail_id,
                "name": g.name,
                "guardrailArn": g.arn,
                "status": g.status,
                "createdAt": g.created_at,
                "version": g.version,
            })
        })
        .collect();
    Ok(json!({ "guardrails": guardrails }))
}

pub fn delete_guardrail(state: &BedrockState, input: &Value) -> Result<Value, AwsError> {
    let guardrail_id = input["guardrailIdentifier"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("MissingParameter", "guardrailIdentifier is required")
        })?;

    if state.guardrails.remove(guardrail_id).is_none() {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Guardrail {} not found", guardrail_id),
        ));
    }

    Ok(json!({}))
}
