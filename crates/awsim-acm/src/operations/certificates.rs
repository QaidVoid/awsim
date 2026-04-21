use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{AcmState, Certificate, DnsValidationRecord};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Generate a structurally valid but fake PEM certificate block.
fn fake_pem_certificate(domain: &str) -> String {
    // Base64-encoded placeholder bytes (not a real cert, but passes format checks).
    let placeholder = format!(
        "FAKECERT:domain={domain}:serial={}",
        Uuid::new_v4().simple()
    );
    let encoded = base64::engine::general_purpose::STANDARD.encode(placeholder.as_bytes());
    // Wrap at 64 chars per line
    let wrapped: String = encoded
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "-----BEGIN CERTIFICATE-----\n{wrapped}\n-----END CERTIFICATE-----\n"
    )
}

fn fake_pem_chain() -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(b"FAKECHAIN:AWSim-Root-CA");
    let wrapped: String = encoded
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "-----BEGIN CERTIFICATE-----\n{wrapped}\n-----END CERTIFICATE-----\n"
    )
}

fn fake_pem_private_key() -> String {
    let encoded =
        base64::engine::general_purpose::STANDARD.encode(b"FAKEPRIVATEKEY:AWSim-placeholder");
    let wrapped: String = encoded
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "-----BEGIN RSA PRIVATE KEY-----\n{wrapped}\n-----END RSA PRIVATE KEY-----\n"
    )
}

/// Build DNS validation records for all domains.
fn build_dns_records(domains: &[String]) -> HashMap<String, DnsValidationRecord> {
    let mut map = HashMap::new();
    for domain in domains {
        let challenge_name = format!("_acme-challenge.{}.", domain.trim_end_matches('.'));
        let challenge_value = format!(
            "{}.acm-validation.aws.",
            Uuid::new_v4().simple()
        );
        map.insert(
            domain.clone(),
            DnsValidationRecord {
                name: challenge_name,
                record_type: "CNAME".to_string(),
                value: challenge_value,
            },
        );
    }
    map
}

// ---------------------------------------------------------------------------
// RequestCertificate
// ---------------------------------------------------------------------------

pub fn request_certificate(
    state: &AcmState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let domain_name = input["DomainName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "DomainName is required"))?
        .to_string();

    let validation_method = input["ValidationMethod"]
        .as_str()
        .unwrap_or("DNS")
        .to_string();

    if !["DNS", "EMAIL"].contains(&validation_method.as_str()) {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            "ValidationMethod must be DNS or EMAIL",
        ));
    }

    let mut sans: Vec<String> = input["SubjectAlternativeNames"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    // domain_name is always included in SANs
    if !sans.contains(&domain_name) {
        sans.insert(0, domain_name.clone());
    }

    let cert_id = Uuid::new_v4();
    let certificate_arn = format!(
        "arn:aws:acm:{}:{}:certificate/{}",
        ctx.region, ctx.account_id, cert_id
    );

    let dns_validation_records = if validation_method == "DNS" {
        build_dns_records(&sans)
    } else {
        HashMap::new()
    };

    // Tags from input
    let mut tags: HashMap<String, String> = HashMap::new();
    if let Some(tag_list) = input["Tags"].as_array() {
        for tag in tag_list {
            if let (Some(k), Some(v)) = (tag["Key"].as_str(), tag["Value"].as_str()) {
                tags.insert(k.to_string(), v.to_string());
            }
        }
    }

    let cert = Certificate {
        certificate_arn: certificate_arn.clone(),
        domain_name,
        subject_alternative_names: sans,
        status: "ISSUED".to_string(), // auto-issue for local dev
        validation_method,
        dns_validation_records,
        tags,
        created_at: now_secs(),
    };

    state
        .certificates
        .insert(certificate_arn.clone(), cert);

    Ok(json!({ "CertificateArn": certificate_arn }))
}

// ---------------------------------------------------------------------------
// DescribeCertificate
// ---------------------------------------------------------------------------

pub fn describe_certificate(
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

    // Build DomainValidationOptions
    let domain_validation: Vec<Value> = cert
        .subject_alternative_names
        .iter()
        .map(|domain| {
            let mut obj = json!({
                "DomainName": domain,
                "ValidationMethod": cert.validation_method,
                "ValidationStatus": "SUCCESS",
            });
            if cert.validation_method == "DNS" {
                if let Some(rec) = cert.dns_validation_records.get(domain) {
                    obj["ResourceRecord"] = json!({
                        "Name": rec.name,
                        "Type": rec.record_type,
                        "Value": rec.value,
                    });
                }
            }
            obj
        })
        .collect();

    let certificate_obj = json!({
        "CertificateArn": cert.certificate_arn,
        "DomainName": cert.domain_name,
        "SubjectAlternativeNames": cert.subject_alternative_names,
        "Status": cert.status,
        "Type": "AMAZON_ISSUED",
        "KeyAlgorithm": "RSA_2048",
        "SignatureAlgorithm": "SHA256WITHRSA",
        "InUseBy": [],
        "NotBefore": cert.created_at,
        "NotAfter": cert.created_at + 365 * 24 * 3600,
        "CreatedAt": cert.created_at,
        "IssuedAt": cert.created_at,
        "Issuer": "Amazon",
        "Subject": format!("CN={}", cert.domain_name),
        "DomainValidationOptions": domain_validation,
    });

    Ok(json!({ "Certificate": certificate_obj }))
}

// ---------------------------------------------------------------------------
// ListCertificates
// ---------------------------------------------------------------------------

pub fn list_certificates(
    state: &AcmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list: Vec<Value> = state
        .certificates
        .iter()
        .map(|e| {
            let c = e.value();
            json!({
                "CertificateArn": c.certificate_arn,
                "DomainName": c.domain_name,
                "Status": c.status,
            })
        })
        .collect();

    Ok(json!({ "CertificateSummaryList": list }))
}

// ---------------------------------------------------------------------------
// DeleteCertificate
// ---------------------------------------------------------------------------

pub fn delete_certificate(
    state: &AcmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["CertificateArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "CertificateArn is required"))?;

    if state.certificates.remove(arn).is_none() {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Certificate not found: {arn}"),
        ));
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// GetCertificate
// ---------------------------------------------------------------------------

pub fn get_certificate(
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

    let pem = fake_pem_certificate(&cert.domain_name);
    let chain = fake_pem_chain();

    Ok(json!({
        "Certificate": pem,
        "CertificateChain": chain,
    }))
}

// ---------------------------------------------------------------------------
// ExportCertificate
// ---------------------------------------------------------------------------

pub fn export_certificate(
    state: &AcmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["CertificateArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "CertificateArn is required"))?;

    // Passphrase is required by the real API but we accept anything
    let _passphrase = input["Passphrase"].as_str().unwrap_or("");

    let cert = state.certificates.get(arn).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Certificate not found: {arn}"),
        )
    })?;

    let pem = fake_pem_certificate(&cert.domain_name);
    let chain = fake_pem_chain();
    let key = fake_pem_private_key();

    Ok(json!({
        "Certificate": pem,
        "CertificateChain": chain,
        "PrivateKey": key,
    }))
}
