use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{CloudWatchState, MetricAlarm};

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

fn alarm_to_json(alarm: &MetricAlarm) -> Value {
    json!({
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
    })
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
    };

    state.alarms.insert(alarm_name, alarm);
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

    Ok(json!({}))
}
