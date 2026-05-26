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
        "Limits": { "member": limits },
        "NextMarker": null
    }))
}

/// ELB Classic `DescribeLoadBalancerPolicies`. With no LoadBalancerName
/// and an empty PolicyNames filter, AWS returns a fixed sample subset of
/// predefined policies (not the full catalog) — mirror that. When
/// PolicyNames is supplied, return matching entries from the catalog or
/// `PolicyNotFoundException` when any name is unknown.
pub fn describe_load_balancer_policies(
    _state: &ElbState,
    input: &Value,
) -> Result<Value, AwsError> {
    let catalog = predefined_classic_policies();
    let names = input
        .get("PolicyNames")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let selected: Vec<Value> = if names.is_empty() {
        // Sample subset returned by AWS when called without names.
        catalog
            .iter()
            .filter(|p| {
                let name = p["PolicyName"].as_str().unwrap_or("");
                matches!(
                    name,
                    "ELBSample-OpenSSLDefaultCipher"
                        | "ELBSample-OpenSSLDefaultNegotiationPolicyType"
                        | "ELBSecurityPolicy-2016-08"
                        | "ELBSecurityPolicy-TLS-1-1-2017-01"
                        | "ELBSecurityPolicy-TLS-1-2-2017-01"
                )
            })
            .cloned()
            .collect()
    } else {
        let mut out = Vec::with_capacity(names.len());
        for n in &names {
            match catalog.iter().find(|p| p["PolicyName"].as_str() == Some(n)) {
                Some(p) => out.push(p.clone()),
                None => {
                    return Err(AwsError::bad_request(
                        "PolicyNotFound",
                        format!("Policy `{n}` is not in the predefined catalog."),
                    ));
                }
            }
        }
        out
    };

    Ok(json!({
        "PolicyDescriptions": { "member": selected },
    }))
}

fn predefined_classic_policies() -> Vec<Value> {
    vec![
        json!({
            "PolicyName": "ELBSample-OpenSSLDefaultCipher",
            "PolicyTypeName": "SSLNegotiationPolicyType",
            "PolicyAttributeDescriptions": { "member": [] },
        }),
        json!({
            "PolicyName": "ELBSample-OpenSSLDefaultNegotiationPolicyType",
            "PolicyTypeName": "SSLNegotiationPolicyType",
            "PolicyAttributeDescriptions": { "member": [] },
        }),
        json!({
            "PolicyName": "ELBSecurityPolicy-2016-08",
            "PolicyTypeName": "SSLNegotiationPolicyType",
            "PolicyAttributeDescriptions": { "member": [
                { "AttributeName": "Protocol-TLSv1.2", "AttributeValue": "true" },
                { "AttributeName": "Protocol-TLSv1.1", "AttributeValue": "true" },
                { "AttributeName": "Protocol-TLSv1",   "AttributeValue": "true" },
            ]},
        }),
        json!({
            "PolicyName": "ELBSecurityPolicy-TLS-1-1-2017-01",
            "PolicyTypeName": "SSLNegotiationPolicyType",
            "PolicyAttributeDescriptions": { "member": [
                { "AttributeName": "Protocol-TLSv1.2", "AttributeValue": "true" },
                { "AttributeName": "Protocol-TLSv1.1", "AttributeValue": "true" },
            ]},
        }),
        json!({
            "PolicyName": "ELBSecurityPolicy-TLS-1-2-2017-01",
            "PolicyTypeName": "SSLNegotiationPolicyType",
            "PolicyAttributeDescriptions": { "member": [
                { "AttributeName": "Protocol-TLSv1.2", "AttributeValue": "true" },
            ]},
        }),
    ]
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
        "SslPolicies": { "member": policies },
        "NextMarker": null
    }))
}

#[cfg(test)]
mod describe_load_balancer_policies_tests {
    use super::*;

    fn names(resp: &Value) -> Vec<String> {
        resp["PolicyDescriptions"]["member"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["PolicyName"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn no_filter_returns_sample_subset() {
        let state = ElbState::default();
        let resp = describe_load_balancer_policies(&state, &json!({})).unwrap();
        let n = names(&resp);
        assert_eq!(n.len(), 5);
        assert!(n.iter().any(|s| s == "ELBSecurityPolicy-2016-08"));
        assert!(n.iter().any(|s| s.starts_with("ELBSample-")));
    }

    #[test]
    fn filter_by_known_name_returns_just_that_policy() {
        let state = ElbState::default();
        let resp = describe_load_balancer_policies(
            &state,
            &json!({ "PolicyNames": ["ELBSecurityPolicy-2016-08"] }),
        )
        .unwrap();
        let n = names(&resp);
        assert_eq!(n, vec!["ELBSecurityPolicy-2016-08"]);
    }

    #[test]
    fn filter_by_unknown_name_returns_policy_not_found() {
        let state = ElbState::default();
        let err =
            describe_load_balancer_policies(&state, &json!({ "PolicyNames": ["BogusPolicy"] }))
                .unwrap_err();
        assert_eq!(err.code, "PolicyNotFound");
    }
}
