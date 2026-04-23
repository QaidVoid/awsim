use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{LoggingConfig, WafState};

fn config_to_value(cfg: &LoggingConfig) -> Value {
    json!({
        "ResourceArn": cfg.resource_arn,
        "LogDestinationConfigs": cfg.log_destination_configs,
        "RedactedFields": cfg.redacted_fields,
        "ManagedByFirewallManager": cfg.managed_by_firewall_manager,
        "LoggingFilter": cfg.logging_filter,
    })
}

pub fn put_logging_configuration(
    state: &WafState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cfg_in = input["LoggingConfiguration"].clone();
    if !cfg_in.is_object() {
        return Err(AwsError::bad_request(
            "WAFInvalidParameterException",
            "LoggingConfiguration is required",
        ));
    }

    let resource_arn = cfg_in["ResourceArn"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("WAFInvalidParameterException", "ResourceArn is required")
        })?
        .to_string();

    let log_destination_configs: Vec<String> = cfg_in["LogDestinationConfigs"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let redacted_fields = cfg_in["RedactedFields"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let managed_by_firewall_manager = cfg_in["ManagedByFirewallManager"]
        .as_bool()
        .unwrap_or(false);
    let logging_filter = cfg_in.get("LoggingFilter").cloned();

    let cfg = LoggingConfig {
        resource_arn: resource_arn.clone(),
        log_destination_configs,
        redacted_fields,
        managed_by_firewall_manager,
        logging_filter,
    };

    let value = config_to_value(&cfg);
    state.logging_configs.insert(resource_arn, cfg);

    Ok(json!({ "LoggingConfiguration": value }))
}

pub fn get_logging_configuration(
    state: &WafState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("WAFInvalidParameterException", "ResourceArn is required")
    })?;

    let cfg = state.logging_configs.get(resource_arn).ok_or_else(|| {
        AwsError::not_found(
            "WAFNonexistentItemException",
            format!("LoggingConfiguration not found: {resource_arn}"),
        )
    })?;

    Ok(json!({ "LoggingConfiguration": config_to_value(&cfg) }))
}

pub fn delete_logging_configuration(
    state: &WafState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("WAFInvalidParameterException", "ResourceArn is required")
    })?;

    state.logging_configs.remove(resource_arn).ok_or_else(|| {
        AwsError::not_found(
            "WAFNonexistentItemException",
            format!("LoggingConfiguration not found: {resource_arn}"),
        )
    })?;

    Ok(json!({}))
}

pub fn list_logging_configurations(
    state: &WafState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let configs: Vec<Value> = state
        .logging_configs
        .iter()
        .map(|e| config_to_value(e.value()))
        .collect();

    Ok(json!({ "LoggingConfigurations": configs }))
}
