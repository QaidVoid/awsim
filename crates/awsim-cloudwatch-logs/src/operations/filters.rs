use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{InsightsQuery, LogsState, MetricFilter, QueryDefinition, SubscriptionFilter};

fn now_millis() -> u64 {
    crate::state::now_millis()
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

fn require_log_group<'a>(
    state: &'a LogsState,
    name: &str,
) -> Result<dashmap::mapref::one::Ref<'a, String, crate::state::LogGroup>, AwsError> {
    state.log_groups.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {name}"),
        )
    })
}

// ---------------------------------------------------------------------------
// TagResource / UntagResource / ListTagsForResource (newer resource-based API)
// ---------------------------------------------------------------------------

/// Extract log group name from an ARN or return it as-is if it looks like a name.
fn log_group_from_arn(arn: &str) -> &str {
    // arn:aws:logs:{region}:{account}:log-group:{name}
    if let Some(rest) = arn.strip_prefix("arn:aws:logs:") {
        // Find the "log-group:" segment
        if let Some(pos) = rest.find(":log-group:") {
            return &rest[pos + ":log-group:".len()..];
        }
    }
    arn
}

pub fn tag_resource(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["resourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "resourceArn is required")
    })?;

    let tags = input["tags"]
        .as_object()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "tags is required"))?;

    let name = log_group_from_arn(resource_arn);
    let mut group = state.log_groups.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {name}"),
        )
    })?;

    for (k, v) in tags {
        if let Some(s) = v.as_str() {
            group.tags.insert(k.clone(), s.to_string());
        }
    }

    Ok(json!({}))
}

pub fn untag_resource(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["resourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "resourceArn is required")
    })?;

    let tag_keys = input["tagKeys"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "tagKeys is required"))?;

    let name = log_group_from_arn(resource_arn);
    let mut group = state.log_groups.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {name}"),
        )
    })?;

    for key in tag_keys {
        if let Some(k) = key.as_str() {
            group.tags.remove(k);
        }
    }

    Ok(json!({}))
}

pub fn list_tags_for_resource(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["resourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "resourceArn is required")
    })?;

    let name = log_group_from_arn(resource_arn);
    let group = state.log_groups.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {name}"),
        )
    })?;

    let tags: serde_json::Map<String, Value> = group
        .tags
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();

    Ok(json!({ "tags": tags }))
}

// ---------------------------------------------------------------------------
// PutSubscriptionFilter
// ---------------------------------------------------------------------------

pub fn put_subscription_filter(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let log_group_name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    let filter_name = input["filterName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "filterName is required")
    })?;

    let filter_pattern = input["filterPattern"].as_str().unwrap_or("").to_string();

    let destination_arn = input["destinationArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "destinationArn is required")
    })?;

    require_log_group(state, log_group_name)?;

    // AWS validates RoleArn shape upfront — must be an IAM role ARN.
    // Persist for downstream delivery (which uses it to assume into
    // the destination's account).
    let role_arn = match input["roleArn"].as_str() {
        Some(s) if !s.is_empty() => {
            if !s.starts_with("arn:aws:iam::") || !s.contains(":role/") {
                return Err(AwsError::bad_request(
                    "InvalidParameterException",
                    format!("roleArn `{s}` must be an IAM role ARN."),
                ));
            }
            Some(s.to_string())
        }
        _ => None,
    };

    // Distribution: AWS accepts Random (default) and ByLogStream.
    // The latter is honoured by the delivery loop when sharding events
    // across Kinesis stream shards.
    let distribution = input["distribution"]
        .as_str()
        .unwrap_or("Random")
        .to_string();
    if !matches!(distribution.as_str(), "Random" | "ByLogStream") {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!("distribution `{distribution}` must be Random or ByLogStream."),
        ));
    }

    let filter = SubscriptionFilter {
        filter_name: filter_name.to_string(),
        log_group_name: log_group_name.to_string(),
        filter_pattern,
        destination_arn: destination_arn.to_string(),
        creation_time: now_millis(),
        role_arn,
        distribution,
    };

    info!(
        log_group = log_group_name,
        filter_name, "PutSubscriptionFilter"
    );
    state.subscription_filters.insert(
        (log_group_name.to_string(), filter_name.to_string()),
        filter,
    );

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DescribeSubscriptionFilters
// ---------------------------------------------------------------------------

