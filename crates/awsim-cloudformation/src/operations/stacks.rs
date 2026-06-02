use awsim_core::{AwsError, InternalEvent, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::{missing_parameter, stack_already_exists, stack_not_found},
    ids::{new_uuid, now_iso8601, stack_arn},
    state::{CloudFormationState, Stack, StackEvent, StackResource},
    template,
};

use super::{opt_str, parse_parameters, parse_tags, require_str};

fn stack_to_value(stack: &Stack) -> Value {
    // NoEcho parameters are masked at every projection: DescribeStacks,
    // stack events, change-set descriptions. Re-parse the template once
    // to discover the set of masked keys; the simulator describes
    // infrequently enough that the cost is fine.
    let no_echo_keys: std::collections::HashSet<String> =
        match template::validate_and_parse(&stack.template_body, &stack.parameters) {
            Ok(parsed) => parsed
                .parameters
                .iter()
                .filter(|p| p.no_echo)
                .map(|p| p.name.clone())
                .collect(),
            Err(_) => std::collections::HashSet::new(),
        };

    let params: Vec<Value> = stack
        .parameters
        .iter()
        .map(|(k, v)| {
            let surface = if no_echo_keys.contains(k) {
                "****".to_string()
            } else {
                v.clone()
            };
            json!({ "ParameterKey": k, "ParameterValue": surface })
        })
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
    result["EnableTerminationProtection"] = json!(stack.termination_protection);
    if !stack.notification_arns.is_empty() {
        result["NotificationARNs"] = json!({
            "member": stack.notification_arns.clone()
        });
    }
    result["DisableRollback"] = json!(stack.on_failure == "DO_NOTHING");

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
    if let Some(dp) = &r.deletion_policy {
        obj["DeletionPolicy"] = Value::String(dp.clone());
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
/// Merge stack-level tags with resource-level tags into a flat
/// `Vec<{Key, Value}>` in the shape downstream services already
/// understand. AWS gives precedence to the resource declaration when
/// the same key appears on both, so per-template overrides win over
/// stack defaults.
///
/// `properties` is the raw resource Properties block (we look for the
/// CloudFormation-standard `Tags` array of `{Key, Value}` entries).
pub fn effective_resource_tags(
    stack_tags: &HashMap<String, String>,
    properties: &Value,
) -> Vec<Value> {
    let mut merged: Vec<(String, String)> = stack_tags
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    merged.sort_by(|a, b| a.0.cmp(&b.0));

    if let Some(arr) = properties.get("Tags").and_then(Value::as_array) {
        for tag in arr {
            let Some(key) = tag.get("Key").and_then(Value::as_str) else {
                continue;
            };
            let Some(value) = tag.get("Value").and_then(Value::as_str) else {
                continue;
            };
            if let Some(existing) = merged.iter_mut().find(|(k, _)| k == key) {
                existing.1 = value.to_string();
            } else {
                merged.push((key.to_string(), value.to_string()));
            }
        }
    }

    merged
        .into_iter()
        .map(|(k, v)| json!({"Key": k, "Value": v}))
        .collect()
}

/// AWS allows up to 5 NotificationARNs per stack; mirror the cap.
const MAX_NOTIFICATION_ARNS: usize = 5;

/// Parse + validate the `NotificationARNs` array from a CreateStack /
/// UpdateStack input. Returns an empty Vec when absent.
fn parse_notification_arns(input: &Value) -> Result<Vec<String>, AwsError> {
    let arr = input.get("NotificationARNs").and_then(Value::as_array);
    let Some(arr) = arr else {
        return Ok(Vec::new());
    };
    let arns: Vec<String> = arr
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    if arns.len() > MAX_NOTIFICATION_ARNS {
        return Err(AwsError::bad_request(
            "ValidationError",
            format!("NotificationARNs accepts at most {MAX_NOTIFICATION_ARNS} ARNs."),
        ));
    }
    for arn in &arns {
        if !arn.starts_with("arn:aws:sns:") {
            return Err(AwsError::bad_request(
                "ValidationError",
                format!("NotificationARN `{arn}` is not a valid SNS topic ARN."),
            ));
        }
    }
    Ok(arns)
}

/// Parse + validate `StackPolicyBody` from a CreateStack /
/// UpdateStack / SetStackPolicy input. The body must be JSON; CFN
/// uses an IAM-flavoured Statement[] schema. We validate the JSON
/// shape but don't bind it to a typed model — the policy evaluator
/// reads it back ad hoc so missing optional fields stay missing.
fn parse_stack_policy(input: &Value) -> Result<Option<String>, AwsError> {
    let Some(raw) = input.get("StackPolicyBody").and_then(Value::as_str) else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Ok(None);
    }
    let parsed: Value = serde_json::from_str(raw).map_err(|e| {
        AwsError::bad_request(
            "ValidationError",
            format!("StackPolicyBody is not valid JSON: {e}"),
        )
    })?;
    if !parsed.is_object() {
        return Err(AwsError::bad_request(
            "ValidationError",
            "StackPolicyBody must be a JSON object.",
        ));
    }
    Ok(Some(raw.to_string()))
}

/// Evaluate the stored stack policy against a list of resource
/// changes. Returns `Err` on the first statement that denies one of
/// the changes. The policy schema mirrors AWS:
/// ```json
/// {
///   "Statement": [{
///     "Effect": "Deny",
///     "Action": "Update:*",
///     "Resource": "LogicalResourceId/Critical*"
///   }]
/// }
/// ```
/// Action wildcards: `Update:Modify`, `Update:Replace`,
/// `Update:Delete`, `Update:*`, `*`. Resource patterns are glob-style
/// against the logical id with `*` as the wildcard char. Allow
/// statements are treated as overrides over a default-deny, matching
/// CFN's evaluation semantics — but only when at least one Deny
/// applies. Stacks without a policy treat every update as allowed.
fn evaluate_stack_policy(
    policy_body: &str,
    change_action: &str,
    logical_id: &str,
) -> Result<(), AwsError> {
    let policy: Value = serde_json::from_str(policy_body).map_err(|e| {
        AwsError::bad_request(
            "ValidationError",
            format!("Stored stack policy is not valid JSON: {e}"),
        )
    })?;
    let Some(statements) = policy.get("Statement").and_then(Value::as_array) else {
        return Ok(());
    };

    let mut explicit_allow = false;
    let mut explicit_deny = false;
    for stmt in statements {
        let effect = stmt.get("Effect").and_then(Value::as_str).unwrap_or("Deny");
        let action = stmt.get("Action").and_then(Value::as_str).unwrap_or("*");
        let resource = stmt.get("Resource").and_then(Value::as_str).unwrap_or("*");
        if !action_matches(action, change_action) {
            continue;
        }
        if !resource_matches(resource, logical_id) {
            continue;
        }
        match effect {
            "Allow" => explicit_allow = true,
            "Deny" => explicit_deny = true,
            _ => {}
        }
    }
    if explicit_deny && !explicit_allow {
        return Err(AwsError::bad_request(
            "ValidationError",
            format!("Stack policy denies {change_action} on resource {logical_id}."),
        ));
    }
    Ok(())
}

fn action_matches(pattern: &str, action: &str) -> bool {
    if pattern == "*" || pattern == action {
        return true;
    }
    // `Update:*` matches any `Update:Foo`.
    if let Some(prefix) = pattern.strip_suffix(":*")
        && let Some(act_prefix) = action.split(':').next()
    {
        return prefix == act_prefix;
    }
    false
}

fn resource_matches(pattern: &str, logical_id: &str) -> bool {
    // Patterns are either bare globs like `Critical*` or prefixed
    // `LogicalResourceId/Critical*` per AWS's documented syntax.
    let stripped = pattern
        .strip_prefix("LogicalResourceId/")
        .unwrap_or(pattern);
    glob_matches(stripped, logical_id)
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return value.starts_with(prefix);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return value.ends_with(suffix);
    }
    pattern == value
}

