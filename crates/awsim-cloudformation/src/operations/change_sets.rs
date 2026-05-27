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

/// Diff two `Properties` blocks and report the CloudFormation
/// "scope" of the change. AWS documents the scope as any subset of
/// `Properties`, `Metadata`, `Tags`, `CreationPolicy`,
/// `UpdatePolicy`, `DeletionPolicy`; the simulator partitions any
/// non-tag-only change into `Properties`.
fn compute_scope(old: &Value, new: &Value) -> Vec<String> {
    let mut scope = Vec::new();

    let old_obj = old.as_object();
    let new_obj = new.as_object();

    let old_tags = old.get("Tags");
    let new_tags = new.get("Tags");
    if old_tags != new_tags {
        scope.push("Tags".to_string());
    }

    let old_meta = old.get("Metadata");
    let new_meta = new.get("Metadata");
    if old_meta != new_meta {
        scope.push("Metadata".to_string());
    }

    for policy in ["CreationPolicy", "UpdatePolicy", "DeletionPolicy"] {
        if old.get(policy) != new.get(policy) {
            scope.push(policy.to_string());
        }
    }

    // Property changes outside the well-known top-level keys count
    // as a `Properties` scope change. Build the union of keys and
    // compare each unless it's already covered above.
    let mut keys: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    if let Some(o) = old_obj {
        keys.extend(o.keys().map(String::as_str));
    }
    if let Some(n) = new_obj {
        keys.extend(n.keys().map(String::as_str));
    }
    for key in keys {
        if matches!(
            key,
            "Tags" | "Metadata" | "CreationPolicy" | "UpdatePolicy" | "DeletionPolicy"
        ) {
            continue;
        }
        let oval = old.get(key);
        let nval = new.get(key);
        if oval != nval {
            if !scope.iter().any(|s| s == "Properties") {
                scope.push("Properties".to_string());
            }
            break;
        }
    }

    scope
}

