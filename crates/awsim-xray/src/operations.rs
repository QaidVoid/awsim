use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{Trace, TraceSegment, XrayState};

/// X-Ray segment "document" field is a JSON-encoded string. This deserializes
/// it once so we can extract trace_id, start/end times, errors, etc.
fn parse_segment_doc(raw: &str) -> Option<Value> {
    serde_json::from_str(raw).ok()
}

pub fn put_trace_segments(
    state: &XrayState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let docs = input
        .get("TraceSegmentDocuments")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidRequestException",
                "TraceSegmentDocuments is required",
            )
        })?;

    let mut unprocessed: Vec<Value> = Vec::new();
    for doc_value in docs {
        let raw = match doc_value.as_str() {
            Some(s) => s,
            None => {
                unprocessed.push(json!({
                    "ErrorCode": "InvalidSegment",
                    "Message": "Segment document must be a JSON-encoded string"
                }));
                continue;
            }
        };
        let parsed = match parse_segment_doc(raw) {
            Some(p) => p,
            None => {
                unprocessed.push(json!({
                    "ErrorCode": "InvalidSegment",
                    "Message": "Segment document is not valid JSON"
                }));
                continue;
            }
        };
        let trace_id = match parsed.get("trace_id").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => {
                unprocessed.push(json!({
                    "ErrorCode": "MissingField",
                    "Message": "Segment is missing trace_id"
                }));
                continue;
            }
        };
        let segment_id = parsed
            .get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_default();
        let start = parsed
            .get("start_time")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let end = parsed
            .get("end_time")
            .and_then(|v| v.as_f64())
            .unwrap_or(start);
        let service_name = parsed
            .get("name")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_default();
        let has_error = parsed
            .get("error")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let has_fault = parsed
            .get("fault")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let has_throttle = parsed
            .get("throttle")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Append segment to the trace, creating it if needed and merging metadata.
        state
            .traces
            .entry(trace_id.clone())
            .and_modify(|t| {
                t.segments.push(TraceSegment {
                    id: segment_id.clone(),
                    document: parsed.clone(),
                });
                if start > 0.0 && (t.start_time == 0.0 || start < t.start_time) {
                    t.start_time = start;
                }
                if end > t.end_time {
                    t.end_time = end;
                }
                t.duration = (t.end_time - t.start_time).max(0.0);
                if !service_name.is_empty() && !t.services.contains(&service_name) {
                    t.services.push(service_name.clone());
                }
                t.has_error |= has_error;
                t.has_fault |= has_fault;
                t.has_throttle |= has_throttle;
            })
            .or_insert_with(|| Trace {
                trace_id: trace_id.clone(),
                segments: vec![TraceSegment {
                    id: segment_id,
                    document: parsed.clone(),
                }],
                start_time: start,
                end_time: end,
                duration: (end - start).max(0.0),
                services: if service_name.is_empty() {
                    vec![]
                } else {
                    vec![service_name]
                },
                has_error,
                has_fault,
                has_throttle,
            });
    }

    Ok(json!({ "UnprocessedTraceSegments": unprocessed }))
}

pub fn batch_get_traces(
    state: &XrayState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ids = input
        .get("TraceIds")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "TraceIds is required"))?;

    let mut found: Vec<Value> = Vec::new();
    let mut unprocessed: Vec<Value> = Vec::new();
    for id in ids {
        let id_str = match id.as_str() {
            Some(s) => s,
            None => continue,
        };
        match state.traces.get(id_str) {
            Some(t) => {
                let segments: Vec<Value> = t
                    .segments
                    .iter()
                    .map(|s| {
                        json!({
                            "Id": s.id,
                            "Document": serde_json::to_string(&s.document).unwrap_or_default(),
                        })
                    })
                    .collect();
                found.push(json!({
                    "Id": t.trace_id,
                    "Duration": t.duration,
                    "Segments": segments,
                    "LimitExceeded": false,
                }));
            }
            None => unprocessed.push(Value::String(id_str.to_string())),
        }
    }
    Ok(json!({
        "Traces": found,
        "UnprocessedTraceIds": unprocessed,
    }))
}

pub fn get_trace_summaries(
    state: &XrayState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let start = input
        .get("StartTime")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let end = input
        .get("EndTime")
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::MAX);

    let summaries: Vec<Value> = state
        .traces
        .iter()
        .filter(|e| e.value().start_time >= start && e.value().start_time <= end)
        .map(|e| {
            let t = e.value();
            json!({
                "Id": t.trace_id,
                "Duration": t.duration,
                "ResponseTime": t.duration,
                "HasFault": t.has_fault,
                "HasError": t.has_error,
                "HasThrottle": t.has_throttle,
                "IsPartial": false,
                "ServiceIds": t.services.iter().map(|n| json!({ "Name": n, "Names": [n], "Type": "AWS::Service" })).collect::<Vec<_>>(),
            })
        })
        .collect();

    Ok(json!({
        "TraceSummaries": summaries,
        "ApproximateTime": end,
        "TracesProcessedCount": summaries.len() as i64,
    }))
}

