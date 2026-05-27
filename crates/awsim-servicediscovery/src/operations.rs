use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{Instance, Namespace, Operation, ServiceDiscoveryState, ServiceEntry};

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn new_id(prefix: char) -> String {
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    format!("{prefix}-{}", &suffix[..16])
}

fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("InvalidInput", format!("{key} is required")))
}

fn instance_key(service_id: &str, instance_id: &str) -> String {
    format!("{service_id}:{instance_id}")
}

fn namespace_arn(ctx: &RequestContext, id: &str) -> String {
    format!(
        "arn:aws:servicediscovery:{}:{}:namespace/{}",
        ctx.region, ctx.account_id, id
    )
}

fn service_arn(ctx: &RequestContext, id: &str) -> String {
    format!(
        "arn:aws:servicediscovery:{}:{}:service/{}",
        ctx.region, ctx.account_id, id
    )
}

fn record_operation(
    state: &ServiceDiscoveryState,
    op_type: &str,
    targets: HashMap<String, String>,
) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let op = Operation {
        id: id.clone(),
        r#type: op_type.to_string(),
        status: "SUCCESS".to_string(),
        error_message: None,
        error_code: None,
        create_date: now(),
        update_date: now(),
        targets,
    };
    state.operations.insert(id.clone(), op);
    id
}

fn ns_to_value(n: &Namespace) -> Value {
    json!({
        "Id": n.id,
        "Arn": n.arn,
        "Name": n.name,
        "Type": n.r#type,
        "Description": n.description,
        "ServiceCount": n.service_count,
        "CreateDate": n.create_date,
        "CreatorRequestId": n.creator_request_id,
        "Properties": n.properties,
    })
}

fn svc_to_value(s: &ServiceEntry) -> Value {
    json!({
        "Id": s.id,
        "Arn": s.arn,
        "Name": s.name,
        "NamespaceId": s.namespace_id,
        "Description": s.description,
        "InstanceCount": s.instance_count,
        "DnsConfig": s.dns_config,
        "HealthCheckConfig": s.health_check_config,
        "HealthCheckCustomConfig": s.health_check_custom_config,
        "CreateDate": s.create_date,
        "CreatorRequestId": s.creator_request_id,
        "Type": s.r#type,
    })
}

fn inst_to_value(i: &Instance) -> Value {
    json!({
        "Id": i.id,
        "CreatorRequestId": i.creator_request_id,
        "Attributes": i.attributes,
    })
}

// ---------- Namespaces ----------

fn create_namespace(
    state: &ServiceDiscoveryState,
    input: &Value,
    ctx: &RequestContext,
    namespace_type: &str,
) -> Result<Value, AwsError> {
    let name = require_str(input, "Name")?.to_string();
    let id = new_id('n');
    let n = Namespace {
        id: id.clone(),
        arn: namespace_arn(ctx, &id),
        name,
        r#type: namespace_type.to_string(),
        description: input
            .get("Description")
            .and_then(|v| v.as_str())
            .map(String::from),
        service_count: 0,
        create_date: now(),
        creator_request_id: input
            .get("CreatorRequestId")
            .and_then(|v| v.as_str())
            .map(String::from),
        properties: input.get("Properties").cloned(),
    };
    state.namespaces.insert(id.clone(), n);
    let mut targets = HashMap::new();
    targets.insert("NAMESPACE".to_string(), id);
    let op_id = record_operation(state, "CREATE_NAMESPACE", targets);
    Ok(json!({ "OperationId": op_id }))
}

pub fn create_http_namespace(
    state: &ServiceDiscoveryState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    create_namespace(state, input, ctx, "HTTP")
}

pub fn create_private_dns_namespace(
    state: &ServiceDiscoveryState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // Private DNS namespaces must carry a real `Vpc` id. AWS expects a
    // string of the form `vpc-XXXXXXXX` (8-17 hex chars after the
    // prefix); anything else fails validation up-front.
    let vpc = input
        .get("Vpc")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "Vpc is required"))?;
    let body = vpc
        .strip_prefix("vpc-")
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "Vpc must start with 'vpc-'."))?;
    if !(8..=17).contains(&body.len()) || !body.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AwsError::bad_request(
            "InvalidInput",
            format!("Vpc '{vpc}' is not a valid VPC identifier."),
        ));
    }
    create_namespace(state, input, ctx, "DNS_PRIVATE")
}

