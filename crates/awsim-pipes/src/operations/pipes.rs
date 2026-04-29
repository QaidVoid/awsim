use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{Pipe, PipesState};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("ValidationException", format!("Missing {key}")))
}

fn pipe_arn(ctx: &RequestContext, name: &str) -> String {
    format!(
        "arn:aws:pipes:{}:{}:pipe/{}",
        ctx.region, ctx.account_id, name
    )
}

fn pipe_to_summary(p: &Pipe) -> Value {
    json!({
        "Name": p.name,
        "Arn": p.arn,
        "Source": p.source,
        "Target": p.target,
        "CurrentState": p.current_state,
        "DesiredState": p.desired_state,
        "StateReason": p.state_reason,
        "Enrichment": p.enrichment,
        "CreationTime": p.creation_time,
        "LastModifiedTime": p.last_modified_time,
    })
}

fn pipe_to_describe(p: &Pipe) -> Value {
    let mut v = json!({
        "Name": p.name,
        "Arn": p.arn,
        "Source": p.source,
        "Target": p.target,
        "CurrentState": p.current_state,
        "DesiredState": p.desired_state,
        "StateReason": p.state_reason,
        "RoleArn": p.role_arn,
        "Description": p.description,
        "Enrichment": p.enrichment,
        "Tags": p.tags,
        "CreationTime": p.creation_time,
        "LastModifiedTime": p.last_modified_time,
    });
    if let Some(sp) = &p.source_parameters {
        v["SourceParameters"] = sp.clone();
    }
    if let Some(tp) = &p.target_parameters {
        v["TargetParameters"] = tp.clone();
    }
    if let Some(ep) = &p.enrichment_parameters {
        v["EnrichmentParameters"] = ep.clone();
    }
    if let Some(lc) = &p.log_configuration {
        v["LogConfiguration"] = lc.clone();
    }
    v
}

pub fn create_pipe(
    state: &PipesState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "Name")?.to_string();
    if state.pipes.contains_key(&name) {
        return Err(AwsError::conflict(
            "ConflictException",
            format!("Pipe {name} already exists"),
        ));
    }
    let source = require_str(input, "Source")?.to_string();
    let target = require_str(input, "Target")?.to_string();
    let role_arn = require_str(input, "RoleArn")?.to_string();
    let desired_state = input
        .get("DesiredState")
        .and_then(|v| v.as_str())
        .unwrap_or("RUNNING")
        .to_string();

    let now = now_secs();
    let pipe = Pipe {
        name: name.clone(),
        arn: pipe_arn(ctx, &name),
        source,
        target,
        current_state: if desired_state == "RUNNING" {
            "RUNNING".to_string()
        } else {
            "STOPPED".to_string()
        },
        desired_state,
        state_reason: None,
        role_arn,
        description: input
            .get("Description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        source_parameters: input.get("SourceParameters").cloned(),
        target_parameters: input.get("TargetParameters").cloned(),
        enrichment: input
            .get("Enrichment")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        enrichment_parameters: input.get("EnrichmentParameters").cloned(),
        log_configuration: input.get("LogConfiguration").cloned(),
        tags: input
            .get("Tags")
            .and_then(|v| v.as_object())
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default(),
        creation_time: now,
        last_modified_time: now,
    };
    let result = json!({
        "Name": pipe.name,
        "Arn": pipe.arn,
        "CurrentState": pipe.current_state,
        "DesiredState": pipe.desired_state,
        "CreationTime": pipe.creation_time,
        "LastModifiedTime": pipe.last_modified_time,
    });
    state.pipes.insert(name, pipe);
    Ok(result)
}

pub fn describe_pipe(
    state: &PipesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "Name")?;
    let p = state.pipes.get(name).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("Pipe {name} not found"))
    })?;
    Ok(pipe_to_describe(&p))
}