pub fn set_stack_policy(state: &CloudFormationState, input: &Value) -> Result<Value, AwsError> {
    let stack_name = require_str(input, "StackName")?;
    let mut stack = state
        .stacks
        .get_mut(stack_name)
        .ok_or_else(|| stack_not_found(stack_name))?;
    let body = parse_stack_policy(input)?;
    stack.stack_policy_body = body;
    Ok(json!({}))
}

pub fn get_stack_policy(state: &CloudFormationState, input: &Value) -> Result<Value, AwsError> {
    let stack_name = require_str(input, "StackName")?;
    let stack = state
        .stacks
        .get(stack_name)
        .ok_or_else(|| stack_not_found(stack_name))?;
    Ok(match stack.stack_policy_body.as_ref() {
        Some(body) => json!({ "StackPolicyBody": body }),
        None => json!({}),
    })
}

/// Parse + validate `OnFailure`. CFN documents three values
/// (`DO_NOTHING`, `ROLLBACK`, `DELETE`) and also accepts
/// `DisableRollback=true` as a synonym for `DO_NOTHING`. The two
/// inputs are mutually exclusive in real AWS; the simulator rejects
/// the conflict.
fn parse_on_failure(input: &Value) -> Result<String, AwsError> {
    let supplied = input.get("OnFailure").and_then(Value::as_str);
    let disable_rollback = input.get("DisableRollback").and_then(Value::as_bool);

    if supplied.is_some() && disable_rollback.is_some() {
        return Err(AwsError::bad_request(
            "ValidationError",
            "OnFailure and DisableRollback cannot both be specified.",
        ));
    }
    if let Some(v) = supplied {
        return match v {
            "DO_NOTHING" | "ROLLBACK" | "DELETE" => Ok(v.to_string()),
            other => Err(AwsError::bad_request(
                "ValidationError",
                format!("OnFailure `{other}` must be DO_NOTHING, ROLLBACK, or DELETE."),
            )),
        };
    }
    if let Some(true) = disable_rollback {
        return Ok("DO_NOTHING".to_string());
    }
    Ok("ROLLBACK".to_string())
}

