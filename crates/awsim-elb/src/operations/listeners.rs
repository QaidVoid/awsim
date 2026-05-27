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
        .map(listener_action_to_value)
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

    // Protocol must match the load-balancer family.
    //   * `application` ALB  -> HTTP, HTTPS
    //   * `network`     NLB  -> TCP, UDP, TCP_UDP, TLS
    //   * `gateway`     GWLB -> GENEVE (only on port 6081)
    let lb_type = state
        .load_balancers
        .get(input["LoadBalancerArn"].as_str().unwrap_or(""))
        .map(|e| e.lb_type.clone())
        .unwrap_or_else(|| "application".to_string());
    let allowed: &[&str] = match lb_type.as_str() {
        "application" => &["HTTP", "HTTPS"],
        "network" => &["TCP", "UDP", "TCP_UDP", "TLS"],
        "gateway" => &["GENEVE"],
        _ => &["HTTP", "HTTPS"],
    };
    if !allowed.contains(&protocol.as_str()) {
        return Err(awsim_core::AwsError::bad_request(
            "ValidationError",
            format!(
                "Listener protocol `{protocol}` is not valid for load balancer type `{lb_type}`. \
                 Allowed: {}.",
                allowed.join(", "),
            ),
        ));
    }
    if lb_type == "gateway" && port != 6081 {
        return Err(awsim_core::AwsError::bad_request(
            "ValidationError",
            format!("Gateway load balancer listeners must use port 6081 (got {port})."),
        ));
    }

    // AWS requires at least one Certificates[] entry when the listener
    // terminates TLS. Surface the documented CertificateNotFound error
    // up front so SDKs don't get a half-configured listener that fails
    // at TLS handshake time.
    if matches!(protocol.as_str(), "HTTPS" | "TLS") {
        let has_cert = input
            .get("Certificates")
            .and_then(Value::as_array)
            .is_some_and(|arr| {
                arr.iter()
                    .any(|c| c.get("CertificateArn").and_then(Value::as_str).is_some())
            });
        if !has_cert {
            return Err(awsim_core::AwsError::bad_request(
                "CertificateNotFound",
                format!(
                    "Listener protocol `{protocol}` requires at least one Certificates entry with a CertificateArn."
                ),
            ));
        }
    }

    let default_actions = parse_actions(input, "DefaultActions")?;

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

    let new_actions = parse_actions(input, "DefaultActions")?;
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

/// Serialize a single ListenerAction back to the AWS wire shape. Each
/// per-type config field uses the documented key (RedirectConfig,
/// FixedResponseConfig, etc.) so SDKs round-trip cleanly.
pub fn listener_action_to_value(a: &ListenerAction) -> Value {
    let mut v = json!({ "Type": a.action_type });
    if let Some(ref tg) = a.target_group_arn {
        v["TargetGroupArn"] = json!(tg);
    }
    if let Some(ref cfg) = a.config {
        let key = match a.action_type.as_str() {
            "redirect" => "RedirectConfig",
            "fixed-response" => "FixedResponseConfig",
            "authenticate-cognito" => "AuthenticateCognitoConfig",
            "authenticate-oidc" => "AuthenticateOidcConfig",
            _ => "ForwardConfig",
        };
        v[key] = cfg.clone();
    } else if let Some(ref tg) = a.target_group_arn {
        // Preserve the legacy single-target ForwardConfig echo so
        // callers that don't supply ForwardConfig still see one back.
        v["ForwardConfig"] = json!({
            "TargetGroups": [{ "TargetGroupArn": tg, "Weight": 1 }]
        });
    }
    v
}