pub fn describe_subscription_filters(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let log_group_name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    let filter_name_prefix = input["filterNamePrefix"].as_str().unwrap_or("");
    let limit = input["limit"].as_u64().unwrap_or(50) as usize;

    let mut filters: Vec<Value> = state
        .subscription_filters
        .iter()
        .filter(|e| e.key().0 == log_group_name && e.filter_name.starts_with(filter_name_prefix))
        .map(|e| {
            let mut obj = json!({
                "filterName": e.filter_name,
                "logGroupName": e.log_group_name,
                "filterPattern": e.filter_pattern,
                "destinationArn": e.destination_arn,
                "creationTime": e.creation_time,
                "distribution": e.distribution,
            });
            if let Some(ref r) = e.role_arn {
                obj["roleArn"] = json!(r);
            }
            obj
        })
        .take(limit)
        .collect();

    filters.sort_by(|a, b| {
        a["filterName"]
            .as_str()
            .unwrap_or("")
            .cmp(b["filterName"].as_str().unwrap_or(""))
    });

    Ok(json!({ "subscriptionFilters": filters }))
}

// ---------------------------------------------------------------------------
// DeleteSubscriptionFilter
// ---------------------------------------------------------------------------

pub fn delete_subscription_filter(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let log_group_name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    let filter_name = input["filterName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "filterName is required")
    })?;

    state
        .subscription_filters
        .remove(&(log_group_name.to_string(), filter_name.to_string()))
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!(
                    "Subscription filter {filter_name} not found for log group {log_group_name}"
                ),
            )
        })?;

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// PutMetricFilter
// ---------------------------------------------------------------------------

pub fn put_metric_filter(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let log_group_name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    let filter_name = input["filterName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "filterName is required")
    })?;

    let filter_pattern = input["filterPattern"].as_str().unwrap_or("").to_string();

    let metric_transformations = input["metricTransformations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if metric_transformations.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "metricTransformations must contain at least one transformation.",
        ));
    }
    for t in &metric_transformations {
        validate_metric_transformation(t)?;
    }

    require_log_group(state, log_group_name)?;

    let filter = MetricFilter {
        filter_name: filter_name.to_string(),
        log_group_name: log_group_name.to_string(),
        filter_pattern,
        metric_transformations,
        creation_time: now_millis(),
    };

    info!(log_group = log_group_name, filter_name, "PutMetricFilter");
    state.metric_filters.insert(
        (log_group_name.to_string(), filter_name.to_string()),
        filter,
    );

    Ok(json!({}))
}

/// AWS-documented CloudWatch `Unit` enum values for a metric.
/// `None` is the documented default. A typo here is one of the
/// quietest ways to break alarms downstream, so reject up front.
const METRIC_UNITS: &[&str] = &[
    "Seconds",
    "Microseconds",
    "Milliseconds",
    "Bytes",
    "Kilobytes",
    "Megabytes",
    "Gigabytes",
    "Terabytes",
    "Bits",
    "Kilobits",
    "Megabits",
    "Gigabits",
    "Terabits",
    "Percent",
    "Count",
    "Bytes/Second",
    "Kilobytes/Second",
    "Megabytes/Second",
    "Gigabytes/Second",
    "Terabytes/Second",
    "Bits/Second",
    "Kilobits/Second",
    "Megabits/Second",
    "Gigabits/Second",
    "Terabits/Second",
    "Count/Second",
    "None",
];

