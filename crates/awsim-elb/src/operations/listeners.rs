use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::resource_not_found,
    ids::{arn_suffix, listener_arn},
    state::{Certificate, ElbState, Listener, ListenerAction},
};

use super::{extract_string_list, opt_str, require_str};

pub fn listener_to_value(l: &Listener) -> Value {
    let actions: Vec<Value> = l
        .default_actions
        .iter()
        .map(|a| {
            let mut v = json!({ "Type": a.action_type });
            if let Some(ref tg) = a.target_group_arn {
                v["TargetGroupArn"] = json!(tg);
                v["ForwardConfig"] = json!({
                    "TargetGroups": [{ "TargetGroupArn": tg, "Weight": 1 }]
                });
            }
            v
        })
        .collect();

    json!({
        "ListenerArn": l.arn,
        "LoadBalancerArn": l.load_balancer_arn,
        "Port": l.port,
        "Protocol": l.protocol,
        "DefaultActions": { "member": actions },
        "SslPolicy": null,
        "Certificates": [],
    })
}

pub fn create_listener(
    state: &ElbState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let lb_arn = require_str(input, "LoadBalancerArn")?.to_string();

    // Ensure the load balancer exists
    let lb = state
        .load_balancers
        .get(&lb_arn)
        .ok_or_else(|| resource_not_found("load balancer", &lb_arn))?;

    let lb_name = lb.name.clone();
    let lb_rand = arn_suffix(&lb_arn).to_string();
    drop(lb);

    let port: u16 = input
        .get("Port")
        .and_then(|v| match v {
            Value::Number(n) => n.as_u64().map(|n| n as u16),
            Value::String(s) => s.parse().ok(),
            _ => None,
        })
        .unwrap_or(80);

    let protocol = opt_str(input, "Protocol").unwrap_or("HTTP").to_string();

    let default_actions = parse_actions(input, "DefaultActions");

    let arn = listener_arn(&ctx.region, &ctx.account_id, &lb_name, &lb_rand);

    let listener = Listener {
        arn: arn.clone(),
        load_balancer_arn: lb_arn,
        port,
        protocol,
        default_actions,
    };

    let result = listener_to_value(&listener);
    state.listeners.insert(arn, listener);

    Ok(json!({
        "Listeners": {
            "member": [result]
        }
    }))
}

pub fn delete_listener(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "ListenerArn")?;

    if state.listeners.remove(arn).is_none() {
        return Err(resource_not_found("listener", arn));
    }

    // Also remove rules that belong to this listener
    state.rules.retain(|_, v| v.listener_arn != arn);

    Ok(json!({}))
}

pub fn describe_listeners(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let lb_arn_filter = opt_str(input, "LoadBalancerArn").map(|s| s.to_string());
    let listener_arns = extract_string_list(input, "ListenerArns");

    let listeners: Vec<Value> = state
        .listeners
        .iter()
        .filter(|e| {
            let l = e.value();
            let lb_ok = lb_arn_filter
                .as_ref()
                .is_none_or(|arn| &l.load_balancer_arn == arn);
            let arn_ok = listener_arns.is_empty() || listener_arns.contains(&l.arn);
            lb_ok && arn_ok
        })
        .map(|e| listener_to_value(e.value()))
        .collect();

    Ok(json!({
        "Listeners": {
            "member": listeners
        },
        "NextMarker": null
    }))
}

pub fn modify_listener(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "ListenerArn")?;

    let mut listener = state
        .listeners
        .get_mut(arn)
        .ok_or_else(|| resource_not_found("listener", arn))?;

    if let Some(port_val) = input.get("Port")
        && let Some(port) = match port_val {
            Value::Number(n) => n.as_u64().map(|n| n as u16),
            Value::String(s) => s.parse().ok(),
            _ => None,
        }
    {
        listener.port = port;
    }

    if let Some(proto) = input.get("Protocol").and_then(|v| v.as_str()) {
        listener.protocol = proto.to_string();
    }

    let new_actions = parse_actions(input, "DefaultActions");
    if !new_actions.is_empty() {
        listener.default_actions = new_actions;
    }

    let result = listener_to_value(&listener);

    Ok(json!({
        "Listeners": {
            "member": [result]
        }
    }))
}

