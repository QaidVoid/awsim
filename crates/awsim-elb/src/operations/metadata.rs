use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::ElbState;

pub fn describe_account_limits(_state: &ElbState, _input: &Value) -> Result<Value, AwsError> {
    let limits = vec![
        json!({ "Name": "application-load-balancers", "Max": "50" }),
        json!({ "Name": "network-load-balancers", "Max": "50" }),
        json!({ "Name": "target-groups", "Max": "3000" }),
        json!({ "Name": "listeners-per-application-load-balancer", "Max": "50" }),
        json!({ "Name": "listeners-per-network-load-balancer", "Max": "50" }),
        json!({ "Name": "rules-per-application-load-balancer", "Max": "100" }),
    ];

    Ok(json!({
        "DescribeAccountLimitsResult": {
            "Limits": { "member": limits },
            "NextMarker": null
        }
    }))
}

pub fn describe_ssl_policies(_state: &ElbState, _input: &Value) -> Result<Value, AwsError> {
    let policies = vec![
        json!({
            "Name": "ELBSecurityPolicy-2016-08",
            "SslProtocols": { "member": ["TLSv1", "TLSv1.1", "TLSv1.2"] },
            "Ciphers": { "member": [] }
        }),
        json!({
            "Name": "ELBSecurityPolicy-TLS-1-2-2017-01",
            "SslProtocols": { "member": ["TLSv1.2"] },
            "Ciphers": { "member": [] }
        }),
        json!({
            "Name": "ELBSecurityPolicy-TLS13-1-2-2021-06",
            "SslProtocols": { "member": ["TLSv1.2", "TLSv1.3"] },
            "Ciphers": { "member": [] }
        }),
    ];

    Ok(json!({
        "DescribeSSLPoliciesResult": {
            "SslPolicies": { "member": policies },
            "NextMarker": null
        }
    }))
}