/// Validate a single entry of `metricTransformations[]`. AWS bounds
/// `dimensions` at 3 entries, the metric `unit` to the documented
/// enum, and rejects `defaultValue` that doesn't parse as a number.
/// `incrementBy` (an alias of `metricValue=1`) is implied when
/// `metricValue` is the literal `"1"`.
fn validate_metric_transformation(t: &Value) -> Result<(), AwsError> {
    let obj = t.as_object().ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterException",
            "metricTransformations[] entries must be objects.",
        )
    })?;
    let metric_name = obj
        .get("metricName")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterException",
                "metricTransformations[].metricName is required.",
            )
        })?;
    if metric_name.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "metricTransformations[].metricName must not be empty.",
        ));
    }
    let metric_namespace = obj
        .get("metricNamespace")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterException",
                "metricTransformations[].metricNamespace is required.",
            )
        })?;
    if metric_namespace.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "metricTransformations[].metricNamespace must not be empty.",
        ));
    }
    let metric_value = obj
        .get("metricValue")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterException",
                "metricTransformations[].metricValue is required.",
            )
        })?;
    if metric_value.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "metricTransformations[].metricValue must not be empty.",
        ));
    }
    if let Some(unit) = obj.get("unit") {
        let u = unit.as_str().ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterException",
                "metricTransformations[].unit must be a string.",
            )
        })?;
        if !METRIC_UNITS.contains(&u) {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                format!("metricTransformations[].unit `{u}` is not a valid CloudWatch unit."),
            ));
        }
    }
    if let Some(dv) = obj.get("defaultValue")
        && !dv.is_null()
        && dv.as_f64().is_none()
    {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "metricTransformations[].defaultValue must be a number.",
        ));
    }
    if let Some(dims) = obj.get("dimensions") {
        let map = dims.as_object().ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterException",
                "metricTransformations[].dimensions must be a map of string to string.",
            )
        })?;
        if map.len() > 3 {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                "metricTransformations[].dimensions accepts at most 3 entries.",
            ));
        }
        for (k, v) in map {
            if k.is_empty() {
                return Err(AwsError::bad_request(
                    "InvalidParameterException",
                    "metricTransformations[].dimensions has an empty key.",
                ));
            }
            if v.as_str().is_none() {
                return Err(AwsError::bad_request(
                    "InvalidParameterException",
                    "metricTransformations[].dimensions values must be strings.",
                ));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// DescribeMetricFilters
// ---------------------------------------------------------------------------

pub fn describe_metric_filters(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let log_group_name = input["logGroupName"].as_str();
    let filter_name_prefix = input["filterNamePrefix"].as_str().unwrap_or("");
    let limit = input["limit"].as_u64().unwrap_or(50) as usize;

    let mut filters: Vec<Value> = state
        .metric_filters
        .iter()
        .filter(|e| {
            log_group_name.is_none_or(|lg| e.key().0 == lg)
                && e.filter_name.starts_with(filter_name_prefix)
        })
        .map(|e| {
            json!({
                "filterName": e.filter_name,
                "logGroupName": e.log_group_name,
                "filterPattern": e.filter_pattern,
                "metricTransformations": e.metric_transformations,
                "creationTime": e.creation_time,
            })
        })
        .take(limit)
        .collect();

    filters.sort_by(|a, b| {
        a["filterName"]
            .as_str()
            .unwrap_or("")
            .cmp(b["filterName"].as_str().unwrap_or(""))
    });

    Ok(json!({ "metricFilters": filters }))
}

// ---------------------------------------------------------------------------
// DeleteMetricFilter
// ---------------------------------------------------------------------------

pub fn delete_metric_filter(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let log_group_name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    let filter_name = input["filterName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "filterName is required")
    })?;

    state
        .metric_filters
        .remove(&(log_group_name.to_string(), filter_name.to_string()))
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Metric filter {filter_name} not found for log group {log_group_name}"),
            )
        })?;

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// PutQueryDefinition
// ---------------------------------------------------------------------------

pub fn put_query_definition(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "name is required"))?;

    let query_string = input["queryString"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "queryString is required")
    })?;

    let log_group_names: Vec<String> = input["logGroupNames"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    // Check if we're updating an existing definition
    let query_definition_id = if let Some(existing_id) = input["queryDefinitionId"].as_str() {
        existing_id.to_string()
    } else {
        new_id()
    };

    let def = QueryDefinition {
        query_definition_id: query_definition_id.clone(),
        name: name.to_string(),
        query_string: query_string.to_string(),
        log_group_names,
    };

    state
        .query_definitions
        .insert(query_definition_id.clone(), def);

    Ok(json!({ "queryDefinitionId": query_definition_id }))
}

// ---------------------------------------------------------------------------
// DescribeQueryDefinitions
// ---------------------------------------------------------------------------

pub fn describe_query_definitions(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name_prefix = input["queryDefinitionNamePrefix"].as_str().unwrap_or("");
    let max_results = input["maxResults"].as_u64().unwrap_or(50) as usize;

    let mut defs: Vec<Value> = state
        .query_definitions
        .iter()
        .filter(|e| e.name.starts_with(name_prefix))
        .map(|e| {
            json!({
                "queryDefinitionId": e.query_definition_id,
                "name": e.name,
                "queryString": e.query_string,
                "logGroupNames": e.log_group_names,
            })
        })
        .take(max_results)
        .collect();

    defs.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });

    Ok(json!({ "queryDefinitions": defs }))
}