fn change_set_to_value(cs: &ChangeSet) -> Value {
    let changes: Vec<Value> = cs
        .changes
        .iter()
        .map(|c| {
            let mut resource_change = json!({
                "Action": c.action,
                "LogicalResourceId": c.logical_resource_id,
                "ResourceType": c.resource_type,
            });
            if let Some(ref rep) = c.replacement {
                resource_change["Replacement"] = Value::String(rep.clone());
            }
            if !c.scope.is_empty() {
                resource_change["Scope"] =
                    Value::Array(c.scope.iter().map(|s| Value::String(s.clone())).collect());
            }
            json!({
                "Type": "Resource",
                "ResourceChange": resource_change,
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
        // Re-parse the existing template once so we can diff the
        // declared properties of each resource against the new
        // template. The simulator only tracks the synthesized
        // StackResource entries (logical id + type), so the property
        // payload lives in the stored template body.
        let existing_template = existing.template_body.clone();
        let existing_parameters = existing.parameters.clone();
        let existing_parsed =
            template::validate_and_parse(&existing_template, &existing_parameters).ok();
        let mut changes: Vec<Change> = parsed
            .resources
            .iter()
            .map(|r| {
                let prior = existing_parsed
                    .as_ref()
                    .and_then(|p| p.resources.iter().find(|er| er.logical_id == r.logical_id));
                if let Some(prior_res) = prior {
                    let scope = compute_scope(&prior_res.properties, &r.properties);
                    let replacement = if scope.iter().any(|s| s == "Properties") {
                        Some("True".to_string())
                    } else if !scope.is_empty() {
                        // Tag-only or metadata-only changes don't
                        // replace the resource.
                        Some("False".to_string())
                    } else {
                        // Nothing actually changed; AWS still surfaces
                        // the Modify with Replacement=False rather
                        // than dropping the change.
                        Some("False".to_string())
                    };
                    Change {
                        action: "Modify".to_string(),
                        logical_resource_id: r.logical_id.clone(),
                        resource_type: r.resource_type.clone(),
                        replacement,
                        scope,
                    }
                } else {
                    Change {
                        action: "Add".to_string(),
                        logical_resource_id: r.logical_id.clone(),
                        resource_type: r.resource_type.clone(),
                        replacement: None,
                        scope: Vec::new(),
                    }
                }
            })
            .collect();
        // Resources that existed before but disappear from the new
        // template are documented as Remove changes.
        if let Some(ref prior) = existing_parsed {
            for er in &prior.resources {
                if !parsed
                    .resources
                    .iter()
                    .any(|r| r.logical_id == er.logical_id)
                {
                    changes.push(Change {
                        action: "Remove".to_string(),
                        logical_resource_id: er.logical_id.clone(),
                        resource_type: er.resource_type.clone(),
                        replacement: None,
                        scope: Vec::new(),
                    });
                }
            }
        }
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
                replacement: None,
                scope: Vec::new(),
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
            notification_arns: Vec::new(),
            on_failure: "ROLLBACK".to_string(),
            stack_policy_body: None,
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
                deletion_policy: r.deletion_policy.clone(),
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

#[cfg(test)]
mod change_set_diff_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("cloudformation", "us-east-1")
    }

    #[test]
    fn compute_scope_returns_empty_for_identical_properties() {
        let a = json!({ "BucketName": "b1" });
        assert!(compute_scope(&a, &a).is_empty());
    }

    #[test]
    fn compute_scope_flags_properties_on_value_change() {
        let a = json!({ "BucketName": "old" });
        let b = json!({ "BucketName": "new" });
        let s = compute_scope(&a, &b);
        assert_eq!(s, vec!["Properties".to_string()]);
    }

    #[test]
    fn compute_scope_flags_tags_independently_of_properties() {
        let a = json!({
            "BucketName": "b1",
            "Tags": [{"Key": "env", "Value": "dev"}]
        });
        let b = json!({
            "BucketName": "b1",
            "Tags": [{"Key": "env", "Value": "prod"}]
        });
        let s = compute_scope(&a, &b);
        assert_eq!(s, vec!["Tags".to_string()]);
    }

    #[test]
    fn compute_scope_flags_metadata_and_lifecycle_policies() {
        let a = json!({
            "BucketName": "b1",
            "Metadata": {"who": "a"},
            "DeletionPolicy": "Retain"
        });
        let b = json!({
            "BucketName": "b1",
            "Metadata": {"who": "b"},
            "DeletionPolicy": "Delete"
        });
        let s = compute_scope(&a, &b);
        assert!(s.contains(&"Metadata".to_string()));
        assert!(s.contains(&"DeletionPolicy".to_string()));
        assert!(!s.contains(&"Properties".to_string()));
    }

    #[test]
    fn modify_with_property_change_marks_replacement_true() {
        let state = CloudFormationState::default();
        let template_v1 = r#"{
          "Resources": {
            "B": { "Type": "AWS::S3::Bucket", "Properties": { "BucketName": "v1" } }
          }
        }"#;
        let template_v2 = r#"{
          "Resources": {
            "B": { "Type": "AWS::S3::Bucket", "Properties": { "BucketName": "v2" } }
          }
        }"#;

        // Pre-populate the stack as if a prior CreateStack had run.
        state.stacks.insert(
            "s".into(),
            Stack {
                stack_id: stack_arn("us-east-1", "000000000000", "s"),
                stack_name: "s".into(),
                template_body: template_v1.to_string(),
                parameters: HashMap::new(),
                tags: HashMap::new(),
                status: "CREATE_COMPLETE".into(),
                status_reason: None,
                resources: vec![StackResource {
                    logical_resource_id: "B".into(),
                    physical_resource_id: Some("phys".into()),
                    resource_type: "AWS::S3::Bucket".into(),
                    resource_status: "CREATE_COMPLETE".into(),
                    resource_status_reason: None,
                    timestamp: now_iso8601(),
                    deletion_policy: None,
                }],
                events: Vec::new(),
                change_sets: HashMap::new(),
                created_at: now_iso8601(),
                updated_at: None,
                outputs: HashMap::new(),
                termination_protection: false,
                notification_arns: Vec::new(),
                on_failure: "ROLLBACK".to_string(),
                stack_policy_body: None,
            },
        );

        create_change_set(
            &state,
            &json!({
                "StackName": "s",
                "ChangeSetName": "cs",
                "TemplateBody": template_v2,
            }),
            &ctx(),
        )
        .unwrap();

        let described =
            describe_change_set(&state, &json!({"StackName": "s", "ChangeSetName": "cs"})).unwrap();
        let change = &described["Changes"]["member"][0]["ResourceChange"];
        assert_eq!(change["Action"], "Modify");
        assert_eq!(change["Replacement"], "True");
        assert_eq!(change["Scope"][0], "Properties");
    }

    #[test]
    fn modify_with_only_tag_change_marks_replacement_false() {
        let state = CloudFormationState::default();
        let template_v1 = r#"{
          "Resources": {
            "B": {
              "Type": "AWS::S3::Bucket",
              "Properties": {
                "BucketName": "same",
                "Tags": [{ "Key": "env", "Value": "old" }]
              }
            }
          }
        }"#;
        let template_v2 = r#"{
          "Resources": {
            "B": {
              "Type": "AWS::S3::Bucket",
              "Properties": {
                "BucketName": "same",
                "Tags": [{ "Key": "env", "Value": "new" }]
              }
            }
          }
        }"#;

        state.stacks.insert(
            "s".into(),
            Stack {
                stack_id: stack_arn("us-east-1", "000000000000", "s"),
                stack_name: "s".into(),
                template_body: template_v1.to_string(),
                parameters: HashMap::new(),
                tags: HashMap::new(),
                status: "CREATE_COMPLETE".into(),
                status_reason: None,
                resources: vec![StackResource {
                    logical_resource_id: "B".into(),
                    physical_resource_id: Some("phys".into()),
                    resource_type: "AWS::S3::Bucket".into(),
                    resource_status: "CREATE_COMPLETE".into(),
                    resource_status_reason: None,
                    timestamp: now_iso8601(),
                    deletion_policy: None,
                }],
                events: Vec::new(),
                change_sets: HashMap::new(),
                created_at: now_iso8601(),
                updated_at: None,
                outputs: HashMap::new(),
                termination_protection: false,
                notification_arns: Vec::new(),
                on_failure: "ROLLBACK".to_string(),
                stack_policy_body: None,
            },
        );

        create_change_set(
            &state,
            &json!({
                "StackName": "s",
                "ChangeSetName": "tags",
                "TemplateBody": template_v2,
            }),
            &ctx(),
        )
        .unwrap();

        let described =
            describe_change_set(&state, &json!({"StackName": "s", "ChangeSetName": "tags"}))
                .unwrap();
        let change = &described["Changes"]["member"][0]["ResourceChange"];
        assert_eq!(change["Action"], "Modify");
        assert_eq!(change["Replacement"], "False");
        assert_eq!(change["Scope"][0], "Tags");
    }

    #[test]
    fn removed_resource_emitted_as_remove_change() {
        let state = CloudFormationState::default();
        let template_v1 = r#"{
          "Resources": {
            "A": { "Type": "AWS::S3::Bucket" },
            "B": { "Type": "AWS::S3::Bucket" }
          }
        }"#;
        let template_v2 = r#"{
          "Resources": {
            "A": { "Type": "AWS::S3::Bucket" }
          }
        }"#;

        state.stacks.insert(
            "s".into(),
            Stack {
                stack_id: stack_arn("us-east-1", "000000000000", "s"),
                stack_name: "s".into(),
                template_body: template_v1.to_string(),
                parameters: HashMap::new(),
                tags: HashMap::new(),
                status: "CREATE_COMPLETE".into(),
                status_reason: None,
                resources: vec![
                    StackResource {
                        logical_resource_id: "A".into(),
                        physical_resource_id: Some("phys-a".into()),
                        resource_type: "AWS::S3::Bucket".into(),
                        resource_status: "CREATE_COMPLETE".into(),
                        resource_status_reason: None,
                        timestamp: now_iso8601(),
                        deletion_policy: None,
                    },
                    StackResource {
                        logical_resource_id: "B".into(),
                        physical_resource_id: Some("phys-b".into()),
                        resource_type: "AWS::S3::Bucket".into(),
                        resource_status: "CREATE_COMPLETE".into(),
                        resource_status_reason: None,
                        timestamp: now_iso8601(),
                        deletion_policy: None,
                    },
                ],
                events: Vec::new(),
                change_sets: HashMap::new(),
                created_at: now_iso8601(),
                updated_at: None,
                outputs: HashMap::new(),
                termination_protection: false,
                notification_arns: Vec::new(),
                on_failure: "ROLLBACK".to_string(),
                stack_policy_body: None,
            },
        );

        create_change_set(
            &state,
            &json!({
                "StackName": "s",
                "ChangeSetName": "rm",
                "TemplateBody": template_v2,
            }),
            &ctx(),
        )
        .unwrap();

        let described =
            describe_change_set(&state, &json!({"StackName": "s", "ChangeSetName": "rm"})).unwrap();
        let changes = described["Changes"]["member"].as_array().unwrap();
        assert!(
            changes
                .iter()
                .any(|c| c["ResourceChange"]["Action"] == "Remove"
                    && c["ResourceChange"]["LogicalResourceId"] == "B"),
            "{described}"
        );
    }
}
