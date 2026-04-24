use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{CloudWatchState, Dimension, MetricDatum};

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d, h, mi, s) = epoch_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn epoch_to_ymdhms(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    let mut year = 1970u64;
    let mut remaining = days;
    loop {
        let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
        let days_in_year = if leap { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let month_days: &[u64] = if leap {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 0u64;
    for &md in month_days {
        if remaining < md {
            break;
        }
        remaining -= md;
        month += 1;
    }
    (year, month + 1, remaining + 1, h, m, s)
}

fn parse_dimensions(dims_val: &Value) -> Vec<Dimension> {
    dims_val
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|d| {
                    let name = d.get("Name").and_then(Value::as_str)?.to_string();
                    let value = d.get("Value").and_then(Value::as_str)?.to_string();
                    Some(Dimension { name, value })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// PutMetricData
pub fn put_metric_data(
    state: &Arc<CloudWatchState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let namespace = input
        .get("Namespace")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Namespace is required"))?
        .to_string();

    let metric_data = input
        .get("MetricData")
        .and_then(Value::as_array)
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "MetricData is required"))?;

    let mut entry = state.metrics.entry(namespace.clone()).or_default();

    for datum in metric_data {
        let metric_name = datum
            .get("MetricName")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AwsError::bad_request("InvalidParameterValue", "MetricName is required")
            })?
            .to_string();
        let value = datum.get("Value").and_then(Value::as_f64).unwrap_or(0.0);
        let unit = datum
            .get("Unit")
            .and_then(Value::as_str)
            .unwrap_or("None")
            .to_string();
        let timestamp = datum
            .get("Timestamp")
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_else(chrono_now);
        let dimensions = datum
            .get("Dimensions")
            .map(parse_dimensions)
            .unwrap_or_default();

        entry.push(MetricDatum {
            metric_name,
            namespace: namespace.clone(),
            value,
            unit,
            timestamp,
            dimensions,
        });
    }

    Ok(json!({}))
}

/// ListMetrics
pub fn list_metrics(
    state: &Arc<CloudWatchState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_namespace = input.get("Namespace").and_then(Value::as_str);
    let filter_metric_name = input.get("MetricName").and_then(Value::as_str);

    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut metrics: Vec<Value> = Vec::new();

    for entry in state.metrics.iter() {
        let ns = entry.key().clone();
        if let Some(fn_) = filter_namespace
            && ns != fn_ {
                continue;
            }
        for datum in entry.value() {
            if let Some(fmn) = filter_metric_name
                && datum.metric_name != fmn {
                    continue;
                }
            let key = (ns.clone(), datum.metric_name.clone());
            if seen.insert(key) {
                metrics.push(json!({
                    "Namespace": ns,
                    "MetricName": datum.metric_name,
                    "Dimensions": datum.dimensions.iter().map(|d| json!({
                        "Name": d.name,
                        "Value": d.value,
                    })).collect::<Vec<_>>(),
                }));
            }
        }
    }

    Ok(json!({ "Metrics": metrics }))
}

/// GetMetricStatistics
pub fn get_metric_statistics(
    state: &Arc<CloudWatchState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let namespace = input
        .get("Namespace")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Namespace is required"))?;
    let metric_name = input
        .get("MetricName")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "MetricName is required"))?;
    let statistics = input
        .get("Statistics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let values: Vec<f64> = state
        .metrics
        .get(namespace)
        .map(|entry| {
            entry
                .iter()
                .filter(|d| d.metric_name == metric_name)
                .map(|d| d.value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let first_unit = state
        .metrics
        .get(namespace)
        .and_then(|entry| {
            entry
                .iter()
                .find(|d| d.metric_name == metric_name)
                .map(|d| d.unit.clone())
        })
        .unwrap_or_else(|| "None".to_string());
    let count = values.len() as f64;

    let sum: f64 = values.iter().sum();
    let average = if count > 0.0 { sum / count } else { 0.0 };
    let minimum = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let maximum = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let minimum = if minimum.is_infinite() { 0.0 } else { minimum };
    let maximum = if maximum.is_infinite() { 0.0 } else { maximum };

    let mut dp = json!({
        "Timestamp": chrono_now(),
        "Unit": first_unit,
        "SampleCount": count,
    });

    for stat in &statistics {
        let stat_name = stat.as_str().unwrap_or("");
        match stat_name {
            "Sum" => {
                dp["Sum"] = json!(sum);
            }
            "Average" => {
                dp["Average"] = json!(average);
            }
            "Minimum" => {
                dp["Minimum"] = json!(minimum);
            }
            "Maximum" => {
                dp["Maximum"] = json!(maximum);
            }
            "SampleCount" => {
                dp["SampleCount"] = json!(count);
            }
            _ => {}
        }
    }

    Ok(json!({
        "Label": metric_name,
        "Datapoints": if values.is_empty() { vec![] } else { vec![dp] },
    }))
}

/// GetMetricData
pub fn get_metric_data(
    state: &Arc<CloudWatchState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let queries = input
        .get("MetricDataQueries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut results: Vec<Value> = Vec::new();

    for query in &queries {
        let id = query
            .get("Id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let metric_stat = query.get("MetricStat");

        let (values, timestamps) = if let Some(ms) = metric_stat {
            let ns = ms
                .get("Metric")
                .and_then(|m| m.get("Namespace"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let mn = ms
                .get("Metric")
                .and_then(|m| m.get("MetricName"))
                .and_then(Value::as_str)
                .unwrap_or("");

            let data_points: Vec<(f64, String)> = state
                .metrics
                .get(ns)
                .map(|entry| {
                    entry
                        .iter()
                        .filter(|d| d.metric_name == mn)
                        .map(|d| (d.value, d.timestamp.clone()))
                        .collect()
                })
                .unwrap_or_default();

            let vals: Vec<Value> = data_points.iter().map(|(v, _)| json!(v)).collect();
            let ts: Vec<Value> = data_points.iter().map(|(_, t)| json!(t)).collect();
            (vals, ts)
        } else {
            (vec![], vec![])
        };

        results.push(json!({
            "Id": id,
            "StatusCode": "Complete",
            "Values": values,
            "Timestamps": timestamps,
        }));
    }

    Ok(json!({
        "MetricDataResults": results,
        "NextToken": null,
    }))
}
