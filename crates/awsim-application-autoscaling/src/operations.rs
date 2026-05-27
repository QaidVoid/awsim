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

/// AWS Application Auto Scaling supports a closed set of
/// `ServiceNamespace` values. Anything outside this list is rejected
/// with `ValidationException` at `RegisterScalableTarget` time so
/// downstream lookups can assume a known namespace.
pub const SERVICE_NAMESPACES: &[&str] = &[
    "appstream",
    "cassandra",
    "comprehend",
    "custom-resource",
    "dynamodb",
    "ec2",
    "ecs",
    "elasticache",
    "elasticmapreduce",
    "kafka",
    "lambda",
    "neptune",
    "rds",
    "sagemaker",
    "workspaces",
];

pub(crate) fn is_valid_service_namespace(ns: &str) -> bool {
    SERVICE_NAMESPACES.contains(&ns)
}

/// Documented `ScalableDimension` values keyed by `ServiceNamespace`.
/// AWS rejects mismatches with `ValidationException`; the emulator
/// uses the same table so callers stay AWS-compatible.
fn allowed_dimensions(ns: &str) -> &'static [&'static str] {
    match ns {
        "appstream" => &["appstream:fleet:DesiredCapacity"],
        "cassandra" => &[
            "cassandra:table:ReadCapacityUnits",
            "cassandra:table:WriteCapacityUnits",
        ],
        "comprehend" => &[
            "comprehend:document-classifier-endpoint:DesiredInferenceUnits",
            "comprehend:entity-recognizer-endpoint:DesiredInferenceUnits",
        ],
        "custom-resource" => &["custom-resource:ResourceType:Property"],
        "dynamodb" => &[
            "dynamodb:table:ReadCapacityUnits",
            "dynamodb:table:WriteCapacityUnits",
            "dynamodb:index:ReadCapacityUnits",
            "dynamodb:index:WriteCapacityUnits",
        ],
        "ec2" => &[
            "ec2:spot-fleet-request:TargetCapacity",
            "ec2:fleet:TargetCapacity",
        ],
        "ecs" => &["ecs:service:DesiredCount"],
        "elasticache" => &[
            "elasticache:replication-group:NodeGroups",
            "elasticache:replication-group:Replicas",
            "elasticache:cache-cluster:Nodes",
        ],
        "elasticmapreduce" => &["elasticmapreduce:instancegroup:InstanceCount"],
        "kafka" => &["kafka:broker-storage:VolumeSize"],
        "lambda" => &["lambda:function:ProvisionedConcurrency"],
        "neptune" => &["neptune:cluster:ReadReplicaCount"],
        "rds" => &["rds:cluster:ReadReplicaCount", "rds:cluster:Capacity"],
        "sagemaker" => &[
            "sagemaker:variant:DesiredInstanceCount",
            "sagemaker:variant:DesiredProvisionedConcurrency",
            "sagemaker:inference-component:DesiredCopyCount",
        ],
        "workspaces" => &["workspaces:workspacespool:DesiredUserSessions"],
        _ => &[],
    }
}

pub(crate) fn is_valid_dimension_for_namespace(ns: &str, dim: &str) -> bool {
    allowed_dimensions(ns).contains(&dim)
}

