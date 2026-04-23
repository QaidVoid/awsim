use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::models::{FOUNDATION_MODELS, model_to_json};
use crate::state::{
    BedrockState, CustomModel, CustomizationJob, Guardrail, InvocationJob, KnowledgeBase,
    LoggingConfig, ProvisionedModel, now_iso,
};

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

fn pmt_to_json(p: &ProvisionedModel) -> Value {
    json!({
        "provisionedModelArn": p.provisioned_model_arn,
        "provisionedModelName": p.provisioned_model_name,
        "modelArn": p.model_arn,
        "modelUnits": p.model_units,
        "status": p.status,
        "creationTime": p.creation_time,
    })
}

pub fn create_provisioned_model_throughput(
    state: &BedrockState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["provisionedModelName"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("MissingParameter", "provisionedModelName is required")
        })?;
    let model_id = input["modelId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "modelId is required"))?;
    let model_units = input["modelUnits"].as_i64().unwrap_or(1) as i32;

    let id = Uuid::new_v4().to_string().replace('-', "")[..16].to_string();
    let arn = format!(
        "arn:aws:bedrock:{}:{}:provisioned-model/{}",
        ctx.region, ctx.account_id, id
    );
    let model_arn = format!(
        "arn:aws:bedrock:{}::foundation-model/{}",
        ctx.region, model_id
    );

    let pmt = ProvisionedModel {
        provisioned_model_id: id.clone(),
        provisioned_model_arn: arn.clone(),
        model_arn,
        model_units,
        provisioned_model_name: name.to_string(),
        status: "InService".to_string(),
        creation_time: now_iso(),
    };

    info!(provisioned_model_id = %id, "Created provisioned model throughput");
    state.provisioned_models.insert(id, pmt);

    Ok(json!({ "provisionedModelArn": arn }))
}

pub fn get_provisioned_model_throughput(
    state: &BedrockState,
    input: &Value,
) -> Result<Value, AwsError> {
    let identifier = input["provisionedModelId"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("MissingParameter", "provisionedModelId is required")
        })?;

    let pmt = state
        .provisioned_models
        .iter()
        .find(|e| e.key() == identifier || e.value().provisioned_model_arn == identifier);

    match pmt {
        Some(e) => Ok(pmt_to_json(e.value())),
        None => Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Provisioned model not found: {identifier}"),
        )),
    }
}

pub fn delete_provisioned_model_throughput(
    state: &BedrockState,
    input: &Value,
) -> Result<Value, AwsError> {
    let identifier = input["provisionedModelId"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("MissingParameter", "provisionedModelId is required")
        })?;

    let key = state
        .provisioned_models
        .iter()
        .find(|e| e.key() == identifier || e.value().provisioned_model_arn == identifier)
        .map(|e| e.key().clone());

    match key {
        Some(k) => {
            state.provisioned_models.remove(&k);
            Ok(json!({}))
        }
        None => Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Provisioned model not found: {identifier}"),
        )),
    }
}

pub fn list_provisioned_model_throughputs(
    state: &BedrockState,
    _input: &Value,
) -> Result<Value, AwsError> {
    let summaries: Vec<Value> = state
        .provisioned_models
        .iter()
        .map(|e| pmt_to_json(e.value()))
        .collect();
    Ok(json!({ "provisionedModelSummaries": summaries }))
}

// ── Model Invocation Jobs ─────────────────────────────────────────────────────

fn invocation_job_to_json(j: &InvocationJob) -> Value {
    json!({
        "jobArn": j.job_arn,
        "jobName": j.job_name,
        "modelId": j.model_id,
        "status": j.status,
        "submitTime": j.submit_time,
        "roleArn": j.role_arn,
    })
}

pub fn create_model_invocation_job(
    state: &BedrockState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let job_name = input["jobName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "jobName is required"))?;
    let model_id = input["modelId"]
        .as_str()
        .unwrap_or("anthropic.claude-v2:1")
        .to_string();
    let role_arn = input["roleArn"].as_str().unwrap_or("").to_string();

    let job_id = Uuid::new_v4().to_string();
    let job_arn = format!(
        "arn:aws:bedrock:{}:{}:model-invocation-job/{}",
        ctx.region, ctx.account_id, job_id
    );

    let job = InvocationJob {
        job_arn: job_arn.clone(),
        job_name: job_name.to_string(),
        model_id,
        status: "Submitted".to_string(),
        submit_time: now_iso(),
        role_arn,
    };

    info!(job_id = %job_id, "Created model invocation job");
    state.invocation_jobs.insert(job_id, job);

    Ok(json!({ "jobArn": job_arn }))
}

pub fn get_model_invocation_job(state: &BedrockState, input: &Value) -> Result<Value, AwsError> {
    let identifier = input["jobIdentifier"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "jobIdentifier is required"))?;

    let job = state
        .invocation_jobs
        .iter()
        .find(|e| e.key() == identifier || e.value().job_arn == identifier);

    match job {
        Some(e) => Ok(invocation_job_to_json(e.value())),
        None => Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Invocation job not found: {identifier}"),
        )),
    }
}

pub fn list_model_invocation_jobs(
    state: &BedrockState,
    _input: &Value,
) -> Result<Value, AwsError> {
    let summaries: Vec<Value> = state
        .invocation_jobs
        .iter()
        .map(|e| invocation_job_to_json(e.value()))
        .collect();
    Ok(json!({ "invocationJobSummaries": summaries }))
}

