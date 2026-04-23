use awsim_core::{AwsError, InternalEvent, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::{missing_parameter, stack_already_exists, stack_not_found},
    ids::{now_iso8601, new_uuid, stack_arn},
    state::{CloudFormationState, Stack, StackEvent, StackResource},
    template,
};

use super::{opt_str, parse_parameters, parse_tags, require_str};

fn stack_to_value(stack: &Stack) -> Value {
    let params: Vec<Value> = stack
        .parameters
        .iter()
        .map(|(k, v)| json!({ "ParameterKey": k, "ParameterValue": v }))
        .collect();

    let tags: Vec<Value> = stack
        .tags
        .iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();

    let outputs: Vec<Value> = stack
        .outputs
        .values()
        .map(|o| {
            let mut obj = json!({
                "OutputKey": o.output_key,
                "OutputValue": o.output_value,
            });
            if let Some(desc) = &o.description {
                obj["Description"] = Value::String(desc.clone());
            }
            obj
        })
        .collect();

    let mut result = json!({
        "StackId": stack.stack_id,
        "StackName": stack.stack_name,
        "StackStatus": stack.status,
        "CreationTime": stack.created_at,
        "Parameters": { "member": params },
        "Tags": { "member": tags },
        "Outputs": { "member": outputs },
    });

    if let Some(reason) = &stack.status_reason {
        result["StackStatusReason"] = Value::String(reason.clone());
    }
    if let Some(updated_at) = &stack.updated_at {
        result["LastUpdatedTime"] = Value::String(updated_at.clone());
    }

    result
}

fn resource_to_value(r: &StackResource, stack: &Stack) -> Value {
    let mut obj = json!({
        "StackName": stack.stack_name,
        "StackId": stack.stack_id,
        "LogicalResourceId": r.logical_resource_id,
        "ResourceType": r.resource_type,
        "ResourceStatus": r.resource_status,
        "Timestamp": r.timestamp,
    });
    if let Some(phys) = &r.physical_resource_id {
        obj["PhysicalResourceId"] = Value::String(phys.clone());
    }
    if let Some(reason) = &r.resource_status_reason {
        obj["ResourceStatusReason"] = Value::String(reason.clone());
    }
    obj
}

fn event_to_value(e: &StackEvent) -> Value {
    let mut obj = json!({
        "EventId": e.event_id,
        "StackId": e.stack_id,
        "StackName": e.stack_name,
        "LogicalResourceId": e.logical_resource_id,
        "ResourceType": e.resource_type,
        "Timestamp": e.timestamp,
        "ResourceStatus": e.resource_status,
    });
    if let Some(phys) = &e.physical_resource_id {
        obj["PhysicalResourceId"] = Value::String(phys.clone());
    }
    if let Some(reason) = &e.resource_status_reason {
        obj["ResourceStatusReason"] = Value::String(reason.clone());
    }
    obj
}

/// Build resources from parsed template, without actually creating them.
fn build_resources(parsed: &template::ParsedTemplate, now: &str) -> Vec<StackResource> {
    parsed
        .resources
        .iter()
        .filter(|r| {
            // Skip resources in false conditions
            if let Some(cond_name) = &r.condition {
                *parsed.conditions.get(cond_name).unwrap_or(&true)
            } else {
                true
            }
        })
        .map(|r| {
            // Generate a fake physical resource ID for now
            let physical_resource_id = Some(format!("awsim-{}-{}", r.logical_id, &new_uuid()[..8]));
            StackResource {
                logical_resource_id: r.logical_id.clone(),
                physical_resource_id,
                resource_type: r.resource_type.clone(),
                resource_status: "CREATE_COMPLETE".to_string(),
                resource_status_reason: None,
                timestamp: now.to_string(),
            }
        })
        .collect()
}

