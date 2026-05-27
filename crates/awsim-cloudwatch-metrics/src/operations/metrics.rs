use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::sqlite_store::{MetricDatumRow, parse_timestamp_ms};
use crate::state::{CloudWatchState, Dimension};

fn chrono_now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// 15-day default retention. Mirrors the AWS retention for high-
/// resolution metric data and is plenty for local dev. Configurable
/// per-account/region eventually; hard-coded for now.
const DEFAULT_RETENTION_MS: i64 = 15 * 86_400_000;

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

fn dimensions_to_json(dims: &[Dimension]) -> Value {
    Value::Array(
        dims.iter()
            .map(|d| {
                json!({
                    "Name": d.name,
                    "Value": d.value,
                })
            })
            .collect(),
    )
}

fn json_to_dimensions(v: &Value) -> Vec<Dimension> {
    v.as_array()
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

fn require_sqlite(state: &Arc<CloudWatchState>) -> Result<Arc<crate::SqliteStore>, AwsError> {
    state
        .sqlite()
        .map(Arc::clone)
        .ok_or_else(|| AwsError::internal("CloudWatch Metrics sqlite store not initialised"))
}

/// PutMetricData
pub fn put_metric_data(
    state: &Arc<CloudWatchState>,
    input: &Value,
    ctx: &RequestContext,
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

    // AWS documents a per-request cap of 1000 MetricData entries.
    // Beyond that, real CloudWatch returns InvalidParameterValue.
    const MAX_METRIC_DATA_PER_REQUEST: usize = 1000;
    if metric_data.len() > MAX_METRIC_DATA_PER_REQUEST {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            format!(
                "MetricData has {} entries; the maximum allowed per PutMetricData request is {MAX_METRIC_DATA_PER_REQUEST}.",
                metric_data.len()
            ),
        ));
    }

    let mut rows: Vec<MetricDatumRow> = Vec::with_capacity(metric_data.len());
    let mut unprocessed: Vec<Value> = Vec::new();

    // Per-datum validation gathers failures into UnprocessedMetricData
    // instead of short-circuiting the whole batch. Top-level errors
    // (missing Namespace, oversize batch) still hard-fail above.
    for datum in metric_data {
        match validate_datum(&namespace, datum) {
            Ok(row) => rows.push(row),
            Err((code, message)) => unprocessed.push(json!({
                "MetricData": datum.clone(),
                "ErrorCode": code,
                "ErrorMessage": message,
            })),
        }
    }

    let sqlite = require_sqlite(state)?;
    sqlite.put_datapoints(&ctx.account_id, &ctx.region, &rows)?;

    // Best-effort retention sweep on every write — cheap when the
    // table is already trimmed; one indexed DELETE otherwise.
    let cutoff = parse_timestamp_ms(&chrono_now()).saturating_sub(DEFAULT_RETENTION_MS);
    let _ = sqlite.trim_older_than(&ctx.account_id, &ctx.region, cutoff);

    super::alarms::evaluate_alarms(state, ctx);

    let mut resp = serde_json::Map::new();
    if !unprocessed.is_empty() {
        resp.insert("UnprocessedMetricData".into(), Value::Array(unprocessed));
    }
    Ok(Value::Object(resp))
}