pub fn stop_model_invocation_job(state: &BedrockState, input: &Value) -> Result<Value, AwsError> {
    let identifier = input["jobIdentifier"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "jobIdentifier is required"))?;

    let key = state
        .invocation_jobs
        .iter()
        .find(|e| e.key() == identifier || e.value().job_arn == identifier)
        .map(|e| e.key().clone());

    match key {
        Some(k) => {
            if let Some(mut job) = state.invocation_jobs.get_mut(&k) {
                job.status = "Stopped".to_string();
            }
            Ok(json!({}))
        }
        None => Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Invocation job not found: {identifier}"),
        )),
    }
}

// ── Knowledge Bases ───────────────────────────────────────────────────────────

fn kb_to_json(k: &KnowledgeBase) -> Value {
    json!({
        "knowledgeBaseId": k.knowledge_base_id,
        "knowledgeBaseArn": k.knowledge_base_arn,
        "name": k.name,
        "description": k.description,
        "roleArn": k.role_arn,
        "status": k.status,
        "createdAt": k.created_at,
        "updatedAt": k.created_at,
    })
}

pub fn create_knowledge_base(
    state: &BedrockState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "name is required"))?;
    let role_arn = input["roleArn"].as_str().unwrap_or("").to_string();

    let kb_id = Uuid::new_v4().to_string().replace('-', "")[..10].to_string();
    let kb_arn = format!(
        "arn:aws:bedrock:{}:{}:knowledge-base/{}",
        ctx.region, ctx.account_id, kb_id
    );

    let kb = KnowledgeBase {
        knowledge_base_id: kb_id.clone(),
        knowledge_base_arn: kb_arn,
        name: name.to_string(),
        description: input["description"].as_str().map(|s| s.to_string()),
        role_arn,
        status: "ACTIVE".to_string(),
        created_at: now_iso(),
    };

    let result = kb_to_json(&kb);
    state.knowledge_bases.insert(kb_id, kb);

    Ok(json!({ "knowledgeBase": result }))
}

pub fn get_knowledge_base(state: &BedrockState, input: &Value) -> Result<Value, AwsError> {
    let id = input["knowledgeBaseId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "knowledgeBaseId is required"))?;

    let kb = state.knowledge_bases.get(id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Knowledge base {} not found", id),
        )
    })?;

    Ok(json!({ "knowledgeBase": kb_to_json(&*kb) }))
}

pub fn list_knowledge_bases(state: &BedrockState, _input: &Value) -> Result<Value, AwsError> {
    let summaries: Vec<Value> = state
        .knowledge_bases
        .iter()
        .map(|e| {
            let k = e.value();
            json!({
                "knowledgeBaseId": k.knowledge_base_id,
                "name": k.name,
                "description": k.description,
                "status": k.status,
                "updatedAt": k.created_at,
            })
        })
        .collect();
    Ok(json!({ "knowledgeBaseSummaries": summaries }))
}

pub fn delete_knowledge_base(state: &BedrockState, input: &Value) -> Result<Value, AwsError> {
    let id = input["knowledgeBaseId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "knowledgeBaseId is required"))?;

    if state.knowledge_bases.remove(id).is_none() {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Knowledge base {} not found", id),
        ));
    }

    Ok(json!({ "knowledgeBaseId": id, "status": "DELETING" }))
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

fn cm_to_json(c: &CustomModel) -> Value {
    json!({
        "modelName": c.model_name,
        "modelArn": c.model_arn,
        "baseModelArn": c.base_model_arn,
        "creationTime": c.creation_time,
    })
}

pub fn list_custom_models(state: &BedrockState, _input: &Value) -> Result<Value, AwsError> {
    let summaries: Vec<Value> = state
        .custom_models
        .iter()
        .map(|e| cm_to_json(e.value()))
        .collect();
    Ok(json!({ "modelSummaries": summaries }))
}

pub fn get_custom_model(state: &BedrockState, input: &Value) -> Result<Value, AwsError> {
    let identifier = input["modelIdentifier"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "modelIdentifier is required"))?;

    let model = state
        .custom_models
        .iter()
        .find(|e| e.key() == identifier || e.value().model_arn == identifier);

    match model {
        Some(e) => {
            let c = e.value();
            Ok(json!({
                "modelName": c.model_name,
                "modelArn": c.model_arn,
                "baseModelArn": c.base_model_arn,
                "creationTime": c.creation_time,
                "modelKmsKeyArn": null,
                "trainingMetrics": { "trainingLoss": 0.0 },
            }))
        }
        None => Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Custom model not found: {identifier}"),
        )),
    }
}

pub fn delete_custom_model(state: &BedrockState, input: &Value) -> Result<Value, AwsError> {
    let identifier = input["modelIdentifier"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "modelIdentifier is required"))?;

    let key = state
        .custom_models
        .iter()
        .find(|e| e.key() == identifier || e.value().model_arn == identifier)
        .map(|e| e.key().clone());

    match key {
        Some(k) => {
            state.custom_models.remove(&k);
            Ok(json!({}))
        }
        None => Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Custom model not found: {identifier}"),
        )),
    }
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
