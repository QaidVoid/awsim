use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{
    AppAutoScalingState, ScalableTarget, ScalingPolicy, ScheduledAction, policy_key, scheduled_key,
    target_key,
};

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
        .ok_or_else(|| AwsError::bad_request("ValidationException", format!("{key} is required")))
}

fn target_to_value(t: &ScalableTarget) -> Value {
    json!({
        "ServiceNamespace": t.service_namespace,
        "ResourceId": t.resource_id,
        "ScalableDimension": t.scalable_dimension,
        "MinCapacity": t.min_capacity,
        "MaxCapacity": t.max_capacity,
        "RoleARN": t.role_arn,
        "CreationTime": t.creation_time,
        "SuspendedState": t.suspended_state,
    })
}

fn policy_to_value(p: &ScalingPolicy) -> Value {
    json!({
        "PolicyName": p.policy_name,
        "PolicyARN": p.policy_arn,
        "ServiceNamespace": p.service_namespace,
        "ResourceId": p.resource_id,
        "ScalableDimension": p.scalable_dimension,
        "PolicyType": p.policy_type,
        "StepScalingPolicyConfiguration": p.step_scaling_policy_configuration,
        "TargetTrackingScalingPolicyConfiguration": p.target_tracking_scaling_policy_configuration,
        "CreationTime": p.creation_time,
        "Alarms": p.alarms,
    })
}

fn scheduled_to_value(a: &ScheduledAction) -> Value {
    json!({
        "ScheduledActionName": a.scheduled_action_name,
        "ScheduledActionARN": a.scheduled_action_arn,
        "ServiceNamespace": a.service_namespace,
        "Schedule": a.schedule,
        "Timezone": a.timezone,
        "ResourceId": a.resource_id,
        "ScalableDimension": a.scalable_dimension,
        "StartTime": a.start_time,
        "EndTime": a.end_time,
        "ScalableTargetAction": a.scalable_target_action,
        "CreationTime": a.creation_time,
    })
}

pub fn register_scalable_target(
    state: &AppAutoScalingState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ns = require_str(input, "ServiceNamespace")?.to_string();
    let rid = require_str(input, "ResourceId")?.to_string();
    let dim = require_str(input, "ScalableDimension")?.to_string();
    let key = target_key(&ns, &rid, &dim);

    let existing = state.targets.get(&key).map(|e| e.value().clone());
    let t = ScalableTarget {
        service_namespace: ns,
        resource_id: rid,
        scalable_dimension: dim,
        min_capacity: input
            .get("MinCapacity")
            .and_then(|v| v.as_i64())
            .map(|v| v as i32)
            .or(existing.as_ref().map(|e| e.min_capacity))
            .unwrap_or(1),
        max_capacity: input
            .get("MaxCapacity")
            .and_then(|v| v.as_i64())
            .map(|v| v as i32)
            .or(existing.as_ref().map(|e| e.max_capacity))
            .unwrap_or(1),
        role_arn: input
            .get("RoleARN")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| existing.as_ref().map(|e| e.role_arn.clone()))
            .unwrap_or_else(|| {
                "arn:aws:iam::000000000000:role/aws-service-role/application-autoscaling.amazonaws.com/AWSServiceRoleForApplicationAutoScaling".to_string()
            }),
        creation_time: existing.as_ref().map(|e| e.creation_time).unwrap_or_else(now_secs),
        suspended_state: input.get("SuspendedState").cloned().or_else(|| existing.and_then(|e| e.suspended_state)),
    };
    let result = json!({ "ScalableTargetARN": format!(
        "arn:aws:application-autoscaling:::scalable-target/{}/{}/{}",
        t.service_namespace, t.resource_id, t.scalable_dimension
    )});
    state.targets.insert(key, t);
    Ok(result)
}