fn validate_datum(namespace: &str, datum: &Value) -> Result<MetricDatumRow, (String, String)> {
    let metric_name = datum
        .get("MetricName")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            (
                "InvalidParameterValue".to_string(),
                "MetricName is required".to_string(),
            )
        })?
        .to_string();
    let raw_value = datum.get("Value").and_then(Value::as_f64);
    let stats = datum.get("StatisticValues").and_then(Value::as_object);
    let value = match (raw_value, stats) {
        (Some(v), None) => v,
        (None, Some(s)) => {
            let sum = s.get("Sum").and_then(Value::as_f64).ok_or_else(|| {
                (
                    "InvalidParameterValue".to_string(),
                    format!("MetricDatum `{metric_name}` StatisticValues requires Sum."),
                )
            })?;
            let count = s
                .get("SampleCount")
                .and_then(Value::as_f64)
                .ok_or_else(|| {
                    (
                        "InvalidParameterValue".to_string(),
                        format!(
                            "MetricDatum `{metric_name}` StatisticValues requires SampleCount."
                        ),
                    )
                })?;
            if count <= 0.0 {
                return Err((
                    "InvalidParameterValue".to_string(),
                    format!("MetricDatum `{metric_name}` StatisticValues.SampleCount must be > 0."),
                ));
            }
            sum / count
        }
        (Some(_), Some(_)) => {
            return Err((
                "InvalidParameterValue".to_string(),
                format!(
                    "MetricDatum `{metric_name}` must specify either Value or StatisticValues, not both."
                ),
            ));
        }
        (None, None) => 0.0,
    };
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
    let ts_ms = parse_timestamp_ms(&timestamp);
    let storage_resolution = datum
        .get("StorageResolution")
        .and_then(Value::as_i64)
        .unwrap_or(60);
    if !matches!(storage_resolution, 1 | 60) {
        return Err((
            "InvalidParameterValue".to_string(),
            format!(
                "MetricDatum `{metric_name}` StorageResolution must be 1 or 60 (got {storage_resolution})."
            ),
        ));
    }
    Ok(MetricDatumRow {
        namespace: namespace.to_string(),
        metric_name,
        value,
        unit,
        timestamp,
        ts_ms,
        dimensions_json: dimensions_to_json(&dimensions),
        storage_resolution: storage_resolution as u32,
    })
}

/// ListMetrics
pub fn list_metrics(
    state: &Arc<CloudWatchState>,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_namespace = input.get("Namespace").and_then(Value::as_str);
    let filter_metric_name = input.get("MetricName").and_then(Value::as_str);

    // Dimensions filter: AWS treats each entry as a required match on
    // the metric's dimensions. An entry with only a Name matches any
    // metric that carries that dimension; with Name+Value, the value
    // must equal exactly.
    let dim_filters: Vec<(String, Option<String>)> = input
        .get("Dimensions")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|d| {
                    let name = d.get("Name").and_then(Value::as_str)?.to_string();
                    let value = d.get("Value").and_then(Value::as_str).map(str::to_string);
                    Some((name, value))
                })
                .collect()
        })
        .unwrap_or_default();

    let sqlite = require_sqlite(state)?;
    let rows = sqlite.list_metrics(
        &ctx.account_id,
        &ctx.region,
        filter_namespace,
        filter_metric_name,
    )?;

    let metrics: Vec<Value> = rows
        .into_iter()
        .filter(|(_, _, dims)| {
            if dim_filters.is_empty() {
                return true;
            }
            let stored = json_to_dimensions(dims);
            dim_filters.iter().all(|(name, value)| {
                stored
                    .iter()
                    .any(|d| d.name == *name && value.as_ref().is_none_or(|v| d.value == *v))
            })
        })
        .map(|(ns, name, dims)| {
            json!({
                "Namespace": ns,
                "MetricName": name,
                "Dimensions": dims,
            })
        })
        .collect();

    Ok(json!({ "Metrics": metrics }))
}