pub fn parse_actions(input: &Value, key: &str) -> Result<Vec<ListenerAction>, AwsError> {
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
            if !matches!(
                action_type.as_str(),
                "forward"
                    | "redirect"
                    | "fixed-response"
                    | "authenticate-cognito"
                    | "authenticate-oidc"
            ) {
                return Err(awsim_core::AwsError::bad_request(
                    "InvalidConfigurationRequestException",
                    format!(
                        "Action type `{action_type}` is not valid. Allowed: forward, \
                         redirect, fixed-response, authenticate-cognito, authenticate-oidc."
                    ),
                ));
            }
            let target_group_arn = item
                .get("TargetGroupArn")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            // Pull the per-type config block. The action type tells us
            // which key carries the typed payload; AWS requires it to
            // be present (and shaped) for non-forward actions.
            let config = match action_type.as_str() {
                "redirect" => {
                    let cfg = item.get("RedirectConfig").cloned().ok_or_else(|| {
                        awsim_core::AwsError::bad_request(
                            "InvalidConfigurationRequestException",
                            "Action type `redirect` requires a RedirectConfig.",
                        )
                    })?;
                    let status = cfg.get("StatusCode").and_then(Value::as_str).unwrap_or("");
                    if !matches!(status, "HTTP_301" | "HTTP_302") {
                        return Err(awsim_core::AwsError::bad_request(
                            "InvalidConfigurationRequestException",
                            format!(
                                "RedirectConfig.StatusCode `{status}` must be HTTP_301 or HTTP_302."
                            ),
                        ));
                    }
                    Some(cfg)
                }
                "fixed-response" => {
                    let cfg = item.get("FixedResponseConfig").cloned().ok_or_else(|| {
                        awsim_core::AwsError::bad_request(
                            "InvalidConfigurationRequestException",
                            "Action type `fixed-response` requires a FixedResponseConfig.",
                        )
                    })?;
                    let status = cfg.get("StatusCode").and_then(Value::as_str).unwrap_or("");
                    if status.parse::<u16>().is_err() {
                        return Err(awsim_core::AwsError::bad_request(
                            "InvalidConfigurationRequestException",
                            format!(
                                "FixedResponseConfig.StatusCode `{status}` must be a numeric HTTP status."
                            ),
                        ));
                    }
                    Some(cfg)
                }
                "authenticate-cognito" => {
                    let cfg = item
                        .get("AuthenticateCognitoConfig")
                        .cloned()
                        .ok_or_else(|| {
                            awsim_core::AwsError::bad_request(
                                "InvalidConfigurationRequestException",
                                "Action type `authenticate-cognito` requires AuthenticateCognitoConfig.",
                            )
                        })?;
                    for required in ["UserPoolArn", "UserPoolClientId", "UserPoolDomain"] {
                        if cfg
                            .get(required)
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .is_none()
                        {
                            return Err(awsim_core::AwsError::bad_request(
                                "InvalidConfigurationRequestException",
                                format!("AuthenticateCognitoConfig.{required} is required."),
                            ));
                        }
                    }
                    Some(cfg)
                }
                "authenticate-oidc" => {
                    let cfg = item.get("AuthenticateOidcConfig").cloned().ok_or_else(|| {
                        awsim_core::AwsError::bad_request(
                            "InvalidConfigurationRequestException",
                            "Action type `authenticate-oidc` requires AuthenticateOidcConfig.",
                        )
                    })?;
                    for required in [
                        "Issuer",
                        "AuthorizationEndpoint",
                        "TokenEndpoint",
                        "UserInfoEndpoint",
                        "ClientId",
                    ] {
                        if cfg
                            .get(required)
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .is_none()
                        {
                            return Err(awsim_core::AwsError::bad_request(
                                "InvalidConfigurationRequestException",
                                format!("AuthenticateOidcConfig.{required} is required."),
                            ));
                        }
                    }
                    Some(cfg)
                }
                _ => {
                    let cfg = item.get("ForwardConfig").cloned();
                    if let Some(ref c) = cfg {
                        validate_forward_target_groups(c)?;
                    }
                    cfg
                }
            };
            actions.push(ListenerAction {
                action_type,
                target_group_arn,
                config,
            });
        }
    }

    Ok(actions)
}

/// Validate the `TargetGroups` array of a `forward` action's
/// `ForwardConfig`. AWS rejects:
/// - Negative weights (each entry must be `0..=999`).
/// - An all-zero or empty target-group list: a weight sum of 0 cannot
///   distribute any traffic.
fn validate_forward_target_groups(cfg: &Value) -> Result<(), awsim_core::AwsError> {
    let Some(items) = collect_target_groups(cfg) else {
        return Ok(());
    };
    if items.is_empty() {
        return Err(awsim_core::AwsError::bad_request(
            "InvalidConfigurationRequestException",
            "ForwardConfig.TargetGroups must contain at least one target group.",
        ));
    }
    let mut total: u64 = 0;
    for item in &items {
        let weight = parse_weight(item)?;
        total = total.saturating_add(u64::from(weight));
    }
    if total == 0 {
        return Err(awsim_core::AwsError::bad_request(
            "InvalidConfigurationRequestException",
            "ForwardConfig.TargetGroups must include at least one target group with a non-zero weight.",
        ));
    }
    Ok(())
}

