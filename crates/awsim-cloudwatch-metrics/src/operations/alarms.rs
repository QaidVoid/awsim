use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{CloudWatchState, Dimension, MetricAlarm};

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
        let leap =
            (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
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

fn alarm_to_json(alarm: &MetricAlarm) -> Value {
    let mut v = json!({
        "AlarmName": alarm.alarm_name,
        "MetricName": alarm.metric_name,
        "Namespace": alarm.namespace,
        "Statistic": alarm.statistic,
        "Period": alarm.period,
        "EvaluationPeriods": alarm.evaluation_periods,
        "Threshold": alarm.threshold,
        "ComparisonOperator": alarm.comparison_operator,
        "StateValue": alarm.state_value,
        "StateReason": alarm.state_reason,
        "ActionsEnabled": alarm.actions_enabled,
        "AlarmActions": alarm.alarm_actions,
        "Dimensions": alarm.dimensions.iter().map(|d| json!({
            "Name": d.name,
            "Value": d.value,
        })).collect::<Vec<_>>(),
    });
    if let Some(ts) = &alarm.state_updated_at {
        v["StateUpdatedTimestamp"] = Value::String(ts.clone());
    }
    v
}

/// PutMetricAlarm
pub fn put_metric_alarm(
    state: &Arc<CloudWatchState>,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let alarm_name = input
        .get("AlarmName")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "AlarmName is required"))?
        .to_string();
    let metric_name = input
        .get("MetricName")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let namespace = input
        .get("Namespace")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let statistic = input
        .get("Statistic")
        .and_then(Value::as_str)
        .unwrap_or("Average")
        .to_string();
    let period = input.get("Period").and_then(Value::as_u64).unwrap_or(60);
    let evaluation_periods = input
        .get("EvaluationPeriods")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let threshold = input
        .get("Threshold")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let comparison_operator = input
        .get("ComparisonOperator")
        .and_then(Value::as_str)
        .unwrap_or("GreaterThanThreshold")
        .to_string();
    let actions_enabled = input
        .get("ActionsEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let alarm_actions: Vec<String> = input
        .get("AlarmActions")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let dimensions: Vec<Dimension> = input
        .get("Dimensions")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|d| {
                    let name = d.get("Name").and_then(Value::as_str)?.to_string();
                    let value = d.get("Value").and_then(Value::as_str)?.to_string();
                    Some(Dimension { name, value })
                })
                .collect()
        })
        .unwrap_or_default();

    let alarm = MetricAlarm {
        alarm_name: alarm_name.clone(),
        metric_name,
        namespace,
        statistic,
        period,
        evaluation_periods,
        threshold,
        comparison_operator,
        state_value: "INSUFFICIENT_DATA".to_string(),
        state_reason: "Newly created alarm".to_string(),
        actions_enabled,
        alarm_actions,
        created_at: chrono_now(),
        state_updated_at: None,
        dimensions,
    };

    state.alarms.insert(alarm_name, alarm);
    // Newly registered alarm immediately re-evaluates against any data
    // that was written before it existed.
    evaluate_alarms(state, ctx);
    Ok(json!({}))
}

/// DescribeAlarms
pub fn describe_alarms(
    state: &Arc<CloudWatchState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_names: Vec<&str> = input
        .get("AlarmNames")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|n| n.as_str()).collect())
        .unwrap_or_default();
    let filter_state = input.get("StateValue").and_then(Value::as_str);

    let alarms: Vec<Value> = state
        .alarms
        .iter()
        .filter(|entry| {
            let alarm = entry.value();
            let name_ok =
                filter_names.is_empty() || filter_names.contains(&alarm.alarm_name.as_str());
            let state_ok = filter_state.map(|s| alarm.state_value == s).unwrap_or(true);
            name_ok && state_ok
        })
        .map(|entry| alarm_to_json(entry.value()))
        .collect();

    Ok(json!({ "MetricAlarms": alarms }))
}

/// DeleteAlarms
pub fn delete_alarms(
    state: &Arc<CloudWatchState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let names: Vec<&str> = input
        .get("AlarmNames")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|n| n.as_str()).collect())
        .unwrap_or_default();

    for name in names {
        state.alarms.remove(name);
    }

    Ok(json!({}))
}

