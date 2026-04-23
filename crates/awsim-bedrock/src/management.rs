use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::models::{FOUNDATION_MODELS, model_to_json};
use crate::state::{BedrockState, CustomizationJob, Guardrail, LoggingConfig, now_iso};

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

// ── Provisioned Model Throughputs ─────────────────────────────────────────────

pub fn list_provisioned_model_throughputs(
    _state: &BedrockState,
    _input: &Value,
) -> Result<Value, AwsError> {
    Ok(json!({ "provisionedModelSummaries": [] }))
}

// ── Model Invocation Logging ──────────────────────────────────────────────────

pub fn get_model_invocation_logging_configuration(
    state: &BedrockState,
    _input: &Value,
) -> Result<Value, AwsError> {
    let config = state
        .logging_config
        .get("default")
        .map(|c| {
            json!({
                "loggingConfig": {
                    "cloudWatchConfig": c.cloud_watch_config,
                    "s3Config": c.s3_config,
                    "embeddingDataDeliveryEnabled": c.embedding_data_delivery_enabled,
                    "imageDataDeliveryEnabled": c.image_data_delivery_enabled,
                    "textDataDeliveryEnabled": c.text_data_delivery_enabled,
                }
            })
        })
        .unwrap_or_else(|| {
            json!({
                "loggingConfig": {
                    "cloudWatchConfig": null,
                    "s3Config": null,
                    "embeddingDataDeliveryEnabled": false,
                    "imageDataDeliveryEnabled": false,
                    "textDataDeliveryEnabled": false,
                }
            })
        });

    Ok(config)
}

pub fn put_model_invocation_logging_configuration(
    state: &BedrockState,
    input: &Value,
) -> Result<Value, AwsError> {
    let lc = &input["loggingConfig"];

    let config = LoggingConfig {
        cloud_watch_config: lc.get("cloudWatchConfig").cloned(),
        s3_config: lc.get("s3Config").cloned(),
        embedding_data_delivery_enabled: lc["embeddingDataDeliveryEnabled"]
            .as_bool()
            .unwrap_or(false),
        image_data_delivery_enabled: lc["imageDataDeliveryEnabled"].as_bool().unwrap_or(false),
        text_data_delivery_enabled: lc["textDataDeliveryEnabled"].as_bool().unwrap_or(false),
    };

    info!("Stored Bedrock model invocation logging config");
    state.logging_config.insert("default".to_string(), config);

    Ok(json!({}))
}

// ── Custom Models ─────────────────────────────────────────────────────────────

pub fn list_custom_models(_state: &BedrockState, _input: &Value) -> Result<Value, AwsError> {
    Ok(json!({ "modelSummaries": [] }))
}

pub fn get_model_customization_job(
    state: &BedrockState,
    input: &Value,
) -> Result<Value, AwsError> {
    let job_identifier = input["jobIdentifier"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "jobIdentifier is required"))?;

    // Look up by job_arn or job_id
    let job = state
        .customization_jobs
        .iter()
        .find(|e| {
            e.key() == job_identifier || e.value().job_arn == job_identifier
        });

    match job {
        Some(j) => {
            let j = j.value().clone();
            Ok(json!({
                "jobArn": j.job_arn,
                "baseModelArn": j.base_model_identifier,
                "customModelName": j.custom_model_name,
                "status": j.status,
                "creationTime": j.creation_time,
            }))
        }
        None => Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Customization job not found: {job_identifier}"),
        )),
    }
}

pub fn stop_model_customization_job(
    state: &BedrockState,
    input: &Value,
) -> Result<Value, AwsError> {
    let job_identifier = input["jobIdentifier"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "jobIdentifier is required"))?;

    let found = state.customization_jobs.iter().find(|e| {
        e.key() == job_identifier || e.value().job_arn == job_identifier
    });

    if let Some(entry) = found {
        let key = entry.key().clone();
        drop(entry);
        if let Some(mut job) = state.customization_jobs.get_mut(&key) {
            job.status = "Stopped".to_string();
        }
        Ok(json!({}))
    } else {
        Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Customization job not found: {job_identifier}"),
        ))
    }
}

// ── Resource Tags ─────────────────────────────────────────────────────────────

pub fn tag_resource(state: &BedrockState, input: &Value) -> Result<Value, AwsError> {
    let resource_arn = input["resourceARN"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "resourceARN is required"))?;

    let new_tags = input["tags"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "tags is required"))?;

    let mut entry = state.tags.entry(resource_arn.to_string()).or_default();
    for tag in new_tags {
        if let (Some(k), Some(v)) = (tag["key"].as_str(), tag["value"].as_str()) {
            entry.insert(k.to_string(), v.to_string());
        }
    }

    Ok(json!({}))
}

pub fn untag_resource(state: &BedrockState, input: &Value) -> Result<Value, AwsError> {
    let resource_arn = input["resourceARN"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "resourceARN is required"))?;

    let tag_keys = input["tagKeys"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "tagKeys is required"))?;

    if let Some(mut entry) = state.tags.get_mut(resource_arn) {
        for key_val in tag_keys {
            if let Some(k) = key_val.as_str() {
                entry.remove(k);
            }
        }
    }

    Ok(json!({}))
}

pub fn list_tags_for_resource(state: &BedrockState, input: &Value) -> Result<Value, AwsError> {
    let resource_arn = input["resourceARN"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "resourceARN is required"))?;

    let tags: Vec<Value> = state
        .tags
        .get(resource_arn)
        .map(|entry| {
            entry
                .iter()
                .map(|(k, v)| json!({ "key": k, "value": v }))
                .collect()
        })
        .unwrap_or_default();

    Ok(json!({ "tags": tags }))
}