/// Build stack events from resources.
fn build_events(
    resources: &[StackResource],
    stack_id: &str,
    stack_name: &str,
    now: &str,
) -> Vec<StackEvent> {
    let mut events = Vec::new();

    // Stack-level CREATE_IN_PROGRESS event
    events.push(StackEvent {
        event_id: new_uuid(),
        stack_id: stack_id.to_string(),
        stack_name: stack_name.to_string(),
        logical_resource_id: stack_name.to_string(),
        physical_resource_id: Some(stack_id.to_string()),
        resource_type: "AWS::CloudFormation::Stack".to_string(),
        timestamp: now.to_string(),
        resource_status: "CREATE_IN_PROGRESS".to_string(),
        resource_status_reason: Some("User Initiated".to_string()),
    });

    // Per-resource events
    for resource in resources {
        events.push(StackEvent {
            event_id: new_uuid(),
            stack_id: stack_id.to_string(),
            stack_name: stack_name.to_string(),
            logical_resource_id: resource.logical_resource_id.clone(),
            physical_resource_id: resource.physical_resource_id.clone(),
            resource_type: resource.resource_type.clone(),
            timestamp: now.to_string(),
            resource_status: "CREATE_COMPLETE".to_string(),
            resource_status_reason: None,
        });
    }

    // Stack-level CREATE_COMPLETE event
    events.push(StackEvent {
        event_id: new_uuid(),
        stack_id: stack_id.to_string(),
        stack_name: stack_name.to_string(),
        logical_resource_id: stack_name.to_string(),
        physical_resource_id: Some(stack_id.to_string()),
        resource_type: "AWS::CloudFormation::Stack".to_string(),
        timestamp: now.to_string(),
        resource_status: "CREATE_COMPLETE".to_string(),
        resource_status_reason: None,
    });

    events
}

pub fn create_stack(
    state: &CloudFormationState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stack_name = require_str(input, "StackName")?.to_string();

    if state.stacks.contains_key(&stack_name) {
        return Err(stack_already_exists(&stack_name));
    }

    let template_body = opt_str(input, "TemplateBody")
        .or_else(|| opt_str(input, "TemplateURL"))
        .ok_or_else(|| missing_parameter("TemplateBody"))?
        .to_string();

    let parameters = parse_parameters(input);
    let tags = parse_tags(input);

    // Validate and parse template
    let parsed = template::validate_and_parse(&template_body, &parameters)?;

    let now = now_iso8601();
    let stack_id = stack_arn(&ctx.region, &ctx.account_id, &stack_name);
    let resources = build_resources(&parsed, &now);
    let events = build_events(&resources, &stack_id, &stack_name, &now);

    let stack = Stack {
        stack_id: stack_id.clone(),
        stack_name: stack_name.clone(),
        template_body,
        parameters,
        tags,
        status: "CREATE_COMPLETE".to_string(),
        status_reason: None,
        resources,
        events,
        change_sets: HashMap::new(),
        created_at: now,
        updated_at: None,
        outputs: HashMap::new(),
    };

    // Emit one CreateResource event per resource so the background router
    // can provision each resource in the appropriate service.
    if let Some(ref bus) = ctx.event_bus {
        for resource in &stack.resources {
            // Find the matching parsed resource to get its properties.
            let properties = parsed
                .resources
                .iter()
                .find(|r| r.logical_id == resource.logical_resource_id)
                .map(|r| r.properties.clone())
                .unwrap_or(Value::Object(serde_json::Map::new()));

            bus.publish(InternalEvent {
                source: "cloudformation".to_string(),
                event_type: "cloudformation:CreateResource".to_string(),
                region: ctx.region.clone(),
                account_id: ctx.account_id.clone(),
                detail: json!({
                    "stackName": stack_name,
                    "logicalId": resource.logical_resource_id,
                    "resourceType": resource.resource_type,
                    "properties": properties,
                }),
            });
        }
    }

    state.stacks.insert(stack_name, stack);

    Ok(json!({ "StackId": stack_id }))
}