pub fn describe_listener_certificates(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "ListenerArn")?;

    if !state.listeners.contains_key(arn) {
        return Err(resource_not_found("listener", arn));
    }

    let certs: Vec<Value> = state
        .listener_certificates
        .get(arn)
        .map(|list| {
            list.iter()
                .map(|c| {
                    json!({
                        "CertificateArn": c.certificate_arn,
                        "IsDefault": c.is_default,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(json!({
        "Certificates": { "member": certs },
        "NextMarker": null
    }))
}

fn parse_cert_list(input: &Value) -> Vec<Certificate> {
    let mut result = Vec::new();
    if let Some(certs) = input.get("Certificates") {
        let items: Vec<&Value> = match certs {
            Value::Array(arr) => arr.iter().collect(),
            Value::Object(map) => {
                if let Some(Value::Object(m)) = map.get("member") {
                    m.values().collect()
                } else {
                    let mut pairs: Vec<_> = map.iter().collect();
                    pairs.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(u64::MAX));
                    pairs.into_iter().map(|(_, v)| v).collect()
                }
            }
            _ => vec![],
        };
        for item in items {
            let arn = item
                .get("CertificateArn")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let is_default = item
                .get("IsDefault")
                .and_then(|v| match v {
                    Value::Bool(b) => Some(*b),
                    Value::String(s) => Some(s == "true"),
                    _ => None,
                })
                .unwrap_or(false);
            result.push(Certificate {
                certificate_arn: arn,
                is_default,
            });
        }
    }
    result
}

pub fn add_listener_certificates(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "ListenerArn")?;

    if !state.listeners.contains_key(arn) {
        return Err(resource_not_found("listener", arn));
    }

    let new_certs = parse_cert_list(input);

    let mut existing = state
        .listener_certificates
        .entry(arn.to_string())
        .or_default();

    for cert in &new_certs {
        if !existing
            .iter()
            .any(|c| c.certificate_arn == cert.certificate_arn)
        {
            existing.push(cert.clone());
        }
    }

    let result: Vec<Value> = new_certs
        .iter()
        .map(|c| json!({ "CertificateArn": c.certificate_arn, "IsDefault": c.is_default }))
        .collect();

    Ok(json!({
        "Certificates": { "member": result }
    }))
}

pub fn remove_listener_certificates(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "ListenerArn")?;

    if !state.listeners.contains_key(arn) {
        return Err(resource_not_found("listener", arn));
    }

    let remove_certs = parse_cert_list(input);
    let remove_arns: Vec<String> = remove_certs
        .into_iter()
        .map(|c| c.certificate_arn)
        .collect();

    if let Some(mut existing) = state.listener_certificates.get_mut(arn) {
        existing.retain(|c| !remove_arns.contains(&c.certificate_arn));
    }

    Ok(json!({}))
}

pub fn parse_actions(input: &Value, key: &str) -> Vec<ListenerAction> {
    let mut actions = Vec::new();

    if let Some(v) = input.get(key) {
        let items: Vec<&Value> = match v {
            Value::Array(arr) => arr.iter().collect(),
            Value::Object(map) => {
                if let Some(Value::Object(m)) = map.get("member") {
                    m.values().collect()
                } else {
                    let mut pairs: Vec<_> = map.iter().collect();
                    pairs.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(u64::MAX));
                    pairs.into_iter().map(|(_, v)| v).collect()
                }
            }
            _ => vec![],
        };

        for item in items {
            let action_type = item
                .get("Type")
                .and_then(|v| v.as_str())
                .unwrap_or("forward")
                .to_string();
            let target_group_arn = item
                .get("TargetGroupArn")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            actions.push(ListenerAction {
                action_type,
                target_group_arn,
            });
        }
    }

    actions
}
