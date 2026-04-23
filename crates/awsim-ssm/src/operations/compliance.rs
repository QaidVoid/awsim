use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{SsmComplianceItem, SsmState};

pub fn put_compliance_items(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_id = input["ResourceId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceId is required"))?
        .to_string();
    let resource_type = input["ResourceType"]
        .as_str()
        .unwrap_or("ManagedInstance")
        .to_string();
    let compliance_type = input["ComplianceType"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ComplianceType is required"))?
        .to_string();
    let execution_summary = input["ExecutionSummary"].clone();
    let items = input["Items"].as_array().cloned().unwrap_or_default();

    let mut new_items = Vec::new();
    for it in items {
        new_items.push(SsmComplianceItem {
            compliance_type: compliance_type.clone(),
            resource_type: resource_type.clone(),
            resource_id: resource_id.clone(),
            id: it["Id"].as_str().unwrap_or("").to_string(),
            title: it["Title"].as_str().unwrap_or("").to_string(),
            status: it["Status"].as_str().unwrap_or("COMPLIANT").to_string(),
            severity: it["Severity"].as_str().unwrap_or("UNSPECIFIED").to_string(),
            execution_summary: execution_summary.clone(),
            details: it["Details"].clone(),
        });
    }

    let key = format!("{resource_type}:{resource_id}:{compliance_type}");
    state.compliance_items.insert(key, new_items);

    Ok(json!({}))
}

pub fn list_compliance_items(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_ids = input["ResourceIds"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut out = Vec::new();
    for entry in state.compliance_items.iter() {
        for ci in entry.value() {
            if !resource_ids.is_empty() && !resource_ids.contains(&ci.resource_id) {
                continue;
            }
            out.push(json!({
                "ComplianceType": ci.compliance_type,
                "ResourceType": ci.resource_type,
                "ResourceId": ci.resource_id,
                "Id": ci.id,
                "Title": ci.title,
                "Status": ci.status,
                "Severity": ci.severity,
                "ExecutionSummary": ci.execution_summary,
                "Details": ci.details,
            }));
        }
    }

    Ok(json!({ "ComplianceItems": out }))
}

pub fn list_resource_compliance_summaries(
    state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let mut out = Vec::new();
    for entry in state.compliance_items.iter() {
        if let Some(first) = entry.value().first() {
            out.push(json!({
                "ResourceId": first.resource_id,
                "ResourceType": first.resource_type,
                "ComplianceType": first.compliance_type,
                "Status": "COMPLIANT",
                "OverallSeverity": "UNSPECIFIED",
                "CompliantSummary": { "CompliantCount": entry.value().len() },
                "NonCompliantSummary": { "NonCompliantCount": 0 },
            }));
        }
    }

    Ok(json!({ "ResourceComplianceSummaryItems": out }))
}