pub fn delete_stack(
    state: &CloudFormationState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stack_name = require_str(input, "StackName")?;

    if let Some((_, mut stack)) = state.stacks.remove(stack_name) {
        // Emit one DeleteResource event per resource before marking deleted.
        if let Some(ref bus) = ctx.event_bus {
            for resource in &stack.resources {
                bus.publish(InternalEvent {
                    source: "cloudformation".to_string(),
                    event_type: "cloudformation:DeleteResource".to_string(),
                    region: ctx.region.clone(),
                    account_id: ctx.account_id.clone(),
                    detail: json!({
                        "stackName": stack_name,
                        "logicalId": resource.logical_resource_id,
                        "resourceType": resource.resource_type,
                        "physicalResourceId": resource.physical_resource_id,
                    }),
                });
            }
        }

        // Mark as DELETE_COMPLETE (keep entry with status for ListStacks)
        stack.status = "DELETE_COMPLETE".to_string();
        stack.updated_at = Some(now_iso8601());
        state.stacks.insert(stack_name.to_string(), stack);
    }
    // DeleteStack is idempotent — no error if not found

    Ok(json!({}))
}

pub fn update_stack(
    state: &CloudFormationState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stack_name = require_str(input, "StackName")?;

    let mut stack = state
        .stacks
        .get_mut(stack_name)
        .ok_or_else(|| stack_not_found(stack_name))?;

    let template_body = opt_str(input, "TemplateBody")
        .map(|s| s.to_string())
        .unwrap_or_else(|| stack.template_body.clone());

    let new_parameters = parse_parameters(input);
    let effective_params = if new_parameters.is_empty() {
        stack.parameters.clone()
    } else {
        new_parameters
    };

    // Validate new template
    let parsed = template::validate_and_parse(&template_body, &effective_params)?;

    let now = now_iso8601();
    let resources = build_resources(&parsed, &now);

    let update_events = resources
        .iter()
        .map(|r| StackEvent {
            event_id: new_uuid(),
            stack_id: stack.stack_id.clone(),
            stack_name: stack.stack_name.clone(),
            logical_resource_id: r.logical_resource_id.clone(),
            physical_resource_id: r.physical_resource_id.clone(),
            resource_type: r.resource_type.clone(),
            timestamp: now.clone(),
            resource_status: "UPDATE_COMPLETE".to_string(),
            resource_status_reason: None,
        })
        .collect::<Vec<_>>();

    stack.template_body = template_body;
    stack.parameters = effective_params;
    stack.resources = resources;
    stack.events.extend(update_events);
    stack.status = "UPDATE_COMPLETE".to_string();
    stack.updated_at = Some(now);

    let stack_id = stack.stack_id.clone();
    Ok(json!({ "StackId": stack_id }))
}

pub fn describe_stacks(state: &CloudFormationState, input: &Value) -> Result<Value, AwsError> {
    let filter_name = opt_str(input, "StackName");

    let stacks: Vec<Value> = state
        .stacks
        .iter()
        .filter(|entry| {
            // Exclude DELETE_COMPLETE unless specifically queried
            if entry.status == "DELETE_COMPLETE" && filter_name.is_none() {
                return false;
            }
            if let Some(name) = filter_name {
                entry.stack_name == name || entry.stack_id == name
            } else {
                true
            }
        })
        .map(|entry| stack_to_value(&entry))
        .collect();

    if let Some(name) = filter_name {
        if stacks.is_empty() {
            return Err(stack_not_found(name));
        }
    }

    Ok(json!({ "Stacks": { "member": stacks } }))
}

pub fn describe_stack_events(
    state: &CloudFormationState,
    input: &Value,
) -> Result<Value, AwsError> {
    let stack_name = require_str(input, "StackName")?;

    let stack = state
        .stacks
        .get(stack_name)
        .ok_or_else(|| stack_not_found(stack_name))?;

    let events: Vec<Value> = stack.events.iter().map(event_to_value).collect();

    Ok(json!({ "StackEvents": { "member": events } }))
}