/// `TargetTrackingScalingPolicyConfiguration` invariants per AWS:
///   * `TargetValue` must be strictly positive
///   * `ScaleInCooldown` / `ScaleOutCooldown` (when present) must be `>= 0`
///   * exactly one of `PredefinedMetricSpecification` or
///     `CustomizedMetricSpecification` is supplied
pub(crate) fn validate_target_tracking_config(cfg: &Value) -> Result<(), AwsError> {
    let target_value = cfg
        .get("TargetValue")
        .and_then(Value::as_f64)
        .ok_or_else(|| AwsError::bad_request("ValidationException", "TargetValue is required"))?;
    if target_value <= 0.0 || target_value.is_nan() {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("TargetValue `{target_value}` must be strictly positive."),
        ));
    }
    for field in ["ScaleInCooldown", "ScaleOutCooldown"] {
        if let Some(v) = cfg.get(field).and_then(Value::as_i64)
            && v < 0
        {
            return Err(AwsError::bad_request(
                "ValidationException",
                format!("{field} `{v}` must be >= 0."),
            ));
        }
    }
    let has_predef = cfg.get("PredefinedMetricSpecification").is_some();
    let has_custom = cfg.get("CustomizedMetricSpecification").is_some();
    if has_predef == has_custom {
        return Err(AwsError::bad_request(
            "ValidationException",
            "Exactly one of PredefinedMetricSpecification or CustomizedMetricSpecification is required.",
        ));
    }
    if let Some(custom) = cfg.get("CustomizedMetricSpecification") {
        validate_customized_metric_spec(custom)?;
    }
    Ok(())
}

/// AWS-documented allowed statistics for
/// `CustomizedMetricSpecification.Statistic`. `Average` is the
/// default when callers omit it.
const ALLOWED_METRIC_STATISTICS: &[&str] = &["Average", "Minimum", "Maximum", "SampleCount", "Sum"];

/// Validate the shape of a `CustomizedMetricSpecification`. The
/// `Metrics` (compound expression) form is mutually exclusive with the
/// single-metric form; we enforce that exactly one is supplied.
pub(crate) fn validate_customized_metric_spec(spec: &Value) -> Result<(), AwsError> {
    let has_metrics = spec.get("Metrics").is_some();
    let has_single = spec.get("MetricName").is_some()
        || spec.get("Namespace").is_some()
        || spec.get("Statistic").is_some();
    if has_metrics && has_single {
        return Err(AwsError::bad_request(
            "ValidationException",
            "CustomizedMetricSpecification cannot mix Metrics with the single-metric fields.",
        ));
    }
    if !has_metrics {
        // Single-metric form: MetricName, Namespace, Statistic
        // required; Unit optional; Dimensions optional list of
        // {Name, Value}.
        for required in ["MetricName", "Namespace"] {
            let s = spec
                .get(required)
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty());
            if s.is_none() {
                return Err(AwsError::bad_request(
                    "ValidationException",
                    format!("CustomizedMetricSpecification.{required} is required."),
                ));
            }
        }
        let stat = spec
            .get("Statistic")
            .and_then(Value::as_str)
            .unwrap_or("Average");
        if !ALLOWED_METRIC_STATISTICS.contains(&stat) {
            return Err(AwsError::bad_request(
                "ValidationException",
                format!(
                    "CustomizedMetricSpecification.Statistic `{stat}` must be one of {ALLOWED_METRIC_STATISTICS:?}.",
                ),
            ));
        }
        if let Some(dims) = spec.get("Dimensions").and_then(Value::as_array) {
            if dims.len() > 30 {
                return Err(AwsError::bad_request(
                    "ValidationException",
                    format!(
                        "CustomizedMetricSpecification.Dimensions has {} entries; the maximum is 30.",
                        dims.len()
                    ),
                ));
            }
            for d in dims {
                let n = d
                    .get("Name")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty());
                let v = d
                    .get("Value")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty());
                if n.is_none() || v.is_none() {
                    return Err(AwsError::bad_request(
                        "ValidationException",
                        "Each Dimension requires non-empty Name and Value.",
                    ));
                }
            }
        }
    }
    Ok(())
}