pub fn create_public_dns_namespace(
    state: &ServiceDiscoveryState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    create_namespace(state, input, ctx, "DNS_PUBLIC")
}

pub fn delete_namespace(
    state: &ServiceDiscoveryState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = require_str(input, "Id")?.to_string();
    if state.services.iter().any(|e| e.value().namespace_id == id) {
        return Err(AwsError::bad_request(
            "ResourceInUseException",
            "Namespace still has services attached",
        ));
    }
    state.namespaces.remove(&id).ok_or_else(|| {
        AwsError::not_found("NamespaceNotFound", format!("Namespace {id} not found"))
    })?;
    let mut targets = HashMap::new();
    targets.insert("NAMESPACE".to_string(), id);
    let op_id = record_operation(state, "DELETE_NAMESPACE", targets);
    Ok(json!({ "OperationId": op_id }))
}

pub fn get_namespace(
    state: &ServiceDiscoveryState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = require_str(input, "Id")?;
    let n = state.namespaces.get(id).ok_or_else(|| {
        AwsError::not_found("NamespaceNotFound", format!("Namespace {id} not found"))
    })?;
    Ok(json!({ "Namespace": ns_to_value(&n) }))
}

pub fn list_namespaces(
    state: &ServiceDiscoveryState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .namespaces
        .iter()
        .map(|e| ns_to_value(e.value()))
        .collect();
    Ok(json!({ "Namespaces": items }))
}

// ---------- Services ----------

pub fn create_service(
    state: &ServiceDiscoveryState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "Name")?.to_string();
    let namespace_id = input
        .get("NamespaceId")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "NamespaceId is required"))?;
    if !state.namespaces.contains_key(&namespace_id) {
        return Err(AwsError::not_found(
            "NamespaceNotFound",
            format!("Namespace {namespace_id} not found"),
        ));
    }
    let id = new_id('s');
    let svc_type = if input.get("DnsConfig").is_some() {
        "DNS"
    } else {
        "HTTP"
    };
    let svc = ServiceEntry {
        id: id.clone(),
        arn: service_arn(ctx, &id),
        name,
        namespace_id: namespace_id.clone(),
        description: input
            .get("Description")
            .and_then(|v| v.as_str())
            .map(String::from),
        instance_count: 0,
        dns_config: input.get("DnsConfig").cloned(),
        health_check_config: input.get("HealthCheckConfig").cloned(),
        health_check_custom_config: input.get("HealthCheckCustomConfig").cloned(),
        create_date: now(),
        creator_request_id: input
            .get("CreatorRequestId")
            .and_then(|v| v.as_str())
            .map(String::from),
        r#type: svc_type.to_string(),
        instances_revision: 0,
    };
    let result = json!({ "Service": svc_to_value(&svc) });
    state.services.insert(id, svc);
    if let Some(mut n) = state.namespaces.get_mut(&namespace_id) {
        n.service_count += 1;
    }
    Ok(result)
}

pub fn delete_service(
    state: &ServiceDiscoveryState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = require_str(input, "Id")?;
    if state.instances.iter().any(|e| e.value().service_id == id) {
        return Err(AwsError::bad_request(
            "ResourceInUseException",
            "Service still has registered instances",
        ));
    }
    let (_, svc) = state
        .services
        .remove(id)
        .ok_or_else(|| AwsError::not_found("ServiceNotFound", format!("Service {id} not found")))?;
    if let Some(mut n) = state.namespaces.get_mut(&svc.namespace_id)
        && n.service_count > 0
    {
        n.service_count -= 1;
    }
    Ok(json!({}))
}

pub fn get_service(
    state: &ServiceDiscoveryState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = require_str(input, "Id")?;
    let s = state
        .services
        .get(id)
        .ok_or_else(|| AwsError::not_found("ServiceNotFound", format!("Service {id} not found")))?;
    Ok(json!({ "Service": svc_to_value(&s) }))
}

