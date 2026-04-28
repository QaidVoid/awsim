use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{CloudWatchState, Dimension, MetricAlarm, MetricDatum};

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
    _ctx: &RequestContext,
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
    evaluate_alarms(state);
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
pub fn evaluate_alarms(state: &Arc<CloudWatchState>) {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Snapshot alarm names so we don't hold a DashMap iter borrow while we
    // get_mut into it.
    let alarm_names: Vec<String> = state.alarms.iter().map(|e| e.key().clone()).collect();
    for name in alarm_names {
        let mut alarm = match state.alarms.get_mut(&name) {
            Some(a) => a,
            None => continue,
        };

        let window_start = now.saturating_sub(alarm.period * alarm.evaluation_periods);
        let datums = match state.metrics.get(&alarm.namespace) {
            Some(d) => d.value().clone(),
            None => Vec::new(),
        };
        let matching: Vec<&MetricDatum> = datums
            .iter()
            .filter(|d| d.metric_name == alarm.metric_name)
            .filter(|d| dimensions_match(&alarm.dimensions, &d.dimensions))
            .filter(|d| within_window(d.timestamp.as_str(), window_start, now))
            .collect();

        let new_state = if matching.is_empty() {
            (
                "INSUFFICIENT_DATA",
                format!(
                    "No matching data points within the last {} seconds",
                    alarm.period * alarm.evaluation_periods
                ),
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

fn within_window(timestamp: &str, start: u64, end: u64) -> bool {
    if timestamp.is_empty() {
        // Datums with no recorded timestamp are treated as "now".
        return true;
    }
    if let Ok(parsed) = parse_iso8601_seconds(timestamp) {
        return parsed >= start && parsed <= end;
    }
    true
}

fn parse_iso8601_seconds(ts: &str) -> Result<u64, ()> {
    // Minimal ISO 8601 parser accepting `YYYY-MM-DDTHH:MM:SSZ`. CloudWatch
    // datapoints we generate use exactly this shape; foreign clients that
    // send richer timestamps fall back to "current" via within_window.
    if ts.len() < 20 {
        return Err(());
    }
    let bytes = ts.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return Err(());
    }
    let year: u64 = ts[0..4].parse().map_err(|_| ())?;
    let month: u64 = ts[5..7].parse().map_err(|_| ())?;
    let day: u64 = ts[8..10].parse().map_err(|_| ())?;
    let hour: u64 = ts[11..13].parse().map_err(|_| ())?;
    let minute: u64 = ts[14..16].parse().map_err(|_| ())?;
    let second: u64 = ts[17..19].parse().map_err(|_| ())?;
    Ok(ymdhms_to_epoch(year, month, day, hour, minute, second))
}

fn ymdhms_to_epoch(year: u64, month: u64, day: u64, hour: u64, minute: u64, second: u64) -> u64 {
    let mut days: u64 = 0;
    for y in 1970..year {
        let leap = (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400);
        days += if leap { 366 } else { 365 };
    }
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let month_days: &[u64] = if leap {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    for &md in month_days.iter().take(month.saturating_sub(1) as usize) {
        days += md;
    }
    days += day.saturating_sub(1);
    days * 86400 + hour * 3600 + minute * 60 + second
}

fn aggregate(statistic: &str, datums: &[&MetricDatum]) -> f64 {
    if datums.is_empty() {
        return 0.0;
    }
    match statistic {
        "Sum" => datums.iter().map(|d| d.value).sum(),
        "Minimum" => datums.iter().map(|d| d.value).fold(f64::INFINITY, f64::min),
        "Maximum" => datums
            .iter()
            .map(|d| d.value)
            .fold(f64::NEG_INFINITY, f64::max),
        "SampleCount" => datums.len() as f64,
        // Average — and the sensible default when an unknown statistic
        // arrives. Real CloudWatch validates the field at PutMetricAlarm
        // but we accept anything for simulator ergonomics.
        _ => datums.iter().map(|d| d.value).sum::<f64>() / datums.len() as f64,
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

    fn alarm_state(state: &Arc<CloudWatchState>, name: &str) -> String {
        state
            .alarms
            .get(name)
            .map(|a| a.state_value.clone())
            .unwrap_or_else(|| "<missing>".to_string())
    }

    #[test]
    fn put_metric_data_flips_alarm_to_alarm_then_ok() {
        let state = Arc::new(CloudWatchState::default());
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
        let state = Arc::new(CloudWatchState::default());
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
