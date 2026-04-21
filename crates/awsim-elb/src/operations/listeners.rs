use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::resource_not_found,
    ids::{arn_suffix, listener_arn},
    state::{ElbState, Listener, ListenerAction},
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
        "CreateListenerResult": {
            "Listeners": {
                "member": [result]
            }
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

    Ok(json!({ "DeleteListenerResult": {} }))
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
                .map_or(true, |arn| &l.load_balancer_arn == arn);
            let arn_ok =
                listener_arns.is_empty() || listener_arns.contains(&l.arn);
            lb_ok && arn_ok
        })
        .map(|e| listener_to_value(e.value()))
        .collect();

    Ok(json!({
        "DescribeListenersResult": {
            "Listeners": {
                "member": listeners
            },
            "NextMarker": null
        }
    }))
}

pub fn parse_actions(input: &Value, key: &str) -> Vec<ListenerAction> {
    let mut actions = Vec::new();

    if let Some(v) = input.get(key) {
        let items: Vec<&Value> = match v {
            Value::Array(arr) => arr.iter().collect(),
            Value::Object(map) => {
                let members = if let Some(Value::Object(m)) = map.get("member") {
                    m.values().collect()
                } else {
                    let mut pairs: Vec<_> = map.iter().collect();
                    pairs.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(u64::MAX));
                    pairs.into_iter().map(|(_, v)| v).collect()
                };
                members
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