/// SetAlarmState
pub fn set_alarm_state(
    state: &Arc<CloudWatchState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let alarm_name = input
        .get("AlarmName")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "AlarmName is required"))?;
    let state_value = input
        .get("StateValue")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "StateValue is required"))?;
    let state_reason = input
        .get("StateReason")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let mut alarm = state.alarms.get_mut(alarm_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Alarm {alarm_name} not found"),
        )
    })?;

    alarm.state_value = state_value.to_string();
    alarm.state_reason = state_reason;
    alarm.state_updated_at = Some(chrono_now());

    Ok(json!({}))
}

/// Recompute every alarm's `state_value` against the current metric store.
///
/// CloudWatch evaluates each alarm by aggregating data points within its
/// `period` window using the configured `statistic`, then comparing the
/// aggregate against `threshold` via `comparison_operator`. Real CloudWatch
/// runs this on a 60-second cadence; we run it on-demand from PutMetricData
/// (and PutMetricAlarm) so test suites see state changes synchronously.
pub fn evaluate_alarms(state: &Arc<CloudWatchState>, ctx: &RequestContext) {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let now_ms = (now_secs as i64).saturating_mul(1000);

    // Snapshot alarm names so we don't hold a DashMap iter borrow while we
    // get_mut into it.
    let alarm_names: Vec<String> = state.alarms.iter().map(|e| e.key().clone()).collect();
    for name in alarm_names {
        let mut alarm = match state.alarms.get_mut(&name) {
            Some(a) => a,
            None => continue,
        };

        let window_secs = alarm.period.saturating_mul(alarm.evaluation_periods);
        let window_start_ms = now_ms.saturating_sub((window_secs as i64).saturating_mul(1000));

        let datums = match super::metrics::datapoints_for_alarm(
            state,
            ctx,
            &alarm.namespace,
            &alarm.metric_name,
            window_start_ms,
            now_ms,
        ) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let matching: Vec<&(f64, Vec<Dimension>, String)> = datums
            .iter()
            .filter(|(_, dims, _)| dimensions_match(&alarm.dimensions, dims))
            .collect();

        let new_state = if matching.is_empty() {
            (
                "INSUFFICIENT_DATA",
                format!("No matching data points within the last {window_secs} seconds"),
            )
        } else {
            let value = aggregate(&alarm.statistic, &matching);
            let breach = compare(&alarm.comparison_operator, value, alarm.threshold);
            if breach {
                (
                    "ALARM",
                    format!(
                        "Statistic {} ({:.4}) {} threshold {}",
                        alarm.statistic, value, alarm.comparison_operator, alarm.threshold
                    ),
                )
            } else {
                (
                    "OK",
                    format!(
                        "Statistic {} ({:.4}) within threshold {}",
                        alarm.statistic, value, alarm.threshold
                    ),
                )
            }
        };

        if alarm.state_value != new_state.0 {
            alarm.state_value = new_state.0.to_string();
            alarm.state_updated_at = Some(chrono_now());
        }
        alarm.state_reason = new_state.1;
    }
}

/// True when every alarm dimension is satisfied by the datum's dimensions.
/// An alarm with no dimensions matches every datum (real CloudWatch
/// "no-dimension" alarms aggregate across all dimensions of the metric).
fn dimensions_match(alarm_dims: &[Dimension], datum_dims: &[Dimension]) -> bool {
    alarm_dims.iter().all(|ad| {
        datum_dims
            .iter()
            .any(|dd| dd.name == ad.name && dd.value == ad.value)
    })
}

/// `datums` is `&[&(value, dimensions, timestamp_string)]` — the
/// shape returned by `metrics::datapoints_for_alarm`.
fn aggregate(statistic: &str, datums: &[&(f64, Vec<Dimension>, String)]) -> f64 {
    if datums.is_empty() {
        return 0.0;
    }
    let values = datums.iter().map(|d| d.0);
    match statistic {
        "Sum" => values.sum(),
        "Minimum" => values.fold(f64::INFINITY, f64::min),
        "Maximum" => values.fold(f64::NEG_INFINITY, f64::max),
        "SampleCount" => datums.len() as f64,
        // Average — and the sensible default when an unknown statistic
        // arrives. Real CloudWatch validates the field at PutMetricAlarm
        // but we accept anything for simulator ergonomics.
        _ => values.sum::<f64>() / datums.len() as f64,
    }
}