/// Publish a stack-level status event to each NotificationARN as an
/// internal `sns:Publish` so downstream subscribers see the message.
/// CFN's documented payload is a CloudFormation-style key=value text
/// blob; we synthesise the most common keys.
fn publish_stack_event_notifications(
    ctx: &RequestContext,
    stack_name: &str,
    stack_id: &str,
    status: &str,
    timestamp: &str,
    notification_arns: &[String],
) {
    let Some(ref bus) = ctx.event_bus else { return };
    for topic_arn in notification_arns {
        let message = format!(
            "StackId='{stack_id}'\nTimestamp='{timestamp}'\nResourceStatus='{status}'\n\
             LogicalResourceId='{stack_name}'\nPhysicalResourceId='{stack_id}'\n\
             ResourceType='AWS::CloudFormation::Stack'\nStackName='{stack_name}'\n",
        );
        bus.publish(InternalEvent {
            source: "cloudformation".to_string(),
            event_type: "sns:Publish".to_string(),
            region: ctx.region.clone(),
            account_id: ctx.account_id.clone(),
            detail: json!({
                "topic_arn": topic_arn,
                "message": message,
                "subject": format!("AWS CloudFormation Notification — {status}"),
            }),
        });
    }
}

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
                deletion_policy: r.deletion_policy.clone(),
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