pub fn deregister_scalable_target(
    state: &AppAutoScalingState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ns = require_str(input, "ServiceNamespace")?;
    let rid = require_str(input, "ResourceId")?;
    let dim = require_str(input, "ScalableDimension")?;
    let key = target_key(ns, rid, dim);
    state.targets.remove(&key).ok_or_else(|| {
        AwsError::not_found(
            "ObjectNotFoundException",
            format!("Scalable target {key} not found"),
        )
    })?;
    // Cascade-delete policies and scheduled actions for this target.
    let prefix = format!("{key}|");
    state.policies.retain(|k, _| !k.starts_with(&prefix));
    state
        .scheduled_actions
        .retain(|k, _| !k.starts_with(&prefix));
    Ok(json!({}))
}

pub fn describe_scalable_targets(
    state: &AppAutoScalingState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ns = require_str(input, "ServiceNamespace")?;
    let resource_ids = input
        .get("ResourceIds")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        });
    let dim = input.get("ScalableDimension").and_then(|v| v.as_str());
    let items: Vec<Value> = state
        .targets
        .iter()
        .filter(|e| {
            let t = e.value();
            if t.service_namespace != ns {
                return false;
            }
            if let Some(rids) = &resource_ids
                && !rids.is_empty()
                && !rids.iter().any(|r| r == &t.resource_id)
            {
                return false;
            }
            if let Some(d) = dim
                && t.scalable_dimension != d
            {
                return false;
            }
            true
        })
        .map(|e| target_to_value(e.value()))
        .collect();
    Ok(json!({ "ScalableTargets": items }))
}

pub fn put_scaling_policy(
    state: &AppAutoScalingState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "PolicyName")?.to_string();
    let ns = require_str(input, "ServiceNamespace")?.to_string();
    let rid = require_str(input, "ResourceId")?.to_string();
    let dim = require_str(input, "ScalableDimension")?.to_string();
    if !state.targets.contains_key(&target_key(&ns, &rid, &dim)) {
        return Err(AwsError::not_found(
            "ObjectNotFoundException",
            "Register the scalable target before attaching a policy",
        ));
    }
    let arn = format!(
        "arn:aws:autoscaling:{}:{}:scalingPolicy:{}:resource/{}/{}/{}:policyName/{}",
        ctx.region,
        ctx.account_id,
        uuid::Uuid::new_v4(),
        ns,
        rid,
        dim,
        name
    );
    let p = ScalingPolicy {
        policy_name: name.clone(),
        policy_arn: arn.clone(),
        service_namespace: ns.clone(),
        resource_id: rid.clone(),
        scalable_dimension: dim.clone(),
        policy_type: input
            .get("PolicyType")
            .and_then(|v| v.as_str())
            .unwrap_or("TargetTrackingScaling")
            .to_string(),
        step_scaling_policy_configuration: input.get("StepScalingPolicyConfiguration").cloned(),
        target_tracking_scaling_policy_configuration: input
            .get("TargetTrackingScalingPolicyConfiguration")
            .cloned(),
        creation_time: now_secs(),
        alarms: vec![],
    };
    state.policies.insert(policy_key(&ns, &rid, &dim, &name), p);
    Ok(json!({ "PolicyARN": arn, "Alarms": [] }))
}

pub fn delete_scaling_policy(
    state: &AppAutoScalingState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "PolicyName")?;
    let ns = require_str(input, "ServiceNamespace")?;
    let rid = require_str(input, "ResourceId")?;
    let dim = require_str(input, "ScalableDimension")?;
    state
        .policies
        .remove(&policy_key(ns, rid, dim, name))
        .ok_or_else(|| {
            AwsError::not_found(
                "ObjectNotFoundException",
                format!("Policy {name} not found"),
            )
        })?;
    Ok(json!({}))
}