/// `StepScalingPolicyConfiguration` invariants per AWS:
///   * `AdjustmentType` is one of `ChangeInCapacity`,
///     `PercentChangeInCapacity`, `ExactCapacity`
///   * `MetricAggregationType` (if present) is `Average`, `Minimum`,
///     `Maximum`
///   * `MinAdjustmentMagnitude` (if present) is `>= 0` and only valid
///     for `PercentChangeInCapacity`
///   * `StepAdjustments` ranges (`MetricIntervalLowerBound` /
///     `MetricIntervalUpperBound`) do not overlap
pub(crate) fn validate_step_scaling_config(cfg: &Value) -> Result<(), AwsError> {
    let adjustment_type = cfg
        .get("AdjustmentType")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AwsError::bad_request("ValidationException", "AdjustmentType is required")
        })?;
    if !matches!(
        adjustment_type,
        "ChangeInCapacity" | "PercentChangeInCapacity" | "ExactCapacity"
    ) {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("AdjustmentType `{adjustment_type}` is not a documented value."),
        ));
    }
    if let Some(agg) = cfg.get("MetricAggregationType").and_then(Value::as_str)
        && !matches!(agg, "Average" | "Minimum" | "Maximum")
    {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("MetricAggregationType `{agg}` is not a documented value."),
        ));
    }
    if let Some(min_mag) = cfg.get("MinAdjustmentMagnitude").and_then(Value::as_i64) {
        if min_mag < 0 {
            return Err(AwsError::bad_request(
                "ValidationException",
                format!("MinAdjustmentMagnitude `{min_mag}` must be >= 0."),
            ));
        }
        if adjustment_type != "PercentChangeInCapacity" {
            return Err(AwsError::bad_request(
                "ValidationException",
                "MinAdjustmentMagnitude only applies to PercentChangeInCapacity.",
            ));
        }
    }
    if let Some(steps) = cfg.get("StepAdjustments").and_then(Value::as_array) {
        // Collect [lower, upper) intervals, sorted by lower bound.
        // Unbounded sides extend to +/- infinity.
        let mut intervals: Vec<(f64, f64)> = Vec::with_capacity(steps.len());
        for s in steps {
            let lo = s
                .get("MetricIntervalLowerBound")
                .and_then(Value::as_f64)
                .unwrap_or(f64::NEG_INFINITY);
            let hi = s
                .get("MetricIntervalUpperBound")
                .and_then(Value::as_f64)
                .unwrap_or(f64::INFINITY);
            if lo > hi || lo.is_nan() || hi.is_nan() {
                return Err(AwsError::bad_request(
                    "ValidationException",
                    format!("StepAdjustment lower bound {lo} exceeds upper bound {hi}."),
                ));
            }
            intervals.push((lo, hi));
        }
        intervals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for pair in intervals.windows(2) {
            // Overlap iff the next interval's lower bound is strictly
            // below the previous interval's upper bound.
            if pair[1].0 < pair[0].1 {
                return Err(AwsError::bad_request(
                    "ValidationException",
                    "StepAdjustments contain overlapping ranges.",
                ));
            }
        }
    }
    Ok(())
}

/// Reject anything that isn't an IAM role ARN of the form
/// `arn:<partition>:iam::<account>:role/<name-or-path/name>`. AWS
/// strictly enforces the `iam`/`role/` shape; we mirror that so
/// callers passing a function ARN or random string fail fast.
pub(crate) fn validate_role_arn(arn: &str) -> Result<(), AwsError> {
    let mut parts = arn.splitn(6, ':');
    let arn_lit = parts.next();
    let partition = parts.next();
    let service = parts.next();
    let region = parts.next();
    let account = parts.next();
    let resource = parts.next();
    let shape_ok = arn_lit == Some("arn")
        && partition.is_some_and(|p| !p.is_empty())
        && service == Some("iam")
        // IAM is global: the region segment is always empty.
        && region == Some("")
        && account.is_some_and(|a| a.len() == 12 && a.chars().all(|c| c.is_ascii_digit()))
        && resource.is_some_and(|r| {
            let trimmed = r.strip_prefix("role/").unwrap_or("");
            !trimmed.is_empty()
        });
    if !shape_ok {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("RoleARN `{arn}` is not a valid IAM role ARN."),
        ));
    }
    Ok(())
}

