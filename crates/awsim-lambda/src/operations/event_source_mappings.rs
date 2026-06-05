use awsim_core::{AwsError, RequestContext, arn};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::resource_not_found,
    state::{EventSourceMapping, LambdaState},
    util::{iso8601_to_epoch, new_uuid, now_iso8601, opt_bool, opt_str, opt_u64, require_str},
};

fn opt_value<'a>(input: &'a Value, key: &str) -> Option<&'a Value> {
    input.get(key).filter(|v| !v.is_null())
}

fn opt_tags(input: &Value) -> HashMap<String, String> {
    input
        .get("Tags")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn opt_string_array(input: &Value, key: &str) -> Vec<String> {
    input
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn mapping_to_value(m: &EventSourceMapping) -> Value {
    let mut out = json!({
        "UUID": m.uuid,
        "EventSourceArn": m.event_source_arn,
        "FunctionArn": m.function_arn,
        "BatchSize": m.batch_size,
        "State": m.state,
        "StateTransitionReason": "USER_INITIATED",
        "LastModified": iso8601_to_epoch(&m.last_modified),
        "MaximumBatchingWindowInSeconds": m.maximum_batching_window_in_seconds,
        "BisectBatchOnFunctionError": m.bisect_batch_on_function_error,
        "FunctionResponseTypes": m.function_response_types,
        "LastProcessingResult": m.last_processing_result,
    });

    let obj = out.as_object_mut().expect("object");
    if let Some(sp) = &m.starting_position {
        obj.insert("StartingPosition".into(), Value::String(sp.clone()));
    }
    if let Some(ts) = m.starting_position_timestamp {
        obj.insert("StartingPositionTimestamp".into(), json!(ts));
    }
    if let Some(age) = m.maximum_record_age_in_seconds {
        obj.insert("MaximumRecordAgeInSeconds".into(), json!(age));
    }
    if let Some(retries) = m.maximum_retry_attempts {
        obj.insert("MaximumRetryAttempts".into(), json!(retries));
    }
    if let Some(pf) = m.parallelization_factor {
        obj.insert("ParallelizationFactor".into(), json!(pf));
    }
    if let Some(tw) = m.tumbling_window_in_seconds {
        obj.insert("TumblingWindowInSeconds".into(), json!(tw));
    }
    if let Some(fc) = &m.filter_criteria {
        obj.insert("FilterCriteria".into(), fc.clone());
    }
    if let Some(max) = m.scaling_max_concurrency {
        obj.insert("ScalingConfig".into(), json!({ "MaximumConcurrency": max }));
    }
    if let Some(arn) = &m.destination_on_failure {
        obj.insert(
            "DestinationConfig".into(),
            json!({ "OnFailure": { "Destination": arn } }),
        );
    }
    out
}

/// Parse + validate `ScalingConfig.MaximumConcurrency`. AWS allows
/// values in `[2, 1000]` for SQS event-source mappings.
fn parse_scaling_max_concurrency(input: &Value) -> Result<Option<u32>, AwsError> {
    let Some(value) = input
        .get("ScalingConfig")
        .and_then(|s| s.get("MaximumConcurrency"))
    else {
        return Ok(None);
    };
    let n = value.as_u64().ok_or_else(|| {
        awsim_core::AwsError::bad_request(
            "InvalidParameterValueException",
            "ScalingConfig.MaximumConcurrency must be a number.",
        )
    })?;
    if !(2..=1000).contains(&n) {
        return Err(awsim_core::AwsError::bad_request(
            "InvalidParameterValueException",
            format!("ScalingConfig.MaximumConcurrency must be between 2 and 1000 (got {n})."),
        ));
    }
    Ok(Some(n as u32))
}

fn destination_on_failure_from(input: &Value) -> Option<String> {
    input
        .get("DestinationConfig")
        .and_then(|d| d.get("OnFailure"))
        .and_then(|f| f.get("Destination"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

pub fn create_event_source_mapping(
    state: &LambdaState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let event_source_arn = require_str(input, "EventSourceArn")?;
    let function_name = require_str(input, "FunctionName")?;
    let batch_size = opt_u64(input, "BatchSize").unwrap_or(10) as u32;
    let enabled = opt_bool(input, "Enabled").unwrap_or(true);

    let function_arn = if function_name.starts_with("arn:") {
        function_name.to_string()
    } else {
        match state.functions.get(function_name) {
            Some(f) => f.arn.clone(),
            None => arn::build(ctx, "lambda", format!("function:{function_name}")),
        }
    };

    let uuid = new_uuid();
    let mapping = EventSourceMapping {
        uuid: uuid.clone(),
        event_source_arn: event_source_arn.to_string(),
        function_arn,
        batch_size,
        enabled,
        state: if enabled {
            "Enabled".to_string()
        } else {
            "Disabled".to_string()
        },
        last_modified: now_iso8601(),
        starting_position: opt_str(input, "StartingPosition").map(|s| s.to_string()),
        starting_position_timestamp: input
            .get("StartingPositionTimestamp")
            .and_then(|v| v.as_f64()),
        maximum_batching_window_in_seconds: opt_u64(input, "MaximumBatchingWindowInSeconds")
            .unwrap_or(0) as u32,
        maximum_record_age_in_seconds: input
            .get("MaximumRecordAgeInSeconds")
            .and_then(|v| v.as_i64())
            .map(|v| v as i32),
        bisect_batch_on_function_error: opt_bool(input, "BisectBatchOnFunctionError")
            .unwrap_or(false),
        maximum_retry_attempts: input
            .get("MaximumRetryAttempts")
            .and_then(|v| v.as_i64())
            .map(|v| v as i32),
        parallelization_factor: opt_u64(input, "ParallelizationFactor").map(|v| v as u32),
        tumbling_window_in_seconds: opt_u64(input, "TumblingWindowInSeconds").map(|v| v as u32),
        filter_criteria: opt_value(input, "FilterCriteria").cloned(),
        destination_on_failure: destination_on_failure_from(input),
        function_response_types: opt_string_array(input, "FunctionResponseTypes"),
        last_processing_result: "No records processed".to_string(),
        shard_iterators: HashMap::new(),
        tags: opt_tags(input),
        scaling_max_concurrency: parse_scaling_max_concurrency(input)?,
    };

    let result = mapping_to_value(&mapping);
    state.event_source_mappings.insert(uuid, mapping);

    Ok(result)
}

pub fn get_event_source_mapping(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let uuid = require_str(input, "UUID")?;
    let m = state
        .event_source_mappings
        .get(uuid)
        .ok_or_else(|| resource_not_found("event source mapping", uuid))?;
    Ok(mapping_to_value(&m))
}

pub fn update_event_source_mapping(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let uuid = require_str(input, "UUID")?;
    let mut m = state
        .event_source_mappings
        .get_mut(uuid)
        .ok_or_else(|| resource_not_found("event source mapping", uuid))?;

    if let Some(bs) = opt_u64(input, "BatchSize") {
        m.batch_size = bs as u32;
    }
    if let Some(en) = opt_bool(input, "Enabled") {
        m.enabled = en;
        m.state = if en {
            "Enabled".to_string()
        } else {
            "Disabled".to_string()
        };
    }
    if let Some(w) = opt_u64(input, "MaximumBatchingWindowInSeconds") {
        m.maximum_batching_window_in_seconds = w as u32;
    }
    if let Some(age) = input
        .get("MaximumRecordAgeInSeconds")
        .and_then(|v| v.as_i64())
    {
        m.maximum_record_age_in_seconds = Some(age as i32);
    }
    if let Some(b) = opt_bool(input, "BisectBatchOnFunctionError") {
        m.bisect_batch_on_function_error = b;
    }
    if let Some(r) = input.get("MaximumRetryAttempts").and_then(|v| v.as_i64()) {
        m.maximum_retry_attempts = Some(r as i32);
    }
    if let Some(pf) = opt_u64(input, "ParallelizationFactor") {
        m.parallelization_factor = Some(pf as u32);
    }
    if let Some(tw) = opt_u64(input, "TumblingWindowInSeconds") {
        m.tumbling_window_in_seconds = Some(tw as u32);
    }
    if let Some(fc) = opt_value(input, "FilterCriteria") {
        m.filter_criteria = Some(fc.clone());
    }
    if input.get("DestinationConfig").is_some() {
        m.destination_on_failure = destination_on_failure_from(input);
    }
    if input.get("FunctionResponseTypes").is_some() {
        m.function_response_types = opt_string_array(input, "FunctionResponseTypes");
    }
    if input.get("ScalingConfig").is_some() {
        m.scaling_max_concurrency = parse_scaling_max_concurrency(input)?;
    }
    if let Some(fn_name) = opt_str(input, "FunctionName") {
        m.function_arn = if fn_name.starts_with("arn:") {
            fn_name.to_string()
        } else if let Some(f) = state.functions.get(fn_name) {
            f.arn.clone()
        } else {
            m.function_arn.clone()
        };
    }
    m.last_modified = now_iso8601();

    Ok(mapping_to_value(&m))
}

pub fn delete_event_source_mapping(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let uuid = require_str(input, "UUID")?;
    let (_, m) = state
        .event_source_mappings
        .remove(uuid)
        .ok_or_else(|| resource_not_found("event source mapping", uuid))?;
    Ok(mapping_to_value(&m))
}

pub fn list_event_source_mappings(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    use awsim_core::pagination::{cap_max_results, paginate};

    let filter_source_arn = opt_str(input, "EventSourceArn");
    let filter_function = opt_str(input, "FunctionName");

    let mut all: Vec<EventSourceMapping> = state
        .event_source_mappings
        .iter()
        .filter(|m| {
            if let Some(arn) = filter_source_arn
                && !m.event_source_arn.contains(arn)
            {
                return false;
            }
            if let Some(fn_name) = filter_function
                && !m.function_arn.contains(fn_name)
            {
                return false;
            }
            true
        })
        .map(|m| m.value().clone())
        .collect();
    all.sort_by(|a, b| a.uuid.cmp(&b.uuid));

    let max = cap_max_results(input.get("MaxItems").and_then(Value::as_i64), 100, 10_000);
    let marker = input.get("Marker").and_then(Value::as_str);
    let page = paginate(all, max, marker, |m| m.uuid.clone())?;

    let mappings: Vec<Value> = page.items.iter().map(mapping_to_value).collect();
    let mut result = json!({ "EventSourceMappings": mappings });
    if let Some(token) = page.next_token {
        result["NextMarker"] = json!(token);
    }
    Ok(result)
}
