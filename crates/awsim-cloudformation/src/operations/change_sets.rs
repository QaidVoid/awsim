use awsim_core::{AwsError, InternalEvent, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::{change_set_not_found, missing_parameter, stack_not_found},
    ids::{change_set_arn, new_uuid, now_iso8601, stack_arn},
    state::{Change, ChangeSet, CloudFormationState, Stack, StackEvent, StackResource},
    template,
};

use super::{opt_str, parse_parameters, require_str};

fn change_set_to_value(cs: &ChangeSet) -> Value {
    let changes: Vec<Value> = cs
        .changes
        .iter()
        .map(|c| {
            json!({
                "Type": "Resource",
                "ResourceChange": {
                    "Action": c.action,
                    "LogicalResourceId": c.logical_resource_id,
                    "ResourceType": c.resource_type,
                }
            })
        })
        .collect();

    let params: Vec<Value> = cs
        .parameters
        .iter()
        .map(|(k, v)| json!({ "ParameterKey": k, "ParameterValue": v }))
        .collect();

    let mut obj = json!({
        "ChangeSetId": cs.change_set_id,
        "ChangeSetName": cs.change_set_name,
        "StackId": cs.stack_id,
        "StackName": cs.stack_name,
        "Status": cs.status,
        "Changes": { "member": changes },
        "Parameters": { "member": params },
        "CreationTime": cs.created_at,
    });

    if let Some(reason) = &cs.status_reason {
        obj["StatusReason"] = Value::String(reason.clone());
    }

    obj
}

pub fn create_change_set(
    state: &CloudFormationState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stack_name = require_str(input, "StackName")?.to_string();
    let change_set_name = require_str(input, "ChangeSetName")?.to_string();
    let template_body = opt_str(input, "TemplateBody")
        .ok_or_else(|| missing_parameter("TemplateBody"))?
        .to_string();

    let parameters = parse_parameters(input);

    // Validate template
    let parsed = template::validate_and_parse(&template_body, &parameters)?;

    let now = now_iso8601();

    // Determine if this is a create or update
    let (stack_id, changes) = if let Some(existing) = state.stacks.get(&stack_name) {
        // Update change set: compute diff (simplified — mark all resources as Modify)
        let changes = parsed
            .resources
            .iter()
            .map(|r| {
                let action = if existing
                    .resources
                    .iter()
                    .any(|er| er.logical_resource_id == r.logical_id)
                {
                    "Modify"
                } else {
                    "Add"
                };
                Change {
                    action: action.to_string(),
                    logical_resource_id: r.logical_id.clone(),
                    resource_type: r.resource_type.clone(),
                }
            })
            .collect();
        (existing.stack_id.clone(), changes)
    } else {
        // Create change set: all resources are "Add"
        let new_stack_id = stack_arn(&ctx.region, &ctx.account_id, &stack_name);
        let changes = parsed
            .resources
            .iter()
            .map(|r| Change {
                action: "Add".to_string(),
                logical_resource_id: r.logical_id.clone(),
                resource_type: r.resource_type.clone(),
            })
            .collect();
        (new_stack_id, changes)
    };

    let change_set_id = change_set_arn(&ctx.region, &ctx.account_id, &stack_name, &change_set_name);

    let change_set = ChangeSet {
        change_set_id: change_set_id.clone(),
        change_set_name: change_set_name.clone(),
        stack_id: stack_id.clone(),
        stack_name: stack_name.clone(),
        template_body: Some(template_body),
        parameters,
        status: "CREATE_COMPLETE".to_string(),
        status_reason: None,
        changes,
        created_at: now,
    };

    // Ensure the stack entry exists (may be a pre-creation change set)
    if !state.stacks.contains_key(&stack_name) {
        let placeholder_stack = Stack {
            stack_id: stack_id.clone(),
            stack_name: stack_name.clone(),
            template_body: String::new(),
            parameters: HashMap::new(),
            tags: HashMap::new(),
            status: "REVIEW_IN_PROGRESS".to_string(),
            status_reason: None,
            resources: Vec::new(),
            events: Vec::new(),
            change_sets: HashMap::new(),
            created_at: now_iso8601(),
            updated_at: None,
            outputs: HashMap::new(),
            termination_protection: false,
        };
        state.stacks.insert(stack_name.clone(), placeholder_stack);
    }

    if let Some(mut stack) = state.stacks.get_mut(&stack_name) {
        stack
            .change_sets
            .insert(change_set_name.clone(), change_set);
    }

    Ok(json!({ "Id": change_set_id, "StackId": stack_id }))
}