/// GetMetricStatistics
pub fn get_metric_statistics(
    state: &Arc<CloudWatchState>,
    input: &Value,
    ctx: &RequestContext,
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
    let start_ms = input
        .get("StartTime")
        .and_then(Value::as_str)
        .map(parse_timestamp_ms);
    let end_ms = input
        .get("EndTime")
        .and_then(Value::as_str)
        .map(parse_timestamp_ms);

    let sqlite = require_sqlite(state)?;
    let mut rows = sqlite.get_datapoints(
        &ctx.account_id,
        &ctx.region,
        namespace,
        metric_name,
        start_ms,
        end_ms,
    )?;

    // Dimensions parameter narrows to datapoints whose stored
    // dimensions match exactly (same set of name/value pairs). AWS
    // treats missing Dimensions as "match any" — so when the caller
    // doesn't supply any, we skip the filter.
    if let Some(dims_arr) = input.get("Dimensions").and_then(Value::as_array)
        && !dims_arr.is_empty()
    {
        let wanted = parse_dimensions(&Value::Array(dims_arr.clone()));
        rows.retain(|r| {
            let stored = json_to_dimensions(&r.dimensions_json);
            stored.len() == wanted.len()
                && wanted.iter().all(|w| {
                    stored
                        .iter()
                        .any(|s| s.name == w.name && s.value == w.value)
                })
        });
    }

    // GetMetricStatistics Period must be a valid resolution for the
    // metric. For standard-resolution metrics, only multiples of 60 are
    // accepted; high-resolution metrics additionally accept 1/5/10/30.
    // Period defaults to 60 when omitted.
    if let Some(period) = input.get("Period").and_then(Value::as_i64) {
        let any_high_res = rows.iter().any(|r| r.storage_resolution == 1);
        let valid = if any_high_res {
            matches!(period, 1 | 5 | 10 | 30) || (period > 0 && period % 60 == 0)
        } else {
            period > 0 && period % 60 == 0
        };
        if !valid {
            return Err(AwsError::bad_request(
                "InvalidParameterValue",
                format!(
                    "Period {period} is not valid for this metric; \
                     standard-resolution metrics require a multiple of 60, \
                     high-resolution metrics additionally accept 1, 5, 10, or 30."
                ),
            ));
        }
    }

    let values: Vec<f64> = rows.iter().map(|r| r.value).collect();
    let first_unit = rows
        .first()
        .map(|r| r.unit.clone())
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
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let queries = input
        .get("MetricDataQueries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let start_ms = input
        .get("StartTime")
        .and_then(Value::as_str)
        .map(parse_timestamp_ms);
    let end_ms = input
        .get("EndTime")
        .and_then(Value::as_str)
        .map(parse_timestamp_ms);

    let sqlite = require_sqlite(state)?;
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
            let rows =
                sqlite.get_datapoints(&ctx.account_id, &ctx.region, ns, mn, start_ms, end_ms)?;
            let vals: Vec<Value> = rows.iter().map(|r| json!(r.value)).collect();
            let ts: Vec<Value> = rows.iter().map(|r| json!(r.timestamp)).collect();
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

/// Internal helper for the alarm evaluator: pull all datapoints for
/// `(namespace, metric_name)` whose ts is within `[start_ms, end_ms]`.
/// Returns `(value, dimensions, timestamp_string)` tuples so the
/// evaluator can do its filtering / aggregation without re-querying.
pub(crate) fn datapoints_for_alarm(
    state: &Arc<CloudWatchState>,
    ctx: &RequestContext,
    namespace: &str,
    metric_name: &str,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<(f64, Vec<Dimension>, String)>, AwsError> {
    let sqlite = require_sqlite(state)?;
    let rows = sqlite.get_datapoints(
        &ctx.account_id,
        &ctx.region,
        namespace,
        metric_name,
        Some(start_ms),
        Some(end_ms),
    )?;
    Ok(rows
        .into_iter()
        .map(|r| (r.value, json_to_dimensions(&r.dimensions_json), r.timestamp))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite_store::SqliteStore;
    use crate::state::CloudWatchState;
    use std::sync::Arc;

    fn ctx() -> RequestContext {
        RequestContext::new("monitoring", "us-east-1")
    }

    fn fresh_state() -> Arc<CloudWatchState> {
        let state = Arc::new(CloudWatchState::default());
        let path = std::env::temp_dir().join(format!("awsim-cwm-list-{}.db", uuid::Uuid::new_v4()));
        let sqlite = Arc::new(SqliteStore::open(path).unwrap());
        state.set_sqlite(sqlite);
        state
    }

    fn put_datum(
        state: &Arc<CloudWatchState>,
        ctx: &RequestContext,
        namespace: &str,
        name: &str,
        dims: Value,
    ) {
        put_metric_data(
            state,
            &json!({
                "Namespace": namespace,
                "MetricData": [{
                    "MetricName": name,
                    "Value": 1.0,
                    "Dimensions": dims,
                }],
            }),
            ctx,
        )
        .unwrap();
    }

    #[test]
    fn put_metric_data_uses_statistic_values_mean() {
        let state = fresh_state();
        let ctx = ctx();
        put_metric_data(
            &state,
            &json!({
                "Namespace": "App",
                "MetricData": [{
                    "MetricName": "RequestSize",
                    "StatisticValues": {
                        "Sum": 1000.0,
                        "SampleCount": 4.0,
                        "Minimum": 200.0,
                        "Maximum": 300.0,
                    },
                }],
            }),
            &ctx,
        )
        .unwrap();
        let sqlite = require_sqlite(&state).unwrap();
        let rows = sqlite
            .get_datapoints(
                &ctx.account_id,
                &ctx.region,
                "App",
                "RequestSize",
                None,
                None,
            )
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].value, 250.0);
    }

    #[test]
    fn put_metric_data_rejects_both_value_and_statistic_values() {
        let state = fresh_state();
        let ctx = ctx();
        let resp = put_metric_data(
            &state,
            &json!({
                "Namespace": "App",
                "MetricData": [{
                    "MetricName": "Bad",
                    "Value": 1.0,
                    "StatisticValues": { "Sum": 1.0, "SampleCount": 1.0 },
                }],
            }),
            &ctx,
        )
        .unwrap();
        let unprocessed = resp["UnprocessedMetricData"].as_array().unwrap();
        assert_eq!(unprocessed.len(), 1);
        assert_eq!(unprocessed[0]["ErrorCode"], "InvalidParameterValue");
        assert!(
            unprocessed[0]["ErrorMessage"]
                .as_str()
                .unwrap()
                .contains("either Value or StatisticValues")
        );
    }

    #[test]
    fn put_metric_data_rejects_invalid_storage_resolution() {
        let state = fresh_state();
        let ctx = ctx();
        let resp = put_metric_data(
            &state,
            &json!({
                "Namespace": "App",
                "MetricData": [{
                    "MetricName": "M",
                    "Value": 1.0,
                    "StorageResolution": 30,
                }],
            }),
            &ctx,
        )
        .unwrap();
        let unprocessed = resp["UnprocessedMetricData"].as_array().unwrap();
        assert_eq!(unprocessed.len(), 1);
        assert!(
            unprocessed[0]["ErrorMessage"]
                .as_str()
                .unwrap()
                .contains("StorageResolution")
        );
    }

    #[test]
    fn get_metric_statistics_rejects_high_res_only_period_on_standard_metric() {
        let state = fresh_state();
        let ctx = ctx();
        put_metric_data(
            &state,
            &json!({
                "Namespace": "App",
                "MetricData": [{
                    "MetricName": "M",
                    "Value": 1.0,
                }],
            }),
            &ctx,
        )
        .unwrap();
        let err = get_metric_statistics(
            &state,
            &json!({
                "Namespace": "App",
                "MetricName": "M",
                "Statistics": ["Sum"],
                "Period": 10,
            }),
            &ctx,
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn get_metric_statistics_accepts_period_10_for_high_res_metric() {
        let state = fresh_state();
        let ctx = ctx();
        put_metric_data(
            &state,
            &json!({
                "Namespace": "App",
                "MetricData": [{
                    "MetricName": "M",
                    "Value": 1.0,
                    "StorageResolution": 1,
                }],
            }),
            &ctx,
        )
        .unwrap();
        get_metric_statistics(
            &state,
            &json!({
                "Namespace": "App",
                "MetricName": "M",
                "Statistics": ["Sum"],
                "Period": 10,
            }),
            &ctx,
        )
        .unwrap();
    }

    #[test]
    fn put_metric_data_rejects_zero_sample_count() {
        let state = fresh_state();
        let ctx = ctx();
        let resp = put_metric_data(
            &state,
            &json!({
                "Namespace": "App",
                "MetricData": [{
                    "MetricName": "Zero",
                    "StatisticValues": { "Sum": 5.0, "SampleCount": 0.0 },
                }],
            }),
            &ctx,
        )
        .unwrap();
        let unprocessed = resp["UnprocessedMetricData"].as_array().unwrap();
        assert_eq!(unprocessed[0]["ErrorCode"], "InvalidParameterValue");
    }

    #[test]
    fn put_metric_data_partial_success_persists_good_datum_and_reports_bad() {
        let state = fresh_state();
        let ctx = ctx();
        let resp = put_metric_data(
            &state,
            &json!({
                "Namespace": "App",
                "MetricData": [
                    { "MetricName": "Ok", "Value": 1.0 },
                    { "MetricName": "Bad", "StorageResolution": 30, "Value": 1.0 }
                ],
            }),
            &ctx,
        )
        .unwrap();
        let unprocessed = resp["UnprocessedMetricData"].as_array().unwrap();
        assert_eq!(unprocessed.len(), 1);
        assert_eq!(unprocessed[0]["MetricData"]["MetricName"], "Bad");
    }

    #[test]
    fn list_metrics_filters_by_dimension_name_value() {
        let state = fresh_state();
        let ctx = ctx();
        put_datum(
            &state,
            &ctx,
            "App",
            "Requests",
            json!([{ "Name": "Service", "Value": "auth" }]),
        );
        put_datum(
            &state,
            &ctx,
            "App",
            "Requests",
            json!([{ "Name": "Service", "Value": "billing" }]),
        );

        let resp = list_metrics(
            &state,
            &json!({
                "Namespace": "App",
                "Dimensions": [{ "Name": "Service", "Value": "auth" }],
            }),
            &ctx,
        )
        .unwrap();
        let metrics = resp["Metrics"].as_array().unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0]["Dimensions"][0]["Value"], "auth");
    }

    #[test]
    fn list_metrics_filters_by_dimension_name_only() {
        let state = fresh_state();
        let ctx = ctx();
        put_datum(
            &state,
            &ctx,
            "App",
            "Latency",
            json!([{ "Name": "Region", "Value": "us-east-1" }]),
        );
        put_datum(
            &state,
            &ctx,
            "App",
            "Latency",
            json!([{ "Name": "InstanceId", "Value": "i-123" }]),
        );

        let resp = list_metrics(
            &state,
            &json!({
                "Namespace": "App",
                "Dimensions": [{ "Name": "Region" }],
            }),
            &ctx,
        )
        .unwrap();
        let metrics = resp["Metrics"].as_array().unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0]["Dimensions"][0]["Name"], "Region");
    }

    #[test]
    fn get_metric_statistics_filters_by_dimensions() {
        let state = fresh_state();
        let ctx = ctx();
        // Two datapoints under the same metric name, different dims.
        put_datum(
            &state,
            &ctx,
            "App",
            "Latency",
            json!([{ "Name": "Service", "Value": "auth" }]),
        );
        put_datum(
            &state,
            &ctx,
            "App",
            "Latency",
            json!([{ "Name": "Service", "Value": "billing" }]),
        );

        // Asking for billing only must return a single datapoint.
        let resp = get_metric_statistics(
            &state,
            &json!({
                "Namespace": "App",
                "MetricName": "Latency",
                "Statistics": ["Sum"],
                "Dimensions": [{ "Name": "Service", "Value": "billing" }],
            }),
            &ctx,
        )
        .unwrap();
        let dps = resp["Datapoints"].as_array().unwrap();
        assert_eq!(dps.len(), 1);
        assert_eq!(dps[0]["SampleCount"], 1.0);

        // No dimensions filter aggregates both.
        let resp = get_metric_statistics(
            &state,
            &json!({
                "Namespace": "App",
                "MetricName": "Latency",
                "Statistics": ["Sum"],
            }),
            &ctx,
        )
        .unwrap();
        let dps = resp["Datapoints"].as_array().unwrap();
        assert_eq!(dps[0]["SampleCount"], 2.0);
    }
}