pub fn list_services(
    state: &ServiceDiscoveryState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // Optional filter on NAMESPACE_ID
    let ns_filter = input
        .get("Filters")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|f| {
                let name = f.get("Name").and_then(|n| n.as_str())?;
                if name == "NAMESPACE_ID" {
                    f.get("Values")
                        .and_then(|v| v.as_array())
                        .and_then(|arr| arr.first().cloned())
                        .and_then(|v| v.as_str().map(String::from))
                } else {
                    None
                }
            })
        });
    let items: Vec<Value> = state
        .services
        .iter()
        .filter(|e| match &ns_filter {
            Some(ns) => e.value().namespace_id == *ns,
            None => true,
        })
        .map(|e| svc_to_value(e.value()))
        .collect();
    Ok(json!({ "Services": items }))
}

// ---------- Instances ----------

pub fn register_instance(
    state: &ServiceDiscoveryState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let service_id = input
        .get("ServiceId")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "ServiceId is required"))?;
    if !state.services.contains_key(&service_id) {
        return Err(AwsError::not_found(
            "ServiceNotFound",
            format!("Service {service_id} not found"),
        ));
    }
    let instance_id = require_str(input, "InstanceId")?.to_string();
    let attrs: HashMap<String, String> = input
        .get("Attributes")
        .and_then(|v| v.as_object())
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    validate_instance_attributes(&attrs)?;
    let inst = Instance {
        id: instance_id.clone(),
        service_id: service_id.clone(),
        creator_request_id: input
            .get("CreatorRequestId")
            .and_then(|v| v.as_str())
            .map(String::from),
        attributes: attrs,
    };
    state
        .instances
        .insert(instance_key(&service_id, &instance_id), inst);
    if let Some(mut s) = state.services.get_mut(&service_id) {
        s.instance_count += 1;
        s.instances_revision = s.instances_revision.saturating_add(1);
    }
    let mut targets = HashMap::new();
    targets.insert("INSTANCE".to_string(), instance_id);
    targets.insert("SERVICE".to_string(), service_id);
    let op_id = record_operation(state, "REGISTER_INSTANCE", targets);
    Ok(json!({ "OperationId": op_id }))
}

pub fn deregister_instance(
    state: &ServiceDiscoveryState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let service_id = require_str(input, "ServiceId")?.to_string();
    let instance_id = require_str(input, "InstanceId")?.to_string();
    state
        .instances
        .remove(&instance_key(&service_id, &instance_id))
        .ok_or_else(|| {
            AwsError::not_found(
                "InstanceNotFound",
                format!("Instance {instance_id} not found"),
            )
        })?;
    if let Some(mut s) = state.services.get_mut(&service_id) {
        if s.instance_count > 0 {
            s.instance_count -= 1;
        }
        s.instances_revision = s.instances_revision.saturating_add(1);
    }
    let mut targets = HashMap::new();
    targets.insert("INSTANCE".to_string(), instance_id);
    targets.insert("SERVICE".to_string(), service_id);
    let op_id = record_operation(state, "DEREGISTER_INSTANCE", targets);
    Ok(json!({ "OperationId": op_id }))
}

pub fn get_instance(
    state: &ServiceDiscoveryState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let service_id = require_str(input, "ServiceId")?;
    let instance_id = require_str(input, "InstanceId")?;
    let i = state
        .instances
        .get(&instance_key(service_id, instance_id))
        .ok_or_else(|| {
            AwsError::not_found(
                "InstanceNotFound",
                format!("Instance {instance_id} not found"),
            )
        })?;
    Ok(json!({ "Instance": inst_to_value(&i) }))
}

pub fn list_instances(
    state: &ServiceDiscoveryState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let service_id = require_str(input, "ServiceId")?;
    let items: Vec<Value> = state
        .instances
        .iter()
        .filter(|e| e.value().service_id == service_id)
        .map(|e| inst_to_value(e.value()))
        .collect();
    Ok(json!({ "Instances": items }))
}