fn collect_target_groups(cfg: &Value) -> Option<Vec<Value>> {
    let tg = cfg.get("TargetGroups")?;
    let items: Vec<Value> = match tg {
        Value::Array(arr) => arr.clone(),
        Value::Object(map) => {
            let inner = if let Some(Value::Object(m)) = map.get("member") {
                m
            } else {
                map
            };
            let mut pairs: Vec<_> = inner.iter().collect();
            pairs.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(u64::MAX));
            pairs.into_iter().map(|(_, v)| v.clone()).collect()
        }
        _ => return None,
    };
    Some(items)
}

fn parse_weight(item: &Value) -> Result<u32, awsim_core::AwsError> {
    let weight = match item.get("Weight") {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.parse::<i64>().ok(),
        None => return Ok(1),
        _ => None,
    };
    let Some(w) = weight else {
        return Err(awsim_core::AwsError::bad_request(
            "InvalidConfigurationRequestException",
            "TargetGroupTuple.Weight must be an integer.",
        ));
    };
    if !(0..=999).contains(&w) {
        return Err(awsim_core::AwsError::bad_request(
            "InvalidConfigurationRequestException",
            format!("TargetGroupTuple.Weight `{w}` must be between 0 and 999."),
        ));
    }
    Ok(w as u32)
}

