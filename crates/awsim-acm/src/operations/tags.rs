use awsim_core::tags::{TagOpts, validate_aws_tags};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::AcmState;

pub fn add_tags_to_certificate(
    state: &AcmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["CertificateArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "CertificateArn is required"))?;

    validate_aws_tags(&input["Tags"], &TagOpts::aws_default())?;

    let mut cert = state.certificates.get_mut(arn).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Certificate not found: {arn}"),
        )
    })?;

    if let Some(tags) = input["Tags"].as_array() {
        for tag in tags {
            if let (Some(k), Some(v)) = (tag["Key"].as_str(), tag["Value"].as_str()) {
                cert.tags.insert(k.to_string(), v.to_string());
            }
        }
    }

    Ok(json!({}))
}

pub fn remove_tags_from_certificate(
    state: &AcmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["CertificateArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "CertificateArn is required"))?;

    let mut cert = state.certificates.get_mut(arn).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Certificate not found: {arn}"),
        )
    })?;

    if let Some(tags) = input["Tags"].as_array() {
        for tag in tags {
            if let Some(k) = tag["Key"].as_str() {
                cert.tags.remove(k);
            }
        }
    }

    Ok(json!({}))
}

pub fn list_tags_for_certificate(
    state: &AcmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["CertificateArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "CertificateArn is required"))?;

    let cert = state.certificates.get(arn).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Certificate not found: {arn}"),
        )
    })?;

    let tags: Vec<Value> = cert
        .tags
        .iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();

    Ok(json!({ "Tags": tags }))
}
