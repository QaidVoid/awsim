use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{QueryLoggingConfig, Route53State};

// ---------------------------------------------------------------------------
// GetHostedZoneCount
// ---------------------------------------------------------------------------

/// GET /2013-04-01/hostedzonecount
pub fn get_hosted_zone_count(
    state: &Arc<Route53State>,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({
        "__xml_root": "GetHostedZoneCountResponse",
        "HostedZoneCount": state.hosted_zones.len()
    }))
}

// ---------------------------------------------------------------------------
// TestDNSAnswer
// ---------------------------------------------------------------------------

/// GET /2013-04-01/testdnsanswer
pub fn test_dns_answer(
    _state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let record_name = input
        .get("RecordName")
        .and_then(Value::as_str)
        .unwrap_or("example.com.");
    let record_type = input
        .get("RecordType")
        .and_then(Value::as_str)
        .unwrap_or("A");

    Ok(json!({
        "Nameserver": "ns-1.awsim.invalid",
        "RecordName": record_name,
        "RecordType": record_type,
        "RecordData": {
            "RecordDataEntry": ["192.0.2.1"]
        },
        "ResponseCode": "NOERROR",
        "Protocol": "UDP",
    }))
}

// ---------------------------------------------------------------------------
// GetCheckerIpRanges
// ---------------------------------------------------------------------------

/// GET /2013-04-01/checkeripranges
pub fn get_checker_ip_ranges(
    _state: &Arc<Route53State>,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({
        "CheckerIpRanges": {
            "member": [
                "192.0.2.0/24",
                "198.51.100.0/24",
                "203.0.113.0/24",
            ]
        }
    }))
}

// ---------------------------------------------------------------------------
// ListHostedZonesByVPC
// ---------------------------------------------------------------------------

/// GET /2013-04-01/hostedzonesbyvpc
pub fn list_hosted_zones_by_vpc(
    _state: &Arc<Route53State>,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({
        "HostedZoneSummaries": {
            "HostedZoneSummary": []
        },
        "MaxItems": "100",
        "NextToken": null,
    }))
}

// ---------------------------------------------------------------------------
// GetDNSSEC
// ---------------------------------------------------------------------------

/// GET /2013-04-01/hostedzone/{Id}/dnssec
pub fn get_dnssec(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id_raw = input
        .get("HostedZoneId")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "HostedZoneId is required"))?;

    let id = if id_raw.starts_with("/hostedzone/") {
        id_raw.to_string()
    } else {
        format!("/hostedzone/{id_raw}")
    };

    if !state.hosted_zones.contains_key(&id) {
        return Err(AwsError::not_found(
            "NoSuchHostedZone",
            format!("No hosted zone found with ID: {id}"),
        ));
    }

    Ok(json!({
        "Status": {
            "ServeSignature": "NOT_SIGNING",
            "StatusMessage": "DNSSEC is disabled for this hosted zone",
        },
        "KeySigningKeys": {
            "member": []
        }
    }))
}

// ---------------------------------------------------------------------------
// Query Logging Configs
// ---------------------------------------------------------------------------

/// POST /2013-04-01/queryloggingconfig
pub fn create_query_logging_config(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let hosted_zone_id = input
        .get("HostedZoneId")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "HostedZoneId is required"))?;

    let log_group_arn = input
        .get("CloudWatchLogsLogGroupArn")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AwsError::bad_request("InvalidInput", "CloudWatchLogsLogGroupArn is required")
        })?;

    // Normalize the hosted zone id
    let zone_id = if hosted_zone_id.starts_with("/hostedzone/") {
        hosted_zone_id.to_string()
    } else {
        format!("/hostedzone/{hosted_zone_id}")
    };

    if !state.hosted_zones.contains_key(&zone_id) {
        return Err(AwsError::not_found(
            "NoSuchHostedZone",
            format!("No hosted zone found with ID: {zone_id}"),
        ));
    }

    let id = Uuid::new_v4().to_string();
    let config = QueryLoggingConfig {
        id: id.clone(),
        hosted_zone_id: zone_id.clone(),
        cloud_watch_logs_log_group_arn: log_group_arn.to_string(),
    };

    state.query_logging_configs.insert(id.clone(), config);

    Ok(json!({
        "QueryLoggingConfig": {
            "Id": id,
            "HostedZoneId": zone_id,
            "CloudWatchLogsLogGroupArn": log_group_arn,
        }
    }))
}

/// DELETE /2013-04-01/queryloggingconfig/{Id}
pub fn delete_query_logging_config(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("Id")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "Id is required"))?;

    if state.query_logging_configs.remove(id).is_none() {
        return Err(AwsError::not_found(
            "NoSuchQueryLoggingConfig",
            format!("No query logging config found with ID: {id}"),
        ));
    }

    Ok(json!({}))
}

/// GET /2013-04-01/queryloggingconfig
pub fn list_query_logging_configs(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let zone_filter = input.get("HostedZoneId").and_then(Value::as_str).map(|s| {
        if s.starts_with("/hostedzone/") {
            s.to_string()
        } else {
            format!("/hostedzone/{s}")
        }
    });

    let configs: Vec<Value> = state
        .query_logging_configs
        .iter()
        .filter(|e| {
            zone_filter
                .as_deref()
                .map(|z| e.value().hosted_zone_id == z)
                .unwrap_or(true)
        })
        .map(|e| {
            let c = e.value();
            json!({
                "Id": c.id,
                "HostedZoneId": c.hosted_zone_id,
                "CloudWatchLogsLogGroupArn": c.cloud_watch_logs_log_group_arn,
            })
        })
        .collect();

    Ok(json!({
        "QueryLoggingConfigs": {
            "QueryLoggingConfig": configs
        },
        "IsTruncated": false,
    }))
}