pub fn get_service_graph(
    state: &XrayState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    use std::collections::BTreeMap;
    let mut services: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new();
    for entry in state.traces.iter() {
        let t = entry.value();
        for name in &t.services {
            let counts = services.entry(name.clone()).or_default();
            counts.0 += 1;
            if t.has_error {
                counts.1 += 1;
            }
            if t.has_fault {
                counts.2 += 1;
            }
        }
    }
    let svc_list: Vec<Value> = services
        .into_iter()
        .enumerate()
        .map(|(i, (name, (ok, err, fault)))| {
            json!({
                "ReferenceId": i,
                "Name": name,
                "Names": [name],
                "Type": "AWS::Service",
                "State": "active",
                "SummaryStatistics": {
                    "OkCount": ok,
                    "ErrorStatistics": { "TotalCount": err, "ThrottleCount": 0, "OtherCount": err },
                    "FaultStatistics": { "TotalCount": fault, "OtherCount": fault },
                    "TotalCount": ok,
                    "TotalResponseTime": 0.0,
                },
                "Edges": [],
            })
        })
        .collect();
    Ok(json!({ "Services": svc_list, "ContainsOldGroupVersions": false }))
}

pub fn get_sampling_rules(
    state: &XrayState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let rules: Vec<Value> = state
        .sampling_rules
        .iter()
        .map(|e| {
            json!({
                "SamplingRuleRecord": {
                    "SamplingRule": e.value(),
                    "CreatedAt": 0,
                    "ModifiedAt": 0,
                }
            })
        })
        .collect();
    Ok(json!({ "SamplingRuleRecords": rules }))
}

pub fn create_sampling_rule(
    state: &XrayState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let rule = input.get("SamplingRule").cloned().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "SamplingRule is required")
    })?;
    let name = rule
        .get("RuleName")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "RuleName is required"))?
        .to_string();
    state.sampling_rules.insert(name, rule.clone());
    Ok(json!({
        "SamplingRuleRecord": {
            "SamplingRule": rule,
            "CreatedAt": 0,
            "ModifiedAt": 0,
        }
    }))
}

pub fn delete_sampling_rule(
    state: &XrayState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input
        .get("RuleName")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "RuleName is required"))?;
    let (_, rule) = state.sampling_rules.remove(name).ok_or_else(|| {
        AwsError::not_found("RuleNotFoundException", format!("Rule {name} not found"))
    })?;
    Ok(json!({
        "SamplingRuleRecord": {
            "SamplingRule": rule,
            "CreatedAt": 0,
            "ModifiedAt": 0,
        }
    }))
}

pub fn get_sampling_targets(
    _state: &XrayState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // The emulator doesn't track per-second sampling stats. Hand back the
    // documents as-is so the SDK keeps using its local reservoir.
    let docs = input
        .get("SamplingStatisticsDocuments")
        .cloned()
        .unwrap_or_else(|| Value::Array(vec![]));
    let names = docs
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|d| d.get("RuleName").cloned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let targets: Vec<Value> = names
        .into_iter()
        .map(|n| {
            json!({
                "RuleName": n,
                "FixedRate": 0.05,
                "ReservoirQuota": 1,
                "Interval": 10,
            })
        })
        .collect();
    Ok(json!({
        "SamplingTargetDocuments": targets,
        "LastRuleModification": 0,
        "UnprocessedStatistics": [],
    }))
}

pub fn create_group(
    state: &XrayState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input
        .get("GroupName")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "GroupName is required"))?
        .to_string();
    let arn = format!(
        "arn:aws:xray:{}:{}:group/{}",
        ctx.region, ctx.account_id, name
    );
    let group = json!({
        "GroupName": name,
        "GroupARN": arn,
        "FilterExpression": input.get("FilterExpression").cloned().unwrap_or(Value::Null),
        "InsightsConfiguration": input.get("InsightsConfiguration").cloned().unwrap_or(Value::Null),
    });
    state.groups.insert(name, group.clone());
    Ok(json!({ "Group": group }))
}

pub fn delete_group(
    state: &XrayState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input
        .get("GroupName")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "GroupName is required"))?;
    state.groups.remove(name).ok_or_else(|| {
        AwsError::not_found("GroupNotFoundException", format!("Group {name} not found"))
    })?;
    Ok(json!({}))
}

pub fn get_groups(
    state: &XrayState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let groups: Vec<Value> = state.groups.iter().map(|e| e.value().clone()).collect();
    Ok(json!({ "Groups": groups }))
}