/// Per-namespace `ResourceId` shape check. AWS documents a distinct
/// path-like format for each namespace (e.g. `service/cluster/name`
/// for ECS); we accept the canonical prefix plus at least one segment
/// so callers that drift from the docs fail fast rather than getting
/// silently stored. The generic length and charset envelope is
/// "1..=1600 chars; no control characters except tab, CR, or LF".
pub(crate) fn validate_resource_id(ns: &str, rid: &str) -> Result<(), AwsError> {
    if rid.is_empty() || rid.len() > 1600 {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("ResourceId must be 1..=1600 chars; got {}.", rid.len()),
        ));
    }
    if rid
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
    {
        return Err(AwsError::bad_request(
            "ValidationException",
            "ResourceId contains control characters.",
        ));
    }
    // Every documented prefix is followed by either `/` (path-style:
    // ecs, dynamodb, ec2, sagemaker, ...) or `:` (colon-style: lambda,
    // rds, neptune, cassandra, kafka, elasticmapreduce). We accept
    // both delimiters so callers stay compatible regardless of which
    // form the AWS docs picked.
    let starts_ok = |s: &str, prefix: &str| {
        let with_slash = s.starts_with(&format!("{prefix}/"));
        let with_colon = s.starts_with(&format!("{prefix}:"));
        with_slash || with_colon
    };

    let prefixes: &[&str] = match ns {
        "appstream" => &["fleet"],
        "cassandra" => &["keyspace"],
        "comprehend" => &["document-classifier-endpoint", "entity-recognizer-endpoint"],
        // `custom-resource` accepts any opaque caller-supplied string.
        "custom-resource" => return Ok(()),
        "dynamodb" => &["table"],
        "ec2" => &["spot-fleet-request", "fleet"],
        "ecs" => &["service"],
        "elasticache" => &["replication-group", "cache-cluster"],
        "elasticmapreduce" => &["instancegroup"],
        "kafka" => &["cluster"],
        "lambda" => &["function"],
        "neptune" => &["cluster"],
        "rds" => &["cluster"],
        "sagemaker" => &["endpoint", "inference-component", "variant"],
        "workspaces" => &["workspacespool"],
        _ => return Ok(()),
    };
    if !prefixes.iter().any(|p| starts_ok(rid, p)) {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "ResourceId `{rid}` does not match the documented shape for `{ns}`; expected one of {prefixes:?}.",
            ),
        ));
    }
    Ok(())
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
    if !is_valid_service_namespace(&ns) {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "ServiceNamespace `{ns}` is not one of the documented values: {:?}.",
                SERVICE_NAMESPACES
            ),
        ));
    }
    let rid = require_str(input, "ResourceId")?.to_string();
    validate_resource_id(&ns, &rid)?;
    if let Some(role) = input.get("RoleARN").and_then(Value::as_str) {
        validate_role_arn(role)?;
    }
    let dim = require_str(input, "ScalableDimension")?.to_string();
    if !is_valid_dimension_for_namespace(&ns, &dim) {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "ScalableDimension `{dim}` is not valid for ServiceNamespace `{ns}`; allowed: {:?}.",
                allowed_dimensions(&ns),
            ),
        ));
    }
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
    if let Some(cfg) = input.get("TargetTrackingScalingPolicyConfiguration") {
        validate_target_tracking_config(cfg)?;
    }
    if let Some(cfg) = input.get("StepScalingPolicyConfiguration") {
        validate_step_scaling_config(cfg)?;
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
            // AWS-parity: `PolicyNames` accepts both short policy
            // names and full PolicyARNs interchangeably. Match a
            // policy when the filter list is empty or its name *or*
            // ARN appears in the filter.
            if let Some(ns) = &names
                && !ns.is_empty()
                && !ns.iter().any(|n| n == &p.policy_name || n == &p.policy_arn)
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
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // ServiceNamespace is required by AWS and must be in the allowlist.
    let ns = require_str(input, "ServiceNamespace")?;
    if !is_valid_service_namespace(ns) {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("ServiceNamespace `{ns}` is not one of the documented values."),
        ));
    }
    // ScalableDimension is optional but must match the namespace's
    // dimension catalog when supplied.
    if let Some(dim) = input.get("ScalableDimension").and_then(Value::as_str)
        && !is_valid_dimension_for_namespace(ns, dim)
    {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("ScalableDimension `{dim}` is not valid for ServiceNamespace `{ns}`."),
        ));
    }
    // ResourceId is optional but must match the namespace's
    // documented shape when supplied.
    if let Some(rid) = input.get("ResourceId").and_then(Value::as_str) {
        validate_resource_id(ns, rid)?;
    }
    // IncludeNotScaledActivities is a documented flag; accept but
    // ignore (no activities to filter against in the emulator).
    let _ = input
        .get("IncludeNotScaledActivities")
        .and_then(Value::as_bool);

    // The emulator never executes scaling actions, so the activity
    // log is always empty. Returning an empty list matches what
    // `aws application-autoscaling` SDK clients expect for
    // newly-registered targets.
    Ok(json!({ "ScalingActivities": [] }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("application-autoscaling", "us-east-1")
    }

    #[test]
    fn register_rejects_unknown_service_namespace() {
        let state = AppAutoScalingState::default();
        let err = register_scalable_target(
            &state,
            &json!({
                "ServiceNamespace": "not-real",
                "ResourceId": "table/foo",
                "ScalableDimension": "dynamodb:table:ReadCapacityUnits",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("not-real"));
    }

    #[test]
    fn register_accepts_documented_service_namespaces() {
        let state = AppAutoScalingState::default();
        // ResourceIds shaped per the documented prefix for each namespace.
        for (ns, rid) in [
            ("appstream", "fleet/my-fleet"),
            ("cassandra", "keyspace/ks/table/tb"),
            ("comprehend", "document-classifier-endpoint/abc"),
            ("custom-resource", "anything-goes"),
            ("dynamodb", "table/my-table"),
            ("ec2", "spot-fleet-request/sfr-0123"),
            ("ecs", "service/cluster/svc"),
            ("elasticache", "replication-group/rg"),
            ("elasticmapreduce", "instancegroup/ig-1/cluster/c-1"),
            ("kafka", "cluster/abc-uuid"),
            ("lambda", "function:my-fn:my-alias"),
            ("neptune", "cluster:db-cluster"),
            ("rds", "cluster:db-cluster"),
            ("sagemaker", "endpoint/my-ep/variant/v1"),
            ("workspaces", "workspacespool/wsp-abc"),
        ] {
            let dim = allowed_dimensions(ns)
                .first()
                .copied()
                .unwrap_or_else(|| panic!("no dimension catalog for {ns}"));
            register_scalable_target(
                &state,
                &json!({
                    "ServiceNamespace": ns,
                    "ResourceId": rid,
                    "ScalableDimension": dim,
                }),
                &ctx(),
            )
            .unwrap_or_else(|e| panic!("namespace `{ns}` should be accepted: {e:?}"));
        }
    }

    #[test]
    fn register_rejects_resource_id_with_wrong_prefix() {
        let state = AppAutoScalingState::default();
        let err = register_scalable_target(
            &state,
            &json!({
                "ServiceNamespace": "ecs",
                "ResourceId": "cluster/foo/bar",
                "ScalableDimension": "ecs:service:DesiredCount",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("service"));
    }

    fn setup_target(state: &AppAutoScalingState) {
        register_scalable_target(
            state,
            &json!({
                "ServiceNamespace": "ecs",
                "ResourceId": "service/cluster/svc",
                "ScalableDimension": "ecs:service:DesiredCount",
            }),
            &ctx(),
        )
        .unwrap();
    }

    fn put_with_config(state: &AppAutoScalingState, cfg: Value) -> Result<Value, AwsError> {
        put_scaling_policy(
            state,
            &json!({
                "PolicyName": "p",
                "ServiceNamespace": "ecs",
                "ResourceId": "service/cluster/svc",
                "ScalableDimension": "ecs:service:DesiredCount",
                "PolicyType": "TargetTrackingScaling",
                "TargetTrackingScalingPolicyConfiguration": cfg,
            }),
            &ctx(),
        )
    }

    #[test]
    fn target_tracking_requires_positive_target_value() {
        let state = AppAutoScalingState::default();
        setup_target(&state);
        let err = put_with_config(
            &state,
            json!({
                "TargetValue": 0,
                "PredefinedMetricSpecification": { "PredefinedMetricType": "ECSServiceAverageCPUUtilization" },
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");

        let err = put_with_config(
            &state,
            json!({
                "TargetValue": -1.5,
                "PredefinedMetricSpecification": { "PredefinedMetricType": "ECSServiceAverageCPUUtilization" },
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn target_tracking_rejects_negative_cooldown() {
        let state = AppAutoScalingState::default();
        setup_target(&state);
        let err = put_with_config(
            &state,
            json!({
                "TargetValue": 50.0,
                "ScaleInCooldown": -1,
                "PredefinedMetricSpecification": { "PredefinedMetricType": "ECSServiceAverageCPUUtilization" },
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn target_tracking_requires_exactly_one_metric_spec() {
        let state = AppAutoScalingState::default();
        setup_target(&state);
        // None.
        let err = put_with_config(&state, json!({ "TargetValue": 50.0 })).unwrap_err();
        assert_eq!(err.code, "ValidationException");

        // Both.
        let err = put_with_config(
            &state,
            json!({
                "TargetValue": 50.0,
                "PredefinedMetricSpecification": { "PredefinedMetricType": "ECSServiceAverageCPUUtilization" },
                "CustomizedMetricSpecification": { "MetricName": "Custom", "Namespace": "App", "Statistic": "Average" },
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn describe_activities_requires_service_namespace() {
        let state = AppAutoScalingState::default();
        let err = describe_scaling_activities(&state, &json!({}), &ctx()).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn describe_activities_accepts_documented_filters() {
        let state = AppAutoScalingState::default();
        // ResourceId / ScalableDimension / IncludeNotScaledActivities
        // all accepted; result is empty for the emulator.
        let resp = describe_scaling_activities(
            &state,
            &json!({
                "ServiceNamespace": "ecs",
                "ResourceId": "service/cluster/svc",
                "ScalableDimension": "ecs:service:DesiredCount",
                "IncludeNotScaledActivities": true,
            }),
            &ctx(),
        )
        .unwrap();
        assert!(resp["ScalingActivities"].as_array().unwrap().is_empty());
    }

    #[test]
    fn describe_activities_validates_resource_id_shape() {
        let state = AppAutoScalingState::default();
        let err = describe_scaling_activities(
            &state,
            &json!({
                "ServiceNamespace": "ecs",
                "ResourceId": "definitely-not-ecs",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn describe_activities_validates_dimension_for_namespace() {
        let state = AppAutoScalingState::default();
        let err = describe_scaling_activities(
            &state,
            &json!({
                "ServiceNamespace": "ecs",
                "ScalableDimension": "lambda:function:ProvisionedConcurrency",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn describe_policies_accepts_both_names_and_arns() {
        let state = AppAutoScalingState::default();
        setup_target(&state);
        let resp = put_with_config(
            &state,
            json!({
                "TargetValue": 50.0,
                "PredefinedMetricSpecification": { "PredefinedMetricType": "ECSServiceAverageCPUUtilization" },
            }),
        )
        .unwrap();
        let arn = resp["PolicyARN"].as_str().unwrap().to_string();

        // Filter by short name.
        let by_name = describe_scaling_policies(
            &state,
            &json!({ "ServiceNamespace": "ecs", "PolicyNames": ["p"] }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(by_name["ScalingPolicies"].as_array().unwrap().len(), 1);

        // Filter by ARN -> same hit.
        let by_arn = describe_scaling_policies(
            &state,
            &json!({ "ServiceNamespace": "ecs", "PolicyNames": [arn] }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(by_arn["ScalingPolicies"].as_array().unwrap().len(), 1);

        // Unknown value -> empty.
        let empty = describe_scaling_policies(
            &state,
            &json!({ "ServiceNamespace": "ecs", "PolicyNames": ["does-not-exist"] }),
            &ctx(),
        )
        .unwrap();
        assert!(empty["ScalingPolicies"].as_array().unwrap().is_empty());
    }

    #[test]
    fn customized_metric_requires_name_namespace_statistic() {
        let state = AppAutoScalingState::default();
        setup_target(&state);
        // Missing MetricName.
        let err = put_with_config(
            &state,
            json!({
                "TargetValue": 50.0,
                "CustomizedMetricSpecification": { "Namespace": "App", "Statistic": "Average" },
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");

        // Unknown Statistic.
        let err = put_with_config(
            &state,
            json!({
                "TargetValue": 50.0,
                "CustomizedMetricSpecification": {
                    "MetricName": "M", "Namespace": "App", "Statistic": "p99",
                },
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn customized_metric_dimensions_must_have_name_and_value() {
        let state = AppAutoScalingState::default();
        setup_target(&state);
        let err = put_with_config(
            &state,
            json!({
                "TargetValue": 50.0,
                "CustomizedMetricSpecification": {
                    "MetricName": "M", "Namespace": "App", "Statistic": "Average",
                    "Dimensions": [{ "Name": "" }],
                },
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn customized_metric_rejects_mixing_metrics_and_single() {
        let state = AppAutoScalingState::default();
        setup_target(&state);
        let err = put_with_config(
            &state,
            json!({
                "TargetValue": 50.0,
                "CustomizedMetricSpecification": {
                    "MetricName": "M",
                    "Namespace": "App",
                    "Statistic": "Average",
                    "Metrics": [],
                },
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn customized_metric_accepts_well_formed_single_spec() {
        let state = AppAutoScalingState::default();
        setup_target(&state);
        put_with_config(
            &state,
            json!({
                "TargetValue": 50.0,
                "CustomizedMetricSpecification": {
                    "MetricName": "RequestLatency",
                    "Namespace": "App/web",
                    "Statistic": "Average",
                    "Unit": "Milliseconds",
                    "Dimensions": [
                        { "Name": "Stage", "Value": "prod" },
                    ],
                },
            }),
        )
        .unwrap();
    }

    fn put_with_step_config(state: &AppAutoScalingState, cfg: Value) -> Result<Value, AwsError> {
        put_scaling_policy(
            state,
            &json!({
                "PolicyName": "step-p",
                "ServiceNamespace": "ecs",
                "ResourceId": "service/cluster/svc",
                "ScalableDimension": "ecs:service:DesiredCount",
                "PolicyType": "StepScaling",
                "StepScalingPolicyConfiguration": cfg,
            }),
            &ctx(),
        )
    }

    #[test]
    fn step_scaling_requires_known_adjustment_type() {
        let state = AppAutoScalingState::default();
        setup_target(&state);
        let err =
            put_with_step_config(&state, json!({ "AdjustmentType": "OopsType" })).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn step_scaling_rejects_min_adjustment_with_change_in_capacity() {
        let state = AppAutoScalingState::default();
        setup_target(&state);
        let err = put_with_step_config(
            &state,
            json!({
                "AdjustmentType": "ChangeInCapacity",
                "MinAdjustmentMagnitude": 5,
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn step_scaling_rejects_overlapping_steps() {
        let state = AppAutoScalingState::default();
        setup_target(&state);
        let err = put_with_step_config(
            &state,
            json!({
                "AdjustmentType": "ChangeInCapacity",
                "StepAdjustments": [
                    { "MetricIntervalLowerBound": 0, "MetricIntervalUpperBound": 10, "ScalingAdjustment": 1 },
                    { "MetricIntervalLowerBound": 5, "MetricIntervalUpperBound": 15, "ScalingAdjustment": 2 },
                ],
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn step_scaling_accepts_adjacent_steps() {
        let state = AppAutoScalingState::default();
        setup_target(&state);
        put_with_step_config(
            &state,
            json!({
                "AdjustmentType": "PercentChangeInCapacity",
                "MetricAggregationType": "Average",
                "MinAdjustmentMagnitude": 1,
                "StepAdjustments": [
                    { "MetricIntervalLowerBound": 0, "MetricIntervalUpperBound": 10, "ScalingAdjustment": 1 },
                    { "MetricIntervalLowerBound": 10, "MetricIntervalUpperBound": 20, "ScalingAdjustment": 2 },
                ],
            }),
        )
        .unwrap();
    }

    #[test]
    fn target_tracking_accepts_predefined_only() {
        let state = AppAutoScalingState::default();
        setup_target(&state);
        put_with_config(
            &state,
            json!({
                "TargetValue": 50.0,
                "ScaleInCooldown": 60,
                "ScaleOutCooldown": 60,
                "PredefinedMetricSpecification": { "PredefinedMetricType": "ECSServiceAverageCPUUtilization" },
            }),
        )
        .unwrap();
    }

    #[test]
    fn register_rejects_bad_role_arn() {
        let state = AppAutoScalingState::default();
        for bad in [
            "not-an-arn",
            "arn:aws:s3:::my-bucket",
            "arn:aws:iam::123456789012:user/me",
            // Account too short.
            "arn:aws:iam::1234:role/r",
            // Region must be empty for IAM.
            "arn:aws:iam:us-east-1:123456789012:role/r",
        ] {
            let err = register_scalable_target(
                &state,
                &json!({
                    "ServiceNamespace": "ecs",
                    "ResourceId": "service/c/s",
                    "ScalableDimension": "ecs:service:DesiredCount",
                    "RoleARN": bad,
                }),
                &ctx(),
            )
            .unwrap_err();
            assert_eq!(err.code, "ValidationException", "input {bad}");
        }
    }

    #[test]
    fn register_accepts_well_formed_role_arn() {
        let state = AppAutoScalingState::default();
        register_scalable_target(
            &state,
            &json!({
                "ServiceNamespace": "ecs",
                "ResourceId": "service/c/s",
                "ScalableDimension": "ecs:service:DesiredCount",
                "RoleARN": "arn:aws:iam::123456789012:role/aws-service-role/application-autoscaling.amazonaws.com/AWSServiceRoleForApplicationAutoScaling",
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn register_rejects_empty_resource_id() {
        let state = AppAutoScalingState::default();
        let err = register_scalable_target(
            &state,
            &json!({
                "ServiceNamespace": "ecs",
                "ResourceId": "",
                "ScalableDimension": "ecs:service:DesiredCount",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn register_rejects_dimension_for_wrong_namespace() {
        let state = AppAutoScalingState::default();
        // ECS-only dimension on a Lambda target.
        let err = register_scalable_target(
            &state,
            &json!({
                "ServiceNamespace": "lambda",
                "ResourceId": "function:foo",
                "ScalableDimension": "ecs:service:DesiredCount",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("ecs:service:DesiredCount"));
    }

    #[test]
    fn register_rejects_unknown_dimension() {
        let state = AppAutoScalingState::default();
        let err = register_scalable_target(
            &state,
            &json!({
                "ServiceNamespace": "ecs",
                "ResourceId": "service/c/s",
                "ScalableDimension": "ecs:service:Pretend",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }
}
