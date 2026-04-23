use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::CloudFrontState;

pub fn list_distributions_by_web_acl_id(
    state: &CloudFrontState,
    _web_acl_id: &str,
) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .distributions
        .iter()
        .map(|e| {
            let d = e.value();
            json!({
                "Id": d.id,
                "ARN": d.arn,
                "DomainName": d.domain_name,
                "Status": d.status,
            })
        })
        .collect();
    let qty = items.len();
    Ok(json!({
        "DistributionList": {
            "Marker": "",
            "MaxItems": 100,
            "IsTruncated": false,
            "Quantity": qty,
            "Items": { "DistributionSummary": items }
        }
    }))
}

pub fn list_distributions_by_realtime_log_config(
    _state: &CloudFrontState,
    _input: &Value,
) -> Result<Value, AwsError> {
    Ok(json!({
        "DistributionList": {
            "Marker": "",
            "MaxItems": 100,
            "IsTruncated": false,
            "Quantity": 0,
            "Items": { "DistributionSummary": [] }
        }
    }))
}