pub fn describe_scaling_policies(
    state: &AppAutoScalingState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ns = require_str(input, "ServiceNamespace")?;
    let rid = input.get("ResourceId").and_then(|v| v.as_str());
    let dim = input.get("ScalableDimension").and_then(|v| v.as_str());
    let names: Option<Vec<String>> =
        input
            .get("PolicyNames")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });

    let items: Vec<Value> = state
        .policies
        .iter()
        .filter(|e| {
            let p = e.value();
            if p.service_namespace != ns {
                return false;
            }
            if let Some(r) = rid
                && p.resource_id != r
            {
                return false;
            }
            if let Some(d) = dim
                && p.scalable_dimension != d
            {
                return false;
            }
            if let Some(ns) = &names
                && !ns.is_empty()
                && !ns.iter().any(|n| n == &p.policy_name)
            {
                return false;
            }
            true
        })
        .map(|e| policy_to_value(e.value()))
        .collect();
    Ok(json!({ "ScalingPolicies": items }))
}

pub fn put_scheduled_action(
    state: &AppAutoScalingState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "ScheduledActionName")?.to_string();
    let ns = require_str(input, "ServiceNamespace")?.to_string();
    let rid = require_str(input, "ResourceId")?.to_string();
    let dim = require_str(input, "ScalableDimension")?.to_string();
    let schedule = require_str(input, "Schedule")?.to_string();
    let target_action = input.get("ScalableTargetAction").cloned().ok_or_else(|| {
        AwsError::bad_request("ValidationException", "ScalableTargetAction is required")
    })?;
    let arn = format!(
        "arn:aws:autoscaling:{}:{}:scheduledAction:{}:resource/{}/{}/{}:scheduledActionName/{}",
        ctx.region,
        ctx.account_id,
        uuid::Uuid::new_v4(),
        ns,
        rid,
        dim,
        name
    );
    let a = ScheduledAction {
        scheduled_action_name: name.clone(),
        scheduled_action_arn: arn,
        service_namespace: ns.clone(),
        schedule,
        timezone: input
            .get("Timezone")
            .and_then(|v| v.as_str())
            .map(String::from),
        resource_id: rid.clone(),
        scalable_dimension: dim.clone(),
        start_time: input.get("StartTime").and_then(|v| v.as_f64()),
        end_time: input.get("EndTime").and_then(|v| v.as_f64()),
        scalable_target_action: target_action,
        creation_time: now_secs(),
    };
    state
        .scheduled_actions
        .insert(scheduled_key(&ns, &rid, &dim, &name), a);
    Ok(json!({}))
}

pub fn delete_scheduled_action(
    state: &AppAutoScalingState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "ScheduledActionName")?;
    let ns = require_str(input, "ServiceNamespace")?;
    let rid = require_str(input, "ResourceId")?;
    let dim = require_str(input, "ScalableDimension")?;
    state
        .scheduled_actions
        .remove(&scheduled_key(ns, rid, dim, name))
        .ok_or_else(|| {
            AwsError::not_found(
                "ObjectNotFoundException",
                format!("Scheduled action {name} not found"),
            )
        })?;
    Ok(json!({}))
}

pub fn describe_scheduled_actions(
    state: &AppAutoScalingState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ns = require_str(input, "ServiceNamespace")?;
    let rid = input.get("ResourceId").and_then(|v| v.as_str());
    let dim = input.get("ScalableDimension").and_then(|v| v.as_str());
    let items: Vec<Value> = state
        .scheduled_actions
        .iter()
        .filter(|e| {
            let a = e.value();
            if a.service_namespace != ns {
                return false;
            }
            if let Some(r) = rid
                && a.resource_id != r
            {
                return false;
            }
            if let Some(d) = dim
                && a.scalable_dimension != d
            {
                return false;
            }
            true
        })
        .map(|e| scheduled_to_value(e.value()))
        .collect();
    Ok(json!({ "ScheduledActions": items }))
}

pub fn describe_scaling_activities(
    _state: &AppAutoScalingState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // The emulator never executes scaling actions, so the activity log is
    // always empty. Returning an empty list matches what `aws application-autoscaling`
    // SDK clients expect for newly-registered targets.
    Ok(json!({ "ScalingActivities": [] }))
}