pub fn discover_instances(
    state: &ServiceDiscoveryState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let namespace_name = require_str(input, "NamespaceName")?;
    let service_name = require_str(input, "ServiceName")?;

    // Resolve namespace by name → id, then service by name + namespace_id.
    let ns = state
        .namespaces
        .iter()
        .find(|e| e.value().name == namespace_name)
        .map(|e| e.value().clone());
    let Some(ns) = ns else {
        return Ok(json!({ "Instances": [] }));
    };
    let svc = state
        .services
        .iter()
        .find(|e| {
            let s = e.value();
            s.name == service_name && s.namespace_id == ns.id
        })
        .map(|e| e.value().clone());
    let Some(svc) = svc else {
        return Ok(json!({ "Instances": [] }));
    };

    let items: Vec<Value> = state
        .instances
        .iter()
        .filter(|e| e.value().service_id == svc.id)
        .map(|e| {
            let i = e.value();
            json!({
                "InstanceId": i.id,
                "NamespaceName": ns.name,
                "ServiceName": svc.name,
                "HealthStatus": "HEALTHY",
                "Attributes": i.attributes,
            })
        })
        .collect();
    Ok(json!({ "Instances": items, "InstancesRevision": svc.instances_revision }))
}

// ---------- Operations ----------

pub fn get_operation(
    state: &ServiceDiscoveryState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = require_str(input, "OperationId")?;
    let o = state.operations.get(id).ok_or_else(|| {
        AwsError::not_found("OperationNotFound", format!("Operation {id} not found"))
    })?;
    Ok(json!({
        "Operation": {
            "Id": o.id,
            "Type": o.r#type,
            "Status": o.status,
            "ErrorMessage": o.error_message,
            "ErrorCode": o.error_code,
            "CreateDate": o.create_date,
            "UpdateDate": o.update_date,
            "Targets": o.targets,
        }
    }))
}

pub fn list_operations(
    state: &ServiceDiscoveryState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .operations
        .iter()
        .map(|e| {
            let o = e.value();
            json!({ "Id": o.id, "Status": o.status })
        })
        .collect();
    Ok(json!({ "Operations": items }))
}

/// Reserved `AWS_*` attribute keys that `RegisterInstance` accepts.
/// Anything not in this list but starting with the `AWS_` prefix is
/// rejected with `InvalidInput`; custom (non-`AWS_`) keys are allowed
/// through. Values for the bounded set are validated when their
/// formats matter (e.g. `AWS_INIT_HEALTH_STATUS`).
const RESERVED_INSTANCE_ATTRS: &[&str] = &[
    "AWS_ALIAS_DNS_NAME",
    "AWS_EC2_INSTANCE_ID",
    "AWS_INIT_HEALTH_STATUS",
    "AWS_INSTANCE_CNAME",
    "AWS_INSTANCE_IPV4",
    "AWS_INSTANCE_IPV6",
    "AWS_INSTANCE_PORT",
];