pub fn describe_stack_resources(
    state: &CloudFormationState,
    input: &Value,
) -> Result<Value, AwsError> {
    let stack_name = require_str(input, "StackName")?;

    let stack = state
        .stacks
        .get(stack_name)
        .ok_or_else(|| stack_not_found(stack_name))?;

    let resources: Vec<Value> = stack
        .resources
        .iter()
        .map(|r| resource_to_value(r, &stack))
        .collect();

    Ok(json!({ "StackResources": { "member": resources } }))
}

pub fn list_stacks(state: &CloudFormationState, input: &Value) -> Result<Value, AwsError> {
    // Parse StackStatusFilter
    let status_filters: Vec<String> = match input.get("StackStatusFilter") {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        Some(Value::Object(obj)) => {
            if let Some(Value::Array(arr)) = obj.get("member") {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    };

    let summaries: Vec<Value> = state
        .stacks
        .iter()
        .filter(|entry| {
            status_filters.is_empty() || status_filters.contains(&entry.status)
        })
        .map(|entry| {
            json!({
                "StackId": entry.stack_id,
                "StackName": entry.stack_name,
                "StackStatus": entry.status,
                "CreationTime": entry.created_at,
            })
        })
        .collect();

    Ok(json!({ "StackSummaries": { "member": summaries } }))
}

pub fn get_template(state: &CloudFormationState, input: &Value) -> Result<Value, AwsError> {
    let stack_name = require_str(input, "StackName")?;

    let stack = state
        .stacks
        .get(stack_name)
        .ok_or_else(|| stack_not_found(stack_name))?;

    Ok(json!({ "TemplateBody": stack.template_body }))
}

/// DescribeStackResource — get a single resource from a stack by logical ID.
pub fn describe_stack_resource(
    state: &CloudFormationState,
    input: &Value,
) -> Result<Value, AwsError> {
    let stack_name = require_str(input, "StackName")?;
    let logical_id = require_str(input, "LogicalResourceId")?;

    let stack = state
        .stacks
        .get(stack_name)
        .ok_or_else(|| stack_not_found(stack_name))?;

    let resource = stack
        .resources
        .iter()
        .find(|r| r.logical_resource_id == logical_id)
        .ok_or_else(|| {
            crate::error::missing_parameter(&format!(
                "Resource {logical_id} does not exist for stack {stack_name}"
            ))
        })?;

    let detail = resource_to_value(resource, &stack);

    Ok(json!({ "StackResourceDetail": detail }))
}

/// GetTemplateSummary — parse template and return metadata without creating a stack.
pub fn get_template_summary(_state: &CloudFormationState, input: &Value) -> Result<Value, AwsError> {
    let template_body = opt_str(input, "TemplateBody")
        .ok_or_else(|| missing_parameter("TemplateBody"))?;

    let parsed = template::validate_and_parse(template_body, &HashMap::new())?;

    let params: Vec<Value> = parsed
        .parameters
        .iter()
        .map(|p| {
            let mut obj = json!({
                "ParameterKey": p.name,
                "ParameterType": p.param_type,
                "NoEcho": false,
            });
            if let Some(desc) = &p.description {
                obj["Description"] = Value::String(desc.clone());
            }
            if let Some(default) = &p.default {
                obj["DefaultValue"] = Value::String(default.clone());
            }
            obj
        })
        .collect();

    let resource_types: Vec<Value> = parsed
        .resources
        .iter()
        .map(|r| Value::String(r.resource_type.clone()))
        .collect();

    let mut result = json!({
        "Parameters": params,
        "ResourceTypes": { "member": resource_types },
        "Version": "2010-09-09",
        "Capabilities": { "member": [] },
        "CapabilitiesReason": null,
    });

    if let Some(desc) = parsed.description {
        result["Description"] = Value::String(desc);
    }

    Ok(result)
}

/// ListStackResources — paginated list of resources in a stack.
pub fn list_stack_resources(
    state: &CloudFormationState,
    input: &Value,
) -> Result<Value, AwsError> {
    let stack_name = require_str(input, "StackName")?;

    let stack = state
        .stacks
        .get(stack_name)
        .ok_or_else(|| stack_not_found(stack_name))?;

    let summaries: Vec<Value> = stack
        .resources
        .iter()
        .map(|r| {
            let mut obj = json!({
                "LogicalResourceId": r.logical_resource_id,
                "ResourceType": r.resource_type,
                "ResourceStatus": r.resource_status,
                "LastUpdatedTimestamp": r.timestamp,
            });
            if let Some(phys) = &r.physical_resource_id {
                obj["PhysicalResourceId"] = Value::String(phys.clone());
            }
            obj
        })
        .collect();

    Ok(json!({
        "StackResourceSummaries": { "member": summaries },
        "NextToken": null,
    }))
}

/// ListExports — stub returning empty list.
pub fn list_exports(_state: &CloudFormationState, _input: &Value) -> Result<Value, AwsError> {
    Ok(json!({ "Exports": { "member": [] }, "NextToken": null }))
}

/// ListImports — stub returning empty list.
pub fn list_imports(_state: &CloudFormationState, _input: &Value) -> Result<Value, AwsError> {
    Ok(json!({ "Imports": { "member": [] }, "NextToken": null }))
}

/// TagResource — add or update tags on a stack.
pub fn tag_resource(state: &CloudFormationState, input: &Value) -> Result<Value, AwsError> {
    let resource_arn = require_str(input, "ResourceArn")?;
    // Extract the stack name from the ARN (last segment after the final '/')
    let stack_name = resource_arn.split('/').nth(1).unwrap_or(resource_arn);

    let new_tags = parse_tags(input);

    let mut entry = state
        .stack_tags
        .entry(stack_name.to_string())
        .or_default();
    for (k, v) in new_tags {
        entry.insert(k, v);
    }

    Ok(json!({}))
}

/// UntagResource — remove tags from a stack.
pub fn untag_resource(state: &CloudFormationState, input: &Value) -> Result<Value, AwsError> {
    let resource_arn = require_str(input, "ResourceArn")?;
    let stack_name = resource_arn.split('/').nth(1).unwrap_or(resource_arn);

    let tag_keys: Vec<&str> = match input.get("TagKeys") {
        Some(Value::Array(arr)) => arr.iter().filter_map(|v| v.as_str()).collect(),
        Some(Value::Object(obj)) => {
            if let Some(Value::Array(arr)) = obj.get("member") {
                arr.iter().filter_map(|v| v.as_str()).collect()
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    };

    if let Some(mut tags) = state.stack_tags.get_mut(stack_name) {
        for key in &tag_keys {
            tags.remove(*key);
        }
    }

    Ok(json!({}))
}

/// SignalResource — accept and succeed silently.
pub fn signal_resource(_state: &CloudFormationState, _input: &Value) -> Result<Value, AwsError> {
    Ok(json!({}))
}

/// EstimateTemplateCost — stub returning a cost estimate URL.
pub fn estimate_template_cost(
    _state: &CloudFormationState,
    _input: &Value,
) -> Result<Value, AwsError> {
    Ok(json!({
        "Url": "http://calculator.s3.amazonaws.com/calc5.html?key=awsim-stub-estimate"
    }))
}

pub fn validate_template(_state: &CloudFormationState, input: &Value) -> Result<Value, AwsError> {
    let template_body = opt_str(input, "TemplateBody")
        .ok_or_else(|| missing_parameter("TemplateBody"))?;

    let parsed = template::validate_and_parse(template_body, &HashMap::new())?;

    let params: Vec<Value> = parsed
        .parameters
        .iter()
        .map(|p| {
            let mut obj = json!({
                "ParameterKey": p.name,
                "ParameterType": p.param_type,
            });
            if let Some(desc) = &p.description {
                obj["Description"] = Value::String(desc.clone());
            }
            if let Some(default) = &p.default {
                obj["DefaultValue"] = Value::String(default.clone());
            }
            obj
        })
        .collect();

    let mut result = json!({ "Parameters": { "member": params } });
    if let Some(desc) = parsed.description {
        result["Description"] = Value::String(desc);
    }

    Ok(result)
}