fn compare(operator: &str, lhs: f64, threshold: f64) -> bool {
    match operator {
        "GreaterThanThreshold" => lhs > threshold,
        "GreaterThanOrEqualToThreshold" => lhs >= threshold,
        "LessThanThreshold" => lhs < threshold,
        "LessThanOrEqualToThreshold" => lhs <= threshold,
        // Default to "no breach" rather than panic on unknown operators.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("monitoring", "us-east-1")
    }

    fn fresh_state() -> Arc<CloudWatchState> {
        let state = Arc::new(CloudWatchState::default());
        let id = uuid::Uuid::new_v4();
        let path = std::env::temp_dir().join(format!("awsim-cwm-state-test-{id}.db"));
        let store = Arc::new(crate::SqliteStore::open(path).expect("test sqlite"));
        state.set_sqlite(store);
        state
    }

    fn alarm_state(state: &Arc<CloudWatchState>, name: &str) -> String {
        state
            .alarms
            .get(name)
            .map(|a| a.state_value.clone())
            .unwrap_or_else(|| "<missing>".to_string())
    }

    #[test]
    fn put_metric_data_flips_alarm_to_alarm_then_ok() {
        let state = fresh_state();
        put_metric_alarm(
            &state,
            &json!({
                "AlarmName": "high-cpu",
                "Namespace": "AWS/EC2",
                "MetricName": "CPUUtilization",
                "Statistic": "Average",
                "Period": 60,
                "EvaluationPeriods": 1,
                "Threshold": 50.0,
                "ComparisonOperator": "GreaterThanThreshold",
            }),
            &ctx(),
        )
        .unwrap();
        // No data yet — INSUFFICIENT_DATA.
        assert_eq!(alarm_state(&state, "high-cpu"), "INSUFFICIENT_DATA");

        // Drive a high value through PutMetricData; the on-write evaluator
        // should immediately flip the alarm to ALARM.
        crate::operations::metrics::put_metric_data(
            &state,
            &json!({
                "Namespace": "AWS/EC2",
                "MetricData": [{ "MetricName": "CPUUtilization", "Value": 92.5 }],
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(alarm_state(&state, "high-cpu"), "ALARM");

        // Two low samples — Average drops below threshold and the alarm
        // recovers to OK.
        crate::operations::metrics::put_metric_data(
            &state,
            &json!({
                "Namespace": "AWS/EC2",
                "MetricData": [
                    { "MetricName": "CPUUtilization", "Value": 5.0 },
                    { "MetricName": "CPUUtilization", "Value": 8.0 },
                ],
            }),
            &ctx(),
        )
        .unwrap();
        // Three points total: 92.5, 5.0, 8.0 — avg ~35, below 50.
        assert_eq!(alarm_state(&state, "high-cpu"), "OK");
    }

    #[test]
    fn alarm_dimensions_filter_metric_match() {
        let state = fresh_state();
        put_metric_alarm(
            &state,
            &json!({
                "AlarmName": "queue-depth",
                "Namespace": "AWS/SQS",
                "MetricName": "ApproximateNumberOfMessagesVisible",
                "Statistic": "Maximum",
                "Period": 60,
                "EvaluationPeriods": 1,
                "Threshold": 100.0,
                "ComparisonOperator": "GreaterThanOrEqualToThreshold",
                "Dimensions": [{ "Name": "QueueName", "Value": "orders" }],
            }),
            &ctx(),
        )
        .unwrap();
        // A datum on the wrong queue must not move the alarm.
        crate::operations::metrics::put_metric_data(
            &state,
            &json!({
                "Namespace": "AWS/SQS",
                "MetricData": [{
                    "MetricName": "ApproximateNumberOfMessagesVisible",
                    "Value": 200.0,
                    "Dimensions": [{ "Name": "QueueName", "Value": "audit-trail" }],
                }],
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(alarm_state(&state, "queue-depth"), "INSUFFICIENT_DATA");

        // The right queue trips it.
        crate::operations::metrics::put_metric_data(
            &state,
            &json!({
                "Namespace": "AWS/SQS",
                "MetricData": [{
                    "MetricName": "ApproximateNumberOfMessagesVisible",
                    "Value": 250.0,
                    "Dimensions": [{ "Name": "QueueName", "Value": "orders" }],
                }],
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(alarm_state(&state, "queue-depth"), "ALARM");
    }
}