/// Pick a target group ARN from a forward action's `ForwardConfig`
/// using a per-call counter. AWS Application Load Balancer distributes
/// traffic across the listed target groups in proportion to their
/// weights; we model that as a deterministic counter modulo the total
/// weight sum so callers (tests, simulated proxies) get repeatable
/// behavior. Returns `None` when the action isn't a weighted forward
/// or all weights are zero.
///
/// Wired into the simulator's proxy path once cross-service ALB
/// dispatch is implemented; until then it's exercised by the unit
/// tests below.
#[allow(dead_code)]
pub fn pick_weighted_target_group(forward_config: &Value, counter: u64) -> Option<String> {
    let items = collect_target_groups(forward_config)?;
    let weights: Vec<(String, u32)> = items
        .iter()
        .filter_map(|item| {
            let arn = item.get("TargetGroupArn").and_then(Value::as_str)?;
            let weight = parse_weight(item).ok()?;
            Some((arn.to_string(), weight))
        })
        .collect();
    let total: u64 = weights.iter().map(|(_, w)| u64::from(*w)).sum();
    if total == 0 {
        return None;
    }
    let mut tick = counter % total;
    for (arn, w) in weights {
        if tick < u64::from(w) {
            return Some(arn);
        }
        tick -= u64::from(w);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::LoadBalancer;
    use serde_json::json;
    use std::collections::HashMap;

    fn ctx() -> RequestContext {
        RequestContext::new("elasticloadbalancing", "us-east-1")
    }

    fn state_with_lb() -> (ElbState, String) {
        let state = ElbState::default();
        let arn = "arn:aws:elasticloadbalancing:us-east-1:000000000000:loadbalancer/app/web/abc"
            .to_string();
        state.load_balancers.insert(
            arn.clone(),
            LoadBalancer {
                arn: arn.clone(),
                name: "web".to_string(),
                dns_name: "web.elb".to_string(),
                lb_type: "application".to_string(),
                scheme: "internet-facing".to_string(),
                state: "active".to_string(),
                subnets: vec![],
                security_groups: vec![],
                tags: HashMap::new(),
                created_at: "now".to_string(),
                vpc_id: "vpc-test".to_string(),
            },
        );
        (state, arn)
    }

    #[test]
    fn create_listener_rejects_https_without_certificate() {
        let (state, lb_arn) = state_with_lb();
        let err = create_listener(
            &state,
            &json!({
                "LoadBalancerArn": lb_arn,
                "Protocol": "HTTPS",
                "Port": 443,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "CertificateNotFound");
    }

    #[test]
    fn create_listener_accepts_https_with_certificate() {
        let (state, lb_arn) = state_with_lb();
        create_listener(
            &state,
            &json!({
                "LoadBalancerArn": lb_arn,
                "Protocol": "HTTPS",
                "Port": 443,
                "Certificates": [{
                    "CertificateArn": "arn:aws:acm:us-east-1:000000000000:certificate/x"
                }]
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn create_listener_http_does_not_require_certificate() {
        let (state, lb_arn) = state_with_lb();
        create_listener(
            &state,
            &json!({
                "LoadBalancerArn": lb_arn,
                "Protocol": "HTTP",
                "Port": 80,
            }),
            &ctx(),
        )
        .unwrap();
    }

    fn state_with_lb_of_type(lb_type: &str) -> (ElbState, String) {
        let state = ElbState::default();
        let arn = format!(
            "arn:aws:elasticloadbalancing:us-east-1:000000000000:loadbalancer/{}/lb/abc",
            match lb_type {
                "network" => "net",
                "gateway" => "gwy",
                _ => "app",
            }
        );
        state.load_balancers.insert(
            arn.clone(),
            LoadBalancer {
                arn: arn.clone(),
                name: "lb".to_string(),
                dns_name: "lb.elb".to_string(),
                lb_type: lb_type.to_string(),
                scheme: "internet-facing".to_string(),
                state: "active".to_string(),
                subnets: vec![],
                security_groups: vec![],
                tags: HashMap::new(),
                created_at: "now".to_string(),
                vpc_id: "vpc-test".to_string(),
            },
        );
        (state, arn)
    }

    #[test]
    fn alb_rejects_network_protocol() {
        let (state, lb) = state_with_lb_of_type("application");
        let err = create_listener(
            &state,
            &json!({ "LoadBalancerArn": lb, "Protocol": "TCP", "Port": 80 }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationError");
    }

    #[test]
    fn nlb_accepts_tcp_udp_protocol() {
        let (state, lb) = state_with_lb_of_type("network");
        create_listener(
            &state,
            &json!({ "LoadBalancerArn": lb, "Protocol": "TCP_UDP", "Port": 80 }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn nlb_rejects_http_protocol() {
        let (state, lb) = state_with_lb_of_type("network");
        let err = create_listener(
            &state,
            &json!({ "LoadBalancerArn": lb, "Protocol": "HTTP", "Port": 80 }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationError");
    }

    #[test]
    fn gwlb_requires_geneve_on_6081() {
        let (state, lb) = state_with_lb_of_type("gateway");
        create_listener(
            &state,
            &json!({ "LoadBalancerArn": lb, "Protocol": "GENEVE", "Port": 6081 }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn gwlb_rejects_wrong_port() {
        let (state, lb) = state_with_lb_of_type("gateway");
        let err = create_listener(
            &state,
            &json!({ "LoadBalancerArn": lb, "Protocol": "GENEVE", "Port": 80 }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationError");
        assert!(err.message.contains("6081"));
    }

    #[test]
    fn parses_redirect_action() {
        let input = json!({
            "DefaultActions": [{
                "Type": "redirect",
                "RedirectConfig": {
                    "Protocol": "HTTPS",
                    "Port": "443",
                    "StatusCode": "HTTP_301"
                }
            }]
        });
        let actions = parse_actions(&input, "DefaultActions").unwrap();
        assert_eq!(actions[0].action_type, "redirect");
        assert_eq!(
            actions[0].config.as_ref().unwrap()["StatusCode"],
            "HTTP_301"
        );
    }

    #[test]
    fn rejects_redirect_bad_status_code() {
        let input = json!({
            "DefaultActions": [{
                "Type": "redirect",
                "RedirectConfig": { "StatusCode": "HTTP_307" }
            }]
        });
        let err = parse_actions(&input, "DefaultActions").unwrap_err();
        assert_eq!(err.code, "InvalidConfigurationRequestException");
    }

    #[test]
    fn parses_fixed_response_action() {
        let input = json!({
            "DefaultActions": [{
                "Type": "fixed-response",
                "FixedResponseConfig": {
                    "ContentType": "text/plain",
                    "MessageBody": "Hello",
                    "StatusCode": "200"
                }
            }]
        });
        let actions = parse_actions(&input, "DefaultActions").unwrap();
        assert_eq!(actions[0].action_type, "fixed-response");
    }

    #[test]
    fn rejects_authenticate_cognito_missing_pool_arn() {
        let input = json!({
            "DefaultActions": [{
                "Type": "authenticate-cognito",
                "AuthenticateCognitoConfig": {
                    "UserPoolClientId": "abc",
                    "UserPoolDomain": "d"
                }
            }]
        });
        let err = parse_actions(&input, "DefaultActions").unwrap_err();
        assert_eq!(err.code, "InvalidConfigurationRequestException");
        assert!(err.message.contains("UserPoolArn"));
    }

    #[test]
    fn rejects_unknown_action_type() {
        let input = json!({ "DefaultActions": [{ "Type": "send-postcard" }] });
        let err = parse_actions(&input, "DefaultActions").unwrap_err();
        assert_eq!(err.code, "InvalidConfigurationRequestException");
    }

    #[test]
    fn forward_action_rejects_all_zero_weights() {
        let input = json!({
            "DefaultActions": [{
                "Type": "forward",
                "ForwardConfig": {
                    "TargetGroups": [
                        { "TargetGroupArn": "arn:aws:elasticloadbalancing:us-east-1:000000000000:targetgroup/a/1", "Weight": 0 },
                        { "TargetGroupArn": "arn:aws:elasticloadbalancing:us-east-1:000000000000:targetgroup/b/2", "Weight": 0 }
                    ]
                }
            }]
        });
        let err = parse_actions(&input, "DefaultActions").unwrap_err();
        assert_eq!(err.code, "InvalidConfigurationRequestException");
        assert!(err.message.contains("non-zero"));
    }

    #[test]
    fn forward_action_rejects_weight_out_of_range() {
        let input = json!({
            "DefaultActions": [{
                "Type": "forward",
                "ForwardConfig": {
                    "TargetGroups": [
                        { "TargetGroupArn": "arn:aws:elasticloadbalancing:us-east-1:000000000000:targetgroup/a/1", "Weight": 1000 }
                    ]
                }
            }]
        });
        let err = parse_actions(&input, "DefaultActions").unwrap_err();
        assert_eq!(err.code, "InvalidConfigurationRequestException");
    }

    #[test]
    fn forward_action_accepts_mixed_weights() {
        let input = json!({
            "DefaultActions": [{
                "Type": "forward",
                "ForwardConfig": {
                    "TargetGroups": [
                        { "TargetGroupArn": "arn:aws:elasticloadbalancing:us-east-1:000000000000:targetgroup/a/1", "Weight": 1 },
                        { "TargetGroupArn": "arn:aws:elasticloadbalancing:us-east-1:000000000000:targetgroup/b/2", "Weight": 0 }
                    ]
                }
            }]
        });
        let actions = parse_actions(&input, "DefaultActions").unwrap();
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn pick_weighted_target_group_distributes_per_weight() {
        let cfg = json!({
            "TargetGroups": [
                { "TargetGroupArn": "tg-a", "Weight": 1 },
                { "TargetGroupArn": "tg-b", "Weight": 3 }
            ]
        });
        let total = 4_u64;
        let mut counts = std::collections::HashMap::new();
        for i in 0..(4 * total) {
            let pick = pick_weighted_target_group(&cfg, i).unwrap();
            *counts.entry(pick).or_insert(0u64) += 1;
        }
        // 4:1 split over 16 picks → tg-a=4, tg-b=12
        assert_eq!(counts["tg-a"], 4);
        assert_eq!(counts["tg-b"], 12);
    }

    #[test]
    fn pick_weighted_target_group_returns_none_when_total_zero() {
        let cfg = json!({
            "TargetGroups": [
                { "TargetGroupArn": "tg-a", "Weight": 0 },
                { "TargetGroupArn": "tg-b", "Weight": 0 }
            ]
        });
        assert!(pick_weighted_target_group(&cfg, 0).is_none());
    }
}