// ---------------------------------------------------------------------------
// DeleteQueryDefinition
// ---------------------------------------------------------------------------

pub fn delete_query_definition(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let query_definition_id = input["queryDefinitionId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "queryDefinitionId is required")
    })?;

    let existed = state
        .query_definitions
        .remove(query_definition_id)
        .is_some();

    Ok(json!({ "success": existed }))
}

// ---------------------------------------------------------------------------
// StartQuery
// ---------------------------------------------------------------------------

pub fn start_query(
    state: &LogsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let query_id = new_id();

    let query = InsightsQuery {
        query_id: query_id.clone(),
        status: "Complete".to_string(),
    };

    state.insights_queries.insert(query_id.clone(), query);

    Ok(json!({ "queryId": query_id }))
}

// ---------------------------------------------------------------------------
// GetQueryResults
// ---------------------------------------------------------------------------

pub fn get_query_results(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let query_id = input["queryId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "queryId is required"))?;

    let query = state.insights_queries.get(query_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Query {query_id} not found"),
        )
    })?;

    Ok(json!({
        "queryId": query.query_id,
        "status": query.status,
        "results": [],
        "statistics": {
            "recordsMatched": 0.0,
            "recordsScanned": 0.0,
            "bytesScanned": 0.0,
        },
    }))
}

// ---------------------------------------------------------------------------
// StopQuery
// ---------------------------------------------------------------------------