pub fn list_pipes(
    state: &PipesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name_prefix = input.get("NamePrefix").and_then(|v| v.as_str());
    let source_prefix = input.get("SourcePrefix").and_then(|v| v.as_str());
    let target_prefix = input.get("TargetPrefix").and_then(|v| v.as_str());
    let current_state = input.get("CurrentState").and_then(|v| v.as_str());
    let desired_state = input.get("DesiredState").and_then(|v| v.as_str());

    let mut pipes: Vec<Value> = state
        .pipes
        .iter()
        .filter(|e| {
            let p = e.value();
            if let Some(np) = name_prefix
                && !p.name.starts_with(np)
            {
                return false;
            }
            if let Some(sp) = source_prefix
                && !p.source.starts_with(sp)
            {
                return false;
            }
            if let Some(tp) = target_prefix
                && !p.target.starts_with(tp)
            {
                return false;
            }
            if let Some(cs) = current_state
                && p.current_state != cs
            {
                return false;
            }
            if let Some(ds) = desired_state
                && p.desired_state != ds
            {
                return false;
            }
            true
        })
        .map(|e| pipe_to_summary(e.value()))
        .collect();
    pipes.sort_by(|a, b| {
        a["Name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["Name"].as_str().unwrap_or(""))
    });
    Ok(json!({ "Pipes": pipes }))
}

pub fn delete_pipe(
    state: &PipesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "Name")?;
    let (_, p) = state.pipes.remove(name).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("Pipe {name} not found"))
    })?;
    Ok(json!({
        "Name": p.name,
        "Arn": p.arn,
        "CurrentState": "DELETING",
        "DesiredState": "STOPPED",
        "CreationTime": p.creation_time,
        "LastModifiedTime": now_secs(),
    }))
}

pub fn update_pipe(
    state: &PipesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "Name")?;
    let mut p = state.pipes.get_mut(name).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("Pipe {name} not found"))
    })?;

    if let Some(role) = input.get("RoleArn").and_then(|v| v.as_str()) {
        p.role_arn = role.to_string();
    }
    if let Some(desc) = input.get("Description").and_then(|v| v.as_str()) {
        p.description = Some(desc.to_string());
    }
    if let Some(target) = input.get("Target").and_then(|v| v.as_str()) {
        p.target = target.to_string();
    }
    if let Some(ds) = input.get("DesiredState").and_then(|v| v.as_str()) {
        p.desired_state = ds.to_string();
        p.current_state = match ds {
            "RUNNING" => "RUNNING".to_string(),
            "STOPPED" => "STOPPED".to_string(),
            other => other.to_string(),
        };
    }
    if let Some(sp) = input.get("SourceParameters") {
        p.source_parameters = Some(sp.clone());
    }
    if let Some(tp) = input.get("TargetParameters") {
        p.target_parameters = Some(tp.clone());
    }
    if let Some(en) = input.get("Enrichment").and_then(|v| v.as_str()) {
        p.enrichment = Some(en.to_string());
    }
    if let Some(ep) = input.get("EnrichmentParameters") {
        p.enrichment_parameters = Some(ep.clone());
    }
    if let Some(lc) = input.get("LogConfiguration") {
        p.log_configuration = Some(lc.clone());
    }
    p.last_modified_time = now_secs();
    Ok(json!({
        "Name": p.name,
        "Arn": p.arn,
        "CurrentState": p.current_state,
        "DesiredState": p.desired_state,
        "CreationTime": p.creation_time,
        "LastModifiedTime": p.last_modified_time,
    }))
}

pub fn start_pipe(
    state: &PipesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "Name")?;
    let mut p = state.pipes.get_mut(name).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("Pipe {name} not found"))
    })?;
    p.desired_state = "RUNNING".to_string();
    p.current_state = "RUNNING".to_string();
    p.last_modified_time = now_secs();
    Ok(json!({
        "Name": p.name,
        "Arn": p.arn,
        "CurrentState": p.current_state,
        "DesiredState": p.desired_state,
        "CreationTime": p.creation_time,
        "LastModifiedTime": p.last_modified_time,
    }))
}

pub fn stop_pipe(
    state: &PipesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "Name")?;
    let mut p = state.pipes.get_mut(name).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("Pipe {name} not found"))
    })?;
    p.desired_state = "STOPPED".to_string();
    p.current_state = "STOPPED".to_string();
    p.last_modified_time = now_secs();
    Ok(json!({
        "Name": p.name,
        "Arn": p.arn,
        "CurrentState": p.current_state,
        "DesiredState": p.desired_state,
        "CreationTime": p.creation_time,
        "LastModifiedTime": p.last_modified_time,
    }))
}

fn pipe_name_from_arn(arn: &str) -> Option<String> {
    arn.rsplit_once('/').map(|(_, n)| n.to_string())
}

pub fn list_tags_for_resource(
    state: &PipesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = require_str(input, "ResourceArn")?;
    let name = pipe_name_from_arn(arn).unwrap_or_default();
    let tags = state
        .pipes
        .get(&name)
        .map(|p| p.tags.clone())
        .unwrap_or_default();
    let tags_json: HashMap<String, Value> = tags.into_iter().map(|(k, v)| (k, json!(v))).collect();
    Ok(json!({ "tags": tags_json }))
}
