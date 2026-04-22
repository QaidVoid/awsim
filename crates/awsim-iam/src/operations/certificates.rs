use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{entity_already_exists, no_such_entity},
    ids::{new_server_certificate_id, normalize_path, now_iso8601},
    state::{IamState, ServerCertificate},
};

use super::{opt_str, require_str};
use super::super::operations::tags::{parse_tag_keys, parse_tags, tags_to_value};

fn cert_metadata_to_value(c: &ServerCertificate) -> Value {
    let mut v = json!({
        "ServerCertificateName": c.server_certificate_name,
        "ServerCertificateId": c.server_certificate_id,
        "Arn": c.arn,
        "Path": c.path,
        "UploadDate": c.upload_date,
    });
    if let Some(exp) = &c.expiration {
        v["Expiration"] = Value::String(exp.clone());
    }
    v
}

fn cert_to_value(c: &ServerCertificate) -> Value {
    json!({
        "ServerCertificateMetadata": cert_metadata_to_value(c),
        "CertificateBody": c.certificate_body,
        "CertificateChain": c.certificate_chain.clone().unwrap_or_default(),
    })
}

pub fn upload_server_certificate(
    state: &IamState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "ServerCertificateName")?;
    let certificate_body = require_str(input, "CertificateBody")?;
    let _private_key = require_str(input, "PrivateKey")?;
    let certificate_chain = opt_str(input, "CertificateChain").map(|s| s.to_string());
    let path = normalize_path(opt_str(input, "Path"));

    if state.server_certificates.contains_key(name) {
        return Err(entity_already_exists("ServerCertificate", name));
    }

    let cert_id = new_server_certificate_id();
    let arn = format!(
        "arn:aws:iam::{}:server-certificate{}{}",
        ctx.account_id, path, name
    );

    let cert = ServerCertificate {
        server_certificate_name: name.to_string(),
        server_certificate_id: cert_id,
        arn,
        path,
        certificate_body: certificate_body.to_string(),
        certificate_chain,
        upload_date: now_iso8601(),
        expiration: None,
        tags: std::collections::HashMap::new(),
    };

    let result = cert_metadata_to_value(&cert);
    state.server_certificates.insert(name.to_string(), cert);

    Ok(json!({ "ServerCertificateMetadata": result }))
}

pub fn get_server_certificate(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let name = require_str(input, "ServerCertificateName")?;
    let cert = state
        .server_certificates
        .get(name)
        .ok_or_else(|| no_such_entity("ServerCertificate", name))?;
    Ok(json!({ "ServerCertificate": cert_to_value(&cert) }))
}

pub fn list_server_certificates(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let path_prefix = opt_str(input, "PathPrefix").unwrap_or("/");
    let list: Vec<Value> = state
        .server_certificates
        .iter()
        .filter(|c| c.path.starts_with(path_prefix))
        .map(|c| cert_metadata_to_value(&c))
        .collect();

    Ok(json!({
        "ServerCertificateMetadataList": { "member": list },
        "IsTruncated": false,
    }))
}

pub fn delete_server_certificate(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let name = require_str(input, "ServerCertificateName")?;

    if state.server_certificates.remove(name).is_none() {
        return Err(no_such_entity("ServerCertificate", name));
    }

    Ok(json!({}))
}

pub fn tag_server_certificate(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let name = require_str(input, "ServerCertificateName")?;
    let new_tags = parse_tags(input);

    let mut cert = state
        .server_certificates
        .get_mut(name)
        .ok_or_else(|| no_such_entity("ServerCertificate", name))?;

    for (k, v) in new_tags {
        cert.tags.insert(k, v);
    }

    Ok(json!({}))
}

pub fn untag_server_certificate(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let name = require_str(input, "ServerCertificateName")?;
    let keys = parse_tag_keys(input);

    let mut cert = state
        .server_certificates
        .get_mut(name)
        .ok_or_else(|| no_such_entity("ServerCertificate", name))?;

    for k in &keys {
        cert.tags.remove(k);
    }

    Ok(json!({}))
}

pub fn list_server_certificate_tags(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let name = require_str(input, "ServerCertificateName")?;
    let cert = state
        .server_certificates
        .get(name)
        .ok_or_else(|| no_such_entity("ServerCertificate", name))?;

    Ok(json!({
        "Tags": tags_to_value(&cert.tags),
        "IsTruncated": false,
    }))
}