fn validate_instance_attributes(attrs: &HashMap<String, String>) -> Result<(), AwsError> {
    for (k, v) in attrs {
        if k.starts_with("AWS_") && !RESERVED_INSTANCE_ATTRS.contains(&k.as_str()) {
            return Err(AwsError::bad_request(
                "InvalidInput",
                format!("Attribute key `{k}` is not in the AWS_* allowlist."),
            ));
        }
        if k == "AWS_INIT_HEALTH_STATUS" && !matches!(v.as_str(), "HEALTHY" | "UNHEALTHY") {
            return Err(AwsError::bad_request(
                "InvalidInput",
                format!("AWS_INIT_HEALTH_STATUS `{v}` must be HEALTHY or UNHEALTHY."),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod revision_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("servicediscovery", "us-east-1")
    }

    fn fresh_service() -> (ServiceDiscoveryState, String, String) {
        let state = ServiceDiscoveryState::default();
        let ns = create_http_namespace(
            &state,
            &json!({ "Name": "ns", "CreatorRequestId": "cr1" }),
            &ctx(),
        )
        .unwrap();
        let ns_id = ns["OperationId"].as_str().unwrap().to_string();
        // create_http_namespace records an operation; the namespace
        // id is stored on the operation's Targets. Look it up.
        let ns_real = state
            .operations
            .get(&ns_id)
            .unwrap()
            .targets
            .get("NAMESPACE")
            .cloned()
            .unwrap();
        let svc = create_service(
            &state,
            &json!({ "Name": "svc", "NamespaceId": ns_real, "Type": "HTTP" }),
            &ctx(),
        )
        .unwrap();
        let svc_id = svc["Service"]["Id"].as_str().unwrap().to_string();
        let ns_name = "ns".to_string();
        (state, svc_id, ns_name)
    }

    #[test]
    fn instances_revision_bumps_on_register_and_deregister() {
        let (state, svc_id, ns_name) = fresh_service();

        // Initial: revision 0, no instances.
        let resp = discover_instances(
            &state,
            &json!({ "NamespaceName": ns_name, "ServiceName": "svc" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["InstancesRevision"], 0);

        register_instance(
            &state,
            &json!({ "ServiceId": svc_id, "InstanceId": "i1", "Attributes": { "AWS_INSTANCE_IPV4": "1.2.3.4" } }),
            &ctx(),
        )
        .unwrap();
        let resp = discover_instances(
            &state,
            &json!({ "NamespaceName": ns_name, "ServiceName": "svc" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["InstancesRevision"], 1);

        register_instance(
            &state,
            &json!({ "ServiceId": svc_id, "InstanceId": "i2", "Attributes": { "AWS_INSTANCE_IPV4": "1.2.3.5" } }),
            &ctx(),
        )
        .unwrap();
        let resp = discover_instances(
            &state,
            &json!({ "NamespaceName": ns_name, "ServiceName": "svc" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["InstancesRevision"], 2);

        deregister_instance(
            &state,
            &json!({ "ServiceId": svc_id, "InstanceId": "i1" }),
            &ctx(),
        )
        .unwrap();
        let resp = discover_instances(
            &state,
            &json!({ "NamespaceName": ns_name, "ServiceName": "svc" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["InstancesRevision"], 3);
    }

    #[test]
    fn register_rejects_unknown_aws_attribute_key() {
        let (state, svc_id, _) = fresh_service();
        let err = register_instance(
            &state,
            &json!({
                "ServiceId": svc_id,
                "InstanceId": "i1",
                "Attributes": { "AWS_NOT_REAL": "v" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidInput");
        assert!(err.message.contains("AWS_"));
    }

    #[test]
    fn register_accepts_custom_attribute_key() {
        let (state, svc_id, _) = fresh_service();
        register_instance(
            &state,
            &json!({
                "ServiceId": svc_id,
                "InstanceId": "i1",
                "Attributes": { "custom-key": "v" },
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn register_rejects_bad_init_health_status() {
        let (state, svc_id, _) = fresh_service();
        let err = register_instance(
            &state,
            &json!({
                "ServiceId": svc_id,
                "InstanceId": "i1",
                "Attributes": { "AWS_INIT_HEALTH_STATUS": "MAYBE" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidInput");
    }

    #[test]
    fn register_accepts_documented_aws_attrs() {
        let (state, svc_id, _) = fresh_service();
        register_instance(
            &state,
            &json!({
                "ServiceId": svc_id,
                "InstanceId": "i1",
                "Attributes": {
                    "AWS_INSTANCE_IPV4": "1.2.3.4",
                    "AWS_INSTANCE_PORT": "80",
                    "AWS_INIT_HEALTH_STATUS": "HEALTHY",
                    "tier": "blue",
                },
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn create_private_dns_namespace_requires_valid_vpc() {
        let state = ServiceDiscoveryState::default();
        // Missing Vpc.
        let err =
            create_private_dns_namespace(&state, &json!({ "Name": "ns" }), &ctx()).unwrap_err();
        assert!(err.message.contains("Vpc"));

        // Wrong prefix.
        let err = create_private_dns_namespace(
            &state,
            &json!({ "Name": "ns", "Vpc": "subnet-12345678" }),
            &ctx(),
        )
        .unwrap_err();
        assert!(err.message.to_lowercase().contains("vpc"));

        // Too short body.
        let err =
            create_private_dns_namespace(&state, &json!({ "Name": "ns", "Vpc": "vpc-12" }), &ctx())
                .unwrap_err();
        assert_eq!(err.code, "InvalidInput");

        // Non-hex body.
        let err = create_private_dns_namespace(
            &state,
            &json!({ "Name": "ns", "Vpc": "vpc-zzzzzzzz" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidInput");

        // Well-formed VPC: accepted.
        create_private_dns_namespace(
            &state,
            &json!({ "Name": "ns", "Vpc": "vpc-0123abcd" }),
            &ctx(),
        )
        .unwrap();
    }
}