pub fn stop_query(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let query_id = input["queryId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "queryId is required"))?;

    if let Some(mut q) = state.insights_queries.get_mut(query_id) {
        q.status = "Cancelled".to_string();
    }

    Ok(json!({ "success": true }))
}

#[cfg(test)]
mod subscription_filter_tests {
    use super::*;
    use crate::SqliteStore;
    use crate::operations::log_groups::create_log_group;
    use std::sync::Arc;

    fn ctx() -> RequestContext {
        RequestContext::new("logs", "us-east-1")
    }

    fn fresh_state_with_group(name: &str) -> LogsState {
        let dir = std::env::temp_dir().join(format!("awsim-logs-sub-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Arc::new(SqliteStore::open(dir.join("logs.db")).unwrap());
        let state = LogsState::default();
        state.set_sqlite(store);
        create_log_group(&state, &json!({ "logGroupName": name }), &ctx()).unwrap();
        state
    }

    #[test]
    fn defaults_distribution_to_random() {
        let state = fresh_state_with_group("g");
        put_subscription_filter(
            &state,
            &json!({
                "logGroupName": "g",
                "filterName": "f1",
                "destinationArn": "arn:aws:lambda:us-east-1:000000000000:function:fn",
                "filterPattern": ""
            }),
            &ctx(),
        )
        .unwrap();
        let resp =
            describe_subscription_filters(&state, &json!({ "logGroupName": "g" }), &ctx()).unwrap();
        let first = &resp["subscriptionFilters"][0];
        assert_eq!(first["distribution"], "Random");
    }

    #[test]
    fn accepts_by_log_stream_distribution() {
        let state = fresh_state_with_group("g");
        put_subscription_filter(
            &state,
            &json!({
                "logGroupName": "g",
                "filterName": "f1",
                "destinationArn": "arn:aws:lambda:us-east-1:000000000000:function:fn",
                "distribution": "ByLogStream"
            }),
            &ctx(),
        )
        .unwrap();
        let resp =
            describe_subscription_filters(&state, &json!({ "logGroupName": "g" }), &ctx()).unwrap();
        assert_eq!(
            resp["subscriptionFilters"][0]["distribution"],
            "ByLogStream"
        );
    }

    #[test]
    fn rejects_unknown_distribution() {
        let state = fresh_state_with_group("g");
        let err = put_subscription_filter(
            &state,
            &json!({
                "logGroupName": "g",
                "filterName": "f1",
                "destinationArn": "arn:aws:lambda:us-east-1:000000000000:function:fn",
                "distribution": "RoundRobin"
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn rejects_non_role_arn_role_arn() {
        let state = fresh_state_with_group("g");
        let err = put_subscription_filter(
            &state,
            &json!({
                "logGroupName": "g",
                "filterName": "f1",
                "destinationArn": "arn:aws:lambda:us-east-1:000000000000:function:fn",
                "roleArn": "arn:aws:iam::000000000000:user/alice"
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn accepts_well_formed_role_arn() {
        let state = fresh_state_with_group("g");
        put_subscription_filter(
            &state,
            &json!({
                "logGroupName": "g",
                "filterName": "f1",
                "destinationArn": "arn:aws:lambda:us-east-1:000000000000:function:fn",
                "roleArn": "arn:aws:iam::000000000000:role/cwlogs"
            }),
            &ctx(),
        )
        .unwrap();
        let resp =
            describe_subscription_filters(&state, &json!({ "logGroupName": "g" }), &ctx()).unwrap();
        assert_eq!(
            resp["subscriptionFilters"][0]["roleArn"],
            "arn:aws:iam::000000000000:role/cwlogs"
        );
    }
}

#[cfg(test)]
mod metric_transformation_tests {
    use super::*;
    use crate::state::LogsState;

    fn ctx() -> RequestContext {
        RequestContext::new("logs", "us-east-1")
    }

    fn state_with_group() -> LogsState {
        let state = LogsState::default();
        crate::operations::log_groups::create_log_group(
            &state,
            &json!({ "logGroupName": "g" }),
            &ctx(),
        )
        .unwrap();
        state
    }

    fn valid_xform() -> Value {
        json!({
            "metricName": "ErrorCount",
            "metricNamespace": "MyApp",
            "metricValue": "1",
            "defaultValue": 0,
            "unit": "Count",
            "dimensions": { "Service": "$.svc" }
        })
    }

    #[test]
    fn accepts_well_formed_transformation() {
        let state = state_with_group();
        put_metric_filter(
            &state,
            &json!({
                "logGroupName": "g",
                "filterName": "f",
                "filterPattern": "ERROR",
                "metricTransformations": [valid_xform()],
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn rejects_empty_transformations_array() {
        let state = state_with_group();
        let err = put_metric_filter(
            &state,
            &json!({
                "logGroupName": "g",
                "filterName": "f",
                "filterPattern": "ERROR",
                "metricTransformations": [],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn rejects_missing_metric_name() {
        let state = state_with_group();
        let mut x = valid_xform();
        x.as_object_mut().unwrap().remove("metricName");
        let err = put_metric_filter(
            &state,
            &json!({
                "logGroupName": "g",
                "filterName": "f",
                "metricTransformations": [x],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert!(err.message.contains("metricName"), "{err:?}");
    }

    #[test]
    fn rejects_unknown_unit() {
        let state = state_with_group();
        let mut x = valid_xform();
        x["unit"] = json!("Watts");
        let err = put_metric_filter(
            &state,
            &json!({
                "logGroupName": "g",
                "filterName": "f",
                "metricTransformations": [x],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert!(err.message.contains("unit"), "{err:?}");
    }

    #[test]
    fn rejects_more_than_three_dimensions() {
        let state = state_with_group();
        let mut x = valid_xform();
        x["dimensions"] = json!({"a": "1", "b": "2", "c": "3", "d": "4"});
        let err = put_metric_filter(
            &state,
            &json!({
                "logGroupName": "g",
                "filterName": "f",
                "metricTransformations": [x],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert!(err.message.contains("3"), "{err:?}");
    }

    #[test]
    fn rejects_non_number_default_value() {
        let state = state_with_group();
        let mut x = valid_xform();
        x["defaultValue"] = json!("not-a-number");
        let err = put_metric_filter(
            &state,
            &json!({
                "logGroupName": "g",
                "filterName": "f",
                "metricTransformations": [x],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert!(err.message.contains("defaultValue"), "{err:?}");
    }

    #[test]
    fn dimensions_must_be_object_of_strings() {
        let state = state_with_group();
        let mut x = valid_xform();
        x["dimensions"] = json!({"k": 42});
        let err = put_metric_filter(
            &state,
            &json!({
                "logGroupName": "g",
                "filterName": "f",
                "metricTransformations": [x],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert!(err.message.contains("string"), "{err:?}");
    }
}