/// CloudFormation requires callers to opt-in to powerful template
/// constructs via Capabilities. The three documented requirements are:
///   * `CAPABILITY_IAM` / `CAPABILITY_NAMED_IAM` for AWS::IAM::* resources
///     (named when an IAM resource carries an explicit Name property)
///   * `CAPABILITY_AUTO_EXPAND` when the template uses a Transform
///     directive (AWS::Serverless, AWS::Include, custom)
fn validate_capabilities(
    parsed: &template::ParsedTemplate,
    template_body: &str,
    supplied: &[String],
) -> Result<(), AwsError> {
    let has_iam = parsed
        .resources
        .iter()
        .any(|r| r.resource_type.starts_with("AWS::IAM::"));
    let needs_named_iam = parsed.resources.iter().any(|r| {
        r.resource_type.starts_with("AWS::IAM::")
            && r.properties
                .get(match r.resource_type.as_str() {
                    "AWS::IAM::Role" => "RoleName",
                    "AWS::IAM::User" => "UserName",
                    "AWS::IAM::Group" => "GroupName",
                    "AWS::IAM::Policy" => "PolicyName",
                    "AWS::IAM::ManagedPolicy" => "ManagedPolicyName",
                    _ => "",
                })
                .is_some()
    });
    let has_transform =
        template_body.contains("\"Transform\"") || template_body.contains("Transform:");

    let has_cap = |c: &str| supplied.iter().any(|x| x == c);

    if needs_named_iam && !has_cap("CAPABILITY_NAMED_IAM") {
        return Err(AwsError::bad_request(
            "InsufficientCapabilitiesException",
            "Requires capabilities : [CAPABILITY_NAMED_IAM]",
        ));
    }
    if has_iam && !has_cap("CAPABILITY_IAM") && !has_cap("CAPABILITY_NAMED_IAM") {
        return Err(AwsError::bad_request(
            "InsufficientCapabilitiesException",
            "Requires capabilities : [CAPABILITY_IAM]",
        ));
    }
    if has_transform && !has_cap("CAPABILITY_AUTO_EXPAND") {
        return Err(AwsError::bad_request(
            "InsufficientCapabilitiesException",
            "Requires capabilities : [CAPABILITY_AUTO_EXPAND]",
        ));
    }
    Ok(())
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

    // Validate Capabilities against what the template actually requires.
    let supplied_capabilities: Vec<String> = input["Capabilities"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    validate_capabilities(&parsed, &template_body, &supplied_capabilities)?;

    let now = now_iso8601();
    let stack_id = stack_arn(&ctx.partition, &ctx.region, &ctx.account_id, &stack_name);
    let resources = build_resources(&parsed, &now);
    let events = build_events(&resources, &stack_id, &stack_name, &now);

    let termination_protection = input["EnableTerminationProtection"]
        .as_bool()
        .unwrap_or(false);
    let notification_arns = parse_notification_arns(input)?;
    let on_failure = parse_on_failure(input)?;
    let stack_policy_body = parse_stack_policy(input)?;

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
        created_at: now.clone(),
        updated_at: None,
        outputs: HashMap::new(),
        termination_protection,
        notification_arns: notification_arns.clone(),
        on_failure,
        stack_policy_body,
    };

    publish_stack_event_notifications(
        ctx,
        &stack_name,
        &stack_id,
        "CREATE_COMPLETE",
        &now,
        &notification_arns,
    );

    // Emit one CreateResource event per resource so the background router
    // can provision each resource in the appropriate service. Stack
    // tags are merged into each resource's tag set so downstream
    // services see them as if they were declared on the resource.
    if let Some(ref bus) = ctx.event_bus {
        for resource in &stack.resources {
            // Find the matching parsed resource to get its properties.
            let properties = parsed
                .resources
                .iter()
                .find(|r| r.logical_id == resource.logical_resource_id)
                .map(|r| r.properties.clone())
                .unwrap_or(Value::Object(serde_json::Map::new()));

            let effective_tags = effective_resource_tags(&stack.tags, &properties);

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
                    "stackTags": stack.tags,
                    "effectiveTags": effective_tags,
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

    if let Some(stack) = state.stacks.get(stack_name)
        && stack.termination_protection
    {
        return Err(AwsError::bad_request(
            "ValidationError",
            format!(
                "Stack [{stack_name}] cannot be deleted while TerminationProtection is enabled."
            ),
        ));
    }

    if let Some((_, mut stack)) = state.stacks.remove(stack_name) {
        // Resources tagged `DeletionPolicy=Retain` survive the
        // DeleteStack: AWS keeps the underlying resource around and
        // surfaces it on the stack record with `DELETE_SKIPPED`
        // status. The DeleteResource event is suppressed so
        // downstream service crates don't tear them down either.
        if let Some(ref bus) = ctx.event_bus {
            for resource in &stack.resources {
                if resource.deletion_policy.as_deref() == Some("Retain") {
                    continue;
                }
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

        let now = now_iso8601();
        for resource in &mut stack.resources {
            if resource.deletion_policy.as_deref() == Some("Retain") {
                resource.resource_status = "DELETE_SKIPPED".to_string();
                resource.resource_status_reason =
                    Some("Resource retained due to DeletionPolicy.".to_string());
            } else {
                resource.resource_status = "DELETE_COMPLETE".to_string();
            }
            resource.timestamp = now.clone();
        }

        // Mark as DELETE_COMPLETE (keep entry with status for ListStacks)
        stack.status = "DELETE_COMPLETE".to_string();
        stack.updated_at = Some(now.clone());
        publish_stack_event_notifications(
            ctx,
            stack_name,
            &stack.stack_id,
            "DELETE_COMPLETE",
            &now,
            &stack.notification_arns,
        );
        state.stacks.insert(stack_name.to_string(), stack);
    }
    // DeleteStack is idempotent — no error if not found

    Ok(json!({}))
}

pub fn update_stack(
    state: &CloudFormationState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stack_name = require_str(input, "StackName")?;
    let new_notification_arns = parse_notification_arns(input)?;

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

    // Stack policy enforcement: when the stack has a policy attached,
    // diff every resource on the way in and confirm the policy allows
    // each change. AWS evaluates this *before* any state mutation, so
    // a denial blocks the update entirely.
    if let Some(ref policy) = stack.stack_policy_body {
        let prior_parsed =
            template::validate_and_parse(&stack.template_body, &stack.parameters).ok();
        let prior_resources: Vec<&template::ResourceDef> = prior_parsed
            .as_ref()
            .map(|p| p.resources.iter().collect())
            .unwrap_or_default();
        for r in &parsed.resources {
            let prior = prior_resources
                .iter()
                .find(|er| er.logical_id == r.logical_id);
            let change_action = match prior {
                Some(prior) if prior.properties == r.properties => continue,
                Some(_) => "Update:Modify",
                None => continue, // Adds are not stack-policy gated.
            };
            evaluate_stack_policy(policy, change_action, &r.logical_id)?;
        }
        for prior in &prior_resources {
            if !parsed
                .resources
                .iter()
                .any(|r| r.logical_id == prior.logical_id)
            {
                evaluate_stack_policy(policy, "Update:Delete", &prior.logical_id)?;
            }
        }
    }

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
    stack.updated_at = Some(now.clone());
    // UpdateStack replaces NotificationARNs only when the caller
    // supplied a non-empty list; an absent / empty list keeps the
    // existing topics. Mirrors AWS's CloudFormation semantics where
    // omitting a parameter on update leaves the prior value in place.
    if !new_notification_arns.is_empty() {
        stack.notification_arns = new_notification_arns;
    }

    let stack_id = stack.stack_id.clone();
    let topics = stack.notification_arns.clone();
    drop(stack);
    publish_stack_event_notifications(ctx, stack_name, &stack_id, "UPDATE_COMPLETE", &now, &topics);
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

    if let Some(name) = filter_name
        && stacks.is_empty()
    {
        return Err(stack_not_found(name));
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
        .filter(|entry| status_filters.is_empty() || status_filters.contains(&entry.status))
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
pub fn get_template_summary(
    _state: &CloudFormationState,
    input: &Value,
) -> Result<Value, AwsError> {
    let template_body =
        opt_str(input, "TemplateBody").ok_or_else(|| missing_parameter("TemplateBody"))?;

    let parsed = template::validate_and_parse(template_body, &HashMap::new())?;

    let params: Vec<Value> = parsed
        .parameters
        .iter()
        .map(|p| {
            let mut obj = json!({
                "ParameterKey": p.name,
                "ParameterType": p.param_type,
                "NoEcho": p.no_echo,
            });
            if let Some(desc) = &p.description {
                obj["Description"] = Value::String(desc.clone());
            }
            if let Some(default) = &p.default {
                // NoEcho parameters mask the default the same way they
                // mask the supplied value in stack events.
                let surface = if p.no_echo {
                    "****".to_string()
                } else {
                    default.clone()
                };
                obj["DefaultValue"] = Value::String(surface);
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
pub fn list_stack_resources(state: &CloudFormationState, input: &Value) -> Result<Value, AwsError> {
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

    let mut entry = state.stack_tags.entry(stack_name.to_string()).or_default();
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

/// UpdateTerminationProtection toggles the stack-level deletion guard.
/// AWS rejects unknown stacks with ValidationError, not NotFound.
pub fn update_termination_protection(
    state: &CloudFormationState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stack_name = require_str(input, "StackName")?;
    let enabled = input["EnableTerminationProtection"]
        .as_bool()
        .ok_or_else(|| missing_parameter("EnableTerminationProtection"))?;
    let mut stack = state.stacks.get_mut(stack_name).ok_or_else(|| {
        AwsError::bad_request(
            "ValidationError",
            format!("Stack [{stack_name}] does not exist."),
        )
    })?;
    stack.termination_protection = enabled;
    Ok(json!({ "StackId": stack.stack_id }))
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
    let template_body =
        opt_str(input, "TemplateBody").ok_or_else(|| missing_parameter("TemplateBody"))?;

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

#[cfg(test)]
mod termination_protection_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("cloudformation", "us-east-1")
    }

    fn create(state: &CloudFormationState, protect: bool) {
        let template = r#"{"Resources":{"R":{"Type":"AWS::S3::Bucket"}}}"#;
        let mut input = json!({
            "StackName": "s1",
            "TemplateBody": template,
        });
        if protect {
            input["EnableTerminationProtection"] = json!(true);
        }
        create_stack(state, &input, &ctx()).unwrap();
    }

    #[test]
    fn delete_stack_blocked_when_termination_protection_on() {
        let state = CloudFormationState::default();
        create(&state, true);
        let err = delete_stack(&state, &json!({ "StackName": "s1" }), &ctx()).unwrap_err();
        assert_eq!(err.code, "ValidationError");
        assert!(err.message.contains("TerminationProtection"));
    }

    #[test]
    fn delete_stack_succeeds_after_disabling_protection() {
        let state = CloudFormationState::default();
        create(&state, true);
        update_termination_protection(
            &state,
            &json!({ "StackName": "s1", "EnableTerminationProtection": false }),
            &ctx(),
        )
        .unwrap();
        delete_stack(&state, &json!({ "StackName": "s1" }), &ctx()).unwrap();
    }

    #[test]
    fn update_termination_protection_validates_unknown_stack() {
        let state = CloudFormationState::default();
        let err = update_termination_protection(
            &state,
            &json!({ "StackName": "missing", "EnableTerminationProtection": true }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationError");
    }

    #[test]
    fn describe_surfaces_enable_termination_protection() {
        let state = CloudFormationState::default();
        create(&state, true);
        let resp = describe_stacks(&state, &json!({ "StackName": "s1" })).unwrap();
        let stacks = &resp["Stacks"]["member"];
        assert_eq!(stacks[0]["EnableTerminationProtection"], true);
    }
}

#[cfg(test)]
mod capabilities_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("cloudformation", "us-east-1")
    }

    const IAM_TEMPLATE: &str = r#"{
        "Resources": {
            "R": { "Type": "AWS::IAM::Role" }
        }
    }"#;

    const NAMED_IAM_TEMPLATE: &str = r#"{
        "Resources": {
            "R": { "Type": "AWS::IAM::Role", "Properties": { "RoleName": "named" } }
        }
    }"#;

    const SAM_TEMPLATE: &str = r#"{
        "Transform": "AWS::Serverless-2016-10-31",
        "Resources": {
            "F": { "Type": "AWS::Serverless::Function" }
        }
    }"#;

    #[test]
    fn iam_template_without_capability_is_rejected() {
        let state = CloudFormationState::default();
        let err = create_stack(
            &state,
            &json!({ "StackName": "s", "TemplateBody": IAM_TEMPLATE }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InsufficientCapabilitiesException");
        assert!(err.message.contains("CAPABILITY_IAM"));
    }

    #[test]
    fn iam_template_with_capability_iam_is_accepted() {
        let state = CloudFormationState::default();
        create_stack(
            &state,
            &json!({
                "StackName": "s",
                "TemplateBody": IAM_TEMPLATE,
                "Capabilities": ["CAPABILITY_IAM"],
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn named_iam_template_requires_named_capability() {
        let state = CloudFormationState::default();
        let err = create_stack(
            &state,
            &json!({
                "StackName": "s",
                "TemplateBody": NAMED_IAM_TEMPLATE,
                "Capabilities": ["CAPABILITY_IAM"],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InsufficientCapabilitiesException");
        assert!(err.message.contains("CAPABILITY_NAMED_IAM"));
    }

    #[test]
    fn transform_template_requires_auto_expand() {
        let state = CloudFormationState::default();
        let err = create_stack(
            &state,
            &json!({ "StackName": "s", "TemplateBody": SAM_TEMPLATE }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InsufficientCapabilitiesException");
        assert!(err.message.contains("CAPABILITY_AUTO_EXPAND"));
    }

    #[test]
    fn effective_tags_merge_stack_and_resource_with_resource_override() {
        let mut stack_tags = HashMap::new();
        stack_tags.insert("env".to_string(), "stack-prod".to_string());
        stack_tags.insert("owner".to_string(), "stack-team".to_string());

        let properties = json!({
            "Tags": [
                { "Key": "env", "Value": "resource-override" },
                { "Key": "extra", "Value": "from-resource" }
            ]
        });

        let merged = effective_resource_tags(&stack_tags, &properties);
        // Stack `owner` survives, resource `env` overrides, `extra` is new.
        assert!(
            merged
                .iter()
                .any(|t| t["Key"] == "owner" && t["Value"] == "stack-team")
        );
        assert!(
            merged
                .iter()
                .any(|t| t["Key"] == "env" && t["Value"] == "resource-override")
        );
        assert!(
            merged
                .iter()
                .any(|t| t["Key"] == "extra" && t["Value"] == "from-resource")
        );
    }

    #[test]
    fn effective_tags_with_no_resource_tags_carry_stack_tags() {
        let mut stack_tags = HashMap::new();
        stack_tags.insert("env".to_string(), "prod".to_string());
        let merged = effective_resource_tags(&stack_tags, &json!({}));
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0]["Key"], "env");
        assert_eq!(merged[0]["Value"], "prod");
    }

    #[test]
    fn effective_tags_skips_malformed_resource_tag_entries() {
        let stack_tags = HashMap::new();
        let properties = json!({
            "Tags": [
                { "Key": "good", "Value": "v" },
                { "Key": "missing-value" },
                { "Value": "missing-key" }
            ]
        });
        let merged = effective_resource_tags(&stack_tags, &properties);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0]["Key"], "good");
    }

    #[test]
    fn delete_stack_retains_resources_with_retain_policy() {
        let state = CloudFormationState::default();
        let template = r#"{
          "Resources": {
            "Keep": {
              "Type": "AWS::S3::Bucket",
              "DeletionPolicy": "Retain"
            },
            "Drop": {
              "Type": "AWS::S3::Bucket"
            }
          }
        }"#;
        create_stack(
            &state,
            &json!({"StackName": "s", "TemplateBody": template}),
            &ctx(),
        )
        .unwrap();
        delete_stack(&state, &json!({"StackName": "s"}), &ctx()).unwrap();

        let described = describe_stacks(&state, &json!({"StackName": "s"})).unwrap();
        let stack = &described["Stacks"]["member"][0];
        assert_eq!(stack["StackStatus"], "DELETE_COMPLETE");

        let resources = list_stack_resources(&state, &json!({"StackName": "s"})).unwrap();
        let summaries = resources["StackResourceSummaries"]["member"]
            .as_array()
            .unwrap();
        let keep = summaries
            .iter()
            .find(|r| r["LogicalResourceId"] == "Keep")
            .expect("retained resource visible");
        assert_eq!(keep["ResourceStatus"], "DELETE_SKIPPED");
        let drop = summaries
            .iter()
            .find(|r| r["LogicalResourceId"] == "Drop")
            .expect("non-retained resource visible");
        assert_eq!(drop["ResourceStatus"], "DELETE_COMPLETE");
    }

    #[test]
    fn template_with_invalid_deletion_policy_is_rejected() {
        let state = CloudFormationState::default();
        let template = r#"{
          "Resources": {
            "X": {
              "Type": "AWS::S3::Bucket",
              "DeletionPolicy": "Forever"
            }
          }
        }"#;
        let err = create_stack(
            &state,
            &json!({"StackName": "bad", "TemplateBody": template}),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationError");
    }

    const MINIMAL_TEMPLATE: &str = r#"{
      "Resources": {
        "B": { "Type": "AWS::S3::Bucket" }
      }
    }"#;

    #[test]
    fn create_stack_persists_notification_arns() {
        let state = CloudFormationState::default();
        create_stack(
            &state,
            &json!({
                "StackName": "s",
                "TemplateBody": MINIMAL_TEMPLATE,
                "NotificationARNs": [
                    "arn:aws:sns:us-east-1:000000000000:ops",
                    "arn:aws:sns:us-east-1:000000000000:audit",
                ],
            }),
            &ctx(),
        )
        .unwrap();
        let described = describe_stacks(&state, &json!({"StackName": "s"})).unwrap();
        let arns = described["Stacks"]["member"][0]["NotificationARNs"]["member"]
            .as_array()
            .unwrap();
        assert_eq!(arns.len(), 2);
    }

    #[test]
    fn create_stack_rejects_non_sns_notification_arn() {
        let state = CloudFormationState::default();
        let err = create_stack(
            &state,
            &json!({
                "StackName": "s",
                "TemplateBody": MINIMAL_TEMPLATE,
                "NotificationARNs": ["arn:aws:sqs:us-east-1:000000000000:q"],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationError");
    }

    #[test]
    fn create_stack_rejects_more_than_five_notification_arns() {
        let state = CloudFormationState::default();
        let arns: Vec<String> = (0..6)
            .map(|i| format!("arn:aws:sns:us-east-1:000000000000:t{i}"))
            .collect();
        let err = create_stack(
            &state,
            &json!({
                "StackName": "s",
                "TemplateBody": MINIMAL_TEMPLATE,
                "NotificationARNs": arns,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationError");
    }

    #[test]
    fn update_stack_can_replace_notification_arns() {
        let state = CloudFormationState::default();
        create_stack(
            &state,
            &json!({
                "StackName": "s",
                "TemplateBody": MINIMAL_TEMPLATE,
                "NotificationARNs": ["arn:aws:sns:us-east-1:000000000000:initial"],
            }),
            &ctx(),
        )
        .unwrap();
        update_stack(
            &state,
            &json!({
                "StackName": "s",
                "TemplateBody": MINIMAL_TEMPLATE,
                "NotificationARNs": ["arn:aws:sns:us-east-1:000000000000:replaced"],
            }),
            &ctx(),
        )
        .unwrap();
        let described = describe_stacks(&state, &json!({"StackName": "s"})).unwrap();
        let arns = described["Stacks"]["member"][0]["NotificationARNs"]["member"]
            .as_array()
            .unwrap();
        assert_eq!(arns.len(), 1);
        assert_eq!(arns[0], "arn:aws:sns:us-east-1:000000000000:replaced");
    }

    #[test]
    fn on_failure_defaults_to_rollback() {
        let v = parse_on_failure(&json!({})).unwrap();
        assert_eq!(v, "ROLLBACK");
    }

    #[test]
    fn on_failure_accepts_all_documented_values() {
        for v in ["DO_NOTHING", "ROLLBACK", "DELETE"] {
            assert_eq!(
                parse_on_failure(&json!({"OnFailure": v})).unwrap(),
                v,
                "OnFailure={v}"
            );
        }
    }

    #[test]
    fn on_failure_rejects_unknown_value() {
        let err = parse_on_failure(&json!({"OnFailure": "PANIC"})).unwrap_err();
        assert_eq!(err.code, "ValidationError");
    }

    #[test]
    fn disable_rollback_true_collapses_to_do_nothing() {
        assert_eq!(
            parse_on_failure(&json!({"DisableRollback": true})).unwrap(),
            "DO_NOTHING"
        );
    }

    #[test]
    fn on_failure_and_disable_rollback_are_mutually_exclusive() {
        let err = parse_on_failure(&json!({
            "OnFailure": "DELETE",
            "DisableRollback": true,
        }))
        .unwrap_err();
        assert_eq!(err.code, "ValidationError");
    }

    #[test]
    fn describe_surfaces_disable_rollback_flag_for_do_nothing() {
        let state = CloudFormationState::default();
        create_stack(
            &state,
            &json!({
                "StackName": "s",
                "TemplateBody": MINIMAL_TEMPLATE,
                "OnFailure": "DO_NOTHING",
            }),
            &ctx(),
        )
        .unwrap();
        let described = describe_stacks(&state, &json!({"StackName": "s"})).unwrap();
        assert_eq!(described["Stacks"]["member"][0]["DisableRollback"], true);
    }

    const TEMPLATE_V1: &str = r#"{
      "Resources": {
        "Critical": {
          "Type": "AWS::S3::Bucket",
          "Properties": { "BucketName": "old" }
        }
      }
    }"#;
    const TEMPLATE_V2: &str = r#"{
      "Resources": {
        "Critical": {
          "Type": "AWS::S3::Bucket",
          "Properties": { "BucketName": "new" }
        }
      }
    }"#;
    const DENY_POLICY: &str = r#"{
      "Statement": [
        {
          "Effect": "Deny",
          "Action": "Update:*",
          "Principal": "*",
          "Resource": "LogicalResourceId/Critical*"
        }
      ]
    }"#;

    #[test]
    fn set_stack_policy_persists_body() {
        let state = CloudFormationState::default();
        create_stack(
            &state,
            &json!({"StackName": "s", "TemplateBody": MINIMAL_TEMPLATE}),
            &ctx(),
        )
        .unwrap();
        set_stack_policy(
            &state,
            &json!({"StackName": "s", "StackPolicyBody": DENY_POLICY}),
        )
        .unwrap();
        let out = get_stack_policy(&state, &json!({"StackName": "s"})).unwrap();
        assert!(out["StackPolicyBody"].as_str().is_some());
    }

    #[test]
    fn update_stack_blocked_by_deny_policy() {
        let state = CloudFormationState::default();
        create_stack(
            &state,
            &json!({
                "StackName": "s",
                "TemplateBody": TEMPLATE_V1,
                "StackPolicyBody": DENY_POLICY,
            }),
            &ctx(),
        )
        .unwrap();
        let err = update_stack(
            &state,
            &json!({"StackName": "s", "TemplateBody": TEMPLATE_V2}),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationError");
        assert!(err.message.contains("Critical"), "{err:?}");
    }

    #[test]
    fn update_stack_allowed_when_policy_excludes_resource() {
        let policy = r#"{
          "Statement": [{
            "Effect": "Deny",
            "Action": "Update:*",
            "Resource": "LogicalResourceId/Other*"
          }]
        }"#;
        let state = CloudFormationState::default();
        create_stack(
            &state,
            &json!({
                "StackName": "s",
                "TemplateBody": TEMPLATE_V1,
                "StackPolicyBody": policy,
            }),
            &ctx(),
        )
        .unwrap();
        update_stack(
            &state,
            &json!({"StackName": "s", "TemplateBody": TEMPLATE_V2}),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn stack_policy_must_be_valid_json_object() {
        let state = CloudFormationState::default();
        let err = create_stack(
            &state,
            &json!({
                "StackName": "s",
                "TemplateBody": MINIMAL_TEMPLATE,
                "StackPolicyBody": "not-json",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationError");
    }
}
