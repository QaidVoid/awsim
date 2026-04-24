use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::{CloudFrontState, RealtimeLogConfig};

fn not_found(arn: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchRealtimeLogConfig",
        format!("The specified real-time log config does not exist: {arn}"),
    )
}

fn rt_to_value(r: &RealtimeLogConfig) -> Value {
    let fields: Vec<Value> = r.fields.iter().map(|s| Value::String(s.clone())).collect();
    json!({
        "ARN": r.arn,
        "Name": r.name,
        "SamplingRate": r.sampling_rate,
        "EndPoints": r.end_points,
        "Fields": fields,
    })
}

pub fn create_realtime_log_config(
    state: &CloudFrontState,
    input: &Value,
) -> Result<Value, AwsError> {
    let name = input
        .get("Name")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();
    let sampling_rate = input
        .get("SamplingRate")
        .and_then(|v| v.as_i64())
        .unwrap_or(100);
    let end_points = input.get("EndPoints").cloned().unwrap_or(json!([]));
    let fields: Vec<String> = input
        .get("Fields")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let arn = format!("arn:aws:cloudfront::123456789012:realtime-log-config/{name}");
    let cfg = RealtimeLogConfig {
        arn: arn.clone(),
        name,
        sampling_rate,
        fields,
        end_points,
    };

    let result = rt_to_value(&cfg);
    state.realtime_log_configs.insert(arn, cfg);

    Ok(json!({ "RealtimeLogConfig": result }))
}

pub fn get_realtime_log_config(state: &CloudFrontState, input: &Value) -> Result<Value, AwsError> {
    let arn = input
        .get("ARN")
        .or_else(|| input.get("Name"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let key = state
        .realtime_log_configs
        .iter()
        .find(|e| e.key() == arn || e.value().name == arn)
        .map(|e| e.key().clone());

    match key {
        Some(k) => {
            let cfg = state.realtime_log_configs.get(&k).unwrap();
            Ok(json!({ "RealtimeLogConfig": rt_to_value(&cfg) }))
        }
        None => Err(not_found(arn)),
    }
}

pub fn delete_realtime_log_config(
    state: &CloudFrontState,
    input: &Value,
) -> Result<Value, AwsError> {
    let arn = input
        .get("ARN")
        .or_else(|| input.get("Name"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let key = state
        .realtime_log_configs
        .iter()
        .find(|e| e.key() == arn || e.value().name == arn)
        .map(|e| e.key().clone());

    match key {
        Some(k) => {
            state.realtime_log_configs.remove(&k);
            Ok(json!({}))
        }
        None => Err(not_found(arn)),
    }
}

pub fn list_realtime_log_configs(state: &CloudFrontState) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .realtime_log_configs
        .iter()
        .map(|e| rt_to_value(e.value()))
        .collect();
    let qty = items.len();
    Ok(json!({
        "RealtimeLogConfigs": {
            "MaxItems": 100,
            "Quantity": qty,
            "IsTruncated": false,
            "Marker": "",
            "Items": items
        }
    }))
}