pub fn execute_change_set(
    state: &CloudFormationState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let change_set_name = require_str(input, "ChangeSetName")?;
    let stack_name = require_str(input, "StackName")?;

    // Get the change set info
    let change_set = {
        let stack = state
            .stacks
            .get(stack_name)
            .ok_or_else(|| stack_not_found(stack_name))?;
        stack
            .change_sets
            .get(change_set_name)
            .ok_or_else(|| change_set_not_found(change_set_name))?
            .clone()
    };

    // Apply the change set: parse template and build resources
    let template_body = change_set.template_body.clone().unwrap_or_default();
    let parsed = template::validate_and_parse(&template_body, &change_set.parameters)?;

    let now = now_iso8601();
    let resources: Vec<StackResource> = parsed
        .resources
        .iter()
        .filter(|r| {
            if let Some(cond_name) = &r.condition {
                *parsed.conditions.get(cond_name).unwrap_or(&true)
            } else {
                true
            }
        })
        .map(|r| {
            let physical_resource_id = Some(format!("awsim-{}-{}", r.logical_id, &new_uuid()[..8]));
            StackResource {
                logical_resource_id: r.logical_id.clone(),
                physical_resource_id,
                resource_type: r.resource_type.clone(),
                resource_status: "CREATE_COMPLETE".to_string(),
                resource_status_reason: None,
                timestamp: now.clone(),
            }
        })
        .collect();

    let events: Vec<StackEvent> = resources
        .iter()
        .map(|r| StackEvent {
            event_id: new_uuid(),
            stack_id: change_set.stack_id.clone(),
            stack_name: stack_name.to_string(),
            logical_resource_id: r.logical_resource_id.clone(),
            physical_resource_id: r.physical_resource_id.clone(),
            resource_type: r.resource_type.clone(),
            timestamp: now.clone(),
            resource_status: "CREATE_COMPLETE".to_string(),
            resource_status_reason: None,
        })
        .collect();

    // Emit CreateResource events for background provisioning.
    if let Some(ref bus) = ctx.event_bus {
        for resource in &resources {
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

    if let Some(mut stack) = state.stacks.get_mut(stack_name) {
        stack.template_body = template_body;
        stack.parameters = change_set.parameters.clone();
        stack.resources = resources;
        stack.events.extend(events);
        stack.status = "CREATE_COMPLETE".to_string();
        stack.updated_at = Some(now);
    }

    Ok(json!({}))
}

pub fn delete_change_set(state: &CloudFormationState, input: &Value) -> Result<Value, AwsError> {
    let change_set_name = require_str(input, "ChangeSetName")?;
    let stack_name = require_str(input, "StackName")?;

    let mut stack = state
        .stacks
        .get_mut(stack_name)
        .ok_or_else(|| stack_not_found(stack_name))?;

    if stack.change_sets.remove(change_set_name).is_none() {
        return Err(change_set_not_found(change_set_name));
    }

    Ok(json!({}))
}

pub fn describe_change_set(state: &CloudFormationState, input: &Value) -> Result<Value, AwsError> {
    let change_set_name = require_str(input, "ChangeSetName")?;
    let stack_name = require_str(input, "StackName")?;

    let stack = state
        .stacks
        .get(stack_name)
        .ok_or_else(|| stack_not_found(stack_name))?;

    let cs = stack
        .change_sets
        .get(change_set_name)
        .ok_or_else(|| change_set_not_found(change_set_name))?;

    Ok(change_set_to_value(cs))
}

pub fn list_change_sets(state: &CloudFormationState, input: &Value) -> Result<Value, AwsError> {
    let stack_name = require_str(input, "StackName")?;

    let stack = state
        .stacks
        .get(stack_name)
        .ok_or_else(|| stack_not_found(stack_name))?;

    let summaries: Vec<Value> = stack
        .change_sets
        .values()
        .map(|cs| {
            json!({
                "ChangeSetId": cs.change_set_id,
                "ChangeSetName": cs.change_set_name,
                "StackId": cs.stack_id,
                "StackName": cs.stack_name,
                "Status": cs.status,
                "CreationTime": cs.created_at,
            })
        })
        .collect();

    Ok(json!({ "Summaries": { "member": summaries } }))
}
