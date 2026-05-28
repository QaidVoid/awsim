use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use base64::Engine as _;
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
    format!("-----BEGIN CERTIFICATE-----\n{wrapped}\n-----END CERTIFICATE-----\n")
}

fn fake_pem_chain() -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(b"FAKECHAIN:AWSim-Root-CA");
    let wrapped: String = encoded
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    format!("-----BEGIN CERTIFICATE-----\n{wrapped}\n-----END CERTIFICATE-----\n")
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
    format!("-----BEGIN RSA PRIVATE KEY-----\n{wrapped}\n-----END RSA PRIVATE KEY-----\n")
}

/// Build DNS validation records for all domains.
fn build_dns_records(domains: &[String]) -> HashMap<String, DnsValidationRecord> {
    let mut map = HashMap::new();
    for domain in domains {
        let challenge_name = format!("_acme-challenge.{}.", domain.trim_end_matches('.'));
        let challenge_value = format!("{}.acm-validation.aws.", Uuid::new_v4().simple());
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

    // AWS limits ACM certificates to 10 total names (DomainName + 9
    // SANs). Reject early when the caller requested more.
    let mut sans: Vec<String> = input["SubjectAlternativeNames"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    // Domain validation dedupes case-insensitively across DomainName
    // and SANs. Real AWS rejects with InvalidDomainValidationOptionsException
    // when the same name appears twice.
    let domain_lower = domain_name.to_ascii_lowercase();
    sans.retain(|s| s.to_ascii_lowercase() != domain_lower);
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for s in &sans {
        let key = s.to_ascii_lowercase();
        if !seen.insert(key) {
            return Err(AwsError::bad_request(
                "InvalidDomainValidationOptionsException",
                format!("SubjectAlternativeNames contains the duplicate name `{s}`."),
            ));
        }
    }
    const MAX_SANS: usize = 9;
    if sans.len() > MAX_SANS {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            format!(
                "Up to 9 SubjectAlternativeNames are allowed in addition to DomainName \
                 ({} supplied).",
                sans.len()
            ),
        ));
    }

    // KeyAlgorithm allowlist matches AWS public docs. Defaults to
    // RSA_2048 to match ACM's default; reject anything off-list.
    let key_algorithm = input["KeyAlgorithm"].as_str().unwrap_or("RSA_2048");
    if !matches!(
        key_algorithm,
        "RSA_1024"
            | "RSA_2048"
            | "RSA_3072"
            | "RSA_4096"
            | "EC_prime256v1"
            | "EC_secp384r1"
            | "EC_secp521r1"
    ) {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            format!("KeyAlgorithm `{key_algorithm}` is not supported."),
        ));
    }

    // domain_name is always included in SANs
    sans.insert(0, domain_name.clone());

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

    let ct_logging = input["Options"]
        .get("CertificateTransparencyLoggingPreference")
        .and_then(|v| v.as_str())
        .unwrap_or("ENABLED")
        .to_string();
    if !matches!(ct_logging.as_str(), "ENABLED" | "DISABLED") {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            "Options.CertificateTransparencyLoggingPreference must be ENABLED or DISABLED.",
        ));
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
        in_use_by: Vec::new(),
        certificate_transparency_logging_preference: ct_logging,
        certificate_type: "AMAZON_ISSUED".to_string(),
    };

    state.certificates.insert(certificate_arn.clone(), cert);

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
            if cert.validation_method == "DNS"
                && let Some(rec) = cert.dns_validation_records.get(domain)
            {
                obj["ResourceRecord"] = json!({
                    "Name": rec.name,
                    "Type": rec.record_type,
                    "Value": rec.value,
                });
            }
            obj
        })
        .collect();

    // RenewalEligibility tracks whether ACM can renew the cert
    // automatically. Imported certs have no private key on AWS's
    // side, so the answer is always INELIGIBLE.
    let renewal_eligibility = if cert.certificate_type == "IMPORTED" {
        "INELIGIBLE"
    } else {
        "ELIGIBLE"
    };
    let certificate_obj = json!({
        "CertificateArn": cert.certificate_arn,
        "DomainName": cert.domain_name,
        "SubjectAlternativeNames": cert.subject_alternative_names,
        "Status": cert.status,
        "Type": cert.certificate_type,
        "RenewalEligibility": renewal_eligibility,
        "KeyAlgorithm": "RSA_2048",
        "SignatureAlgorithm": "SHA256WITHRSA",
        "InUseBy": cert.in_use_by,
        "NotBefore": cert.created_at,
        "NotAfter": cert.created_at + 365 * 24 * 3600,
        "CreatedAt": cert.created_at,
        "IssuedAt": cert.created_at,
        "Issuer": "Amazon",
        "Subject": format!("CN={}", cert.domain_name),
        "DomainValidationOptions": domain_validation,
        "Options": {
            "CertificateTransparencyLoggingPreference": cert.certificate_transparency_logging_preference,
        },
    });

    Ok(json!({ "Certificate": certificate_obj }))
}

// ---------------------------------------------------------------------------
// ListCertificates
// ---------------------------------------------------------------------------

pub fn list_certificates(
    state: &AcmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // AWS ListCertificates accepts `CertificateStatuses` as an array of
    // status strings; absence of the filter returns every status the
    // service has issued, not just `ISSUED`.
    let status_filter: Option<Vec<String>> = input
        .get("CertificateStatuses")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        });
    let max_items = awsim_core::clamp_max_results_strict(
        input.get("MaxItems").and_then(Value::as_i64),
        100,
        1000,
    )?;
    let starting_token = input.get("NextToken").and_then(Value::as_str);
    let mut entries: Vec<(String, Value)> = state
        .certificates
        .iter()
        .filter(|e| {
            status_filter
                .as_ref()
                .is_none_or(|set| set.iter().any(|s| s == &e.value().status))
        })
        .map(|e| {
            let c = e.value();
            (
                c.certificate_arn.clone(),
                json!({
                    "CertificateArn": c.certificate_arn,
                    "DomainName": c.domain_name,
                    "Status": c.status,
                }),
            )
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let page = awsim_core::paginate(entries, max_items, starting_token, |(k, _)| k.clone())?;
    let items: Vec<Value> = page.items.into_iter().map(|(_, v)| v).collect();
    let mut body = json!({ "CertificateSummaryList": items });
    if let Some(token) = page.next_token {
        body["NextToken"] = json!(token);
    }
    Ok(body)
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

    let in_use_by = {
        let cert = state.certificates.get(arn).ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Certificate not found: {arn}"),
            )
        })?;
        cert.in_use_by.clone()
    };
    if !in_use_by.is_empty() {
        return Err(AwsError::bad_request(
            "ResourceInUseException",
            format!(
                "Certificate {arn} is in use by {} resource(s) and cannot be deleted.",
                in_use_by.len()
            ),
        ));
    }

    state.certificates.remove(arn);
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

    if cert.status == "PENDING_VALIDATION" {
        return Err(AwsError::bad_request(
            "RequestInProgressException",
            format!("Certificate {arn} is still PENDING_VALIDATION."),
        ));
    }

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

// ---------------------------------------------------------------------------
// ImportCertificate
// ---------------------------------------------------------------------------

pub fn import_certificate(
    state: &AcmState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // Accept Certificate, PrivateKey, CertificateChain (all PEM/base64).
    // We store them but don't validate the content — this is a dev emulator.
    let _certificate = input["Certificate"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Certificate is required"))?;
    let _private_key = input["PrivateKey"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PrivateKey is required"))?;

    let domain_name = input["DomainName"]
        .as_str()
        .unwrap_or("imported.example.com")
        .to_string();

    // If CertificateArn is given, re-import (update) existing certificate
    let certificate_arn = if let Some(existing_arn) = input["CertificateArn"].as_str() {
        if !state.certificates.contains_key(existing_arn) {
            return Err(AwsError::not_found(
                "ResourceNotFoundException",
                format!("Certificate not found: {existing_arn}"),
            ));
        }
        existing_arn.to_string()
    } else {
        let cert_id = Uuid::new_v4();
        format!(
            "arn:aws:acm:{}:{}:certificate/{}",
            ctx.region, ctx.account_id, cert_id
        )
    };

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
        subject_alternative_names: Vec::new(),
        status: "ISSUED".to_string(),
        validation_method: "IMPORTED".to_string(),
        dns_validation_records: HashMap::new(),
        tags,
        created_at: now_secs(),
        in_use_by: Vec::new(),
        certificate_transparency_logging_preference: "ENABLED".to_string(),
        certificate_type: "IMPORTED".to_string(),
    };

    state.certificates.insert(certificate_arn.clone(), cert);

    Ok(json!({ "CertificateArn": certificate_arn }))
}

// ---------------------------------------------------------------------------
// RenewCertificate
// ---------------------------------------------------------------------------

pub fn renew_certificate(
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

    // Imported certs carry no private key on AWS's side, so ACM cannot
    // renew them — the documented response is InvalidRequestException.
    if cert.certificate_type == "IMPORTED" {
        return Err(AwsError::bad_request(
            "InvalidRequestException",
            format!(
                "Certificate {arn} is IMPORTED and cannot be renewed; \
                 re-import a new certificate instead."
            ),
        ));
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UpdateCertificateOptions
// ---------------------------------------------------------------------------

pub fn update_certificate_options(
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

    if let Some(pref) = input["Options"]
        .get("CertificateTransparencyLoggingPreference")
        .and_then(|v| v.as_str())
    {
        if !matches!(pref, "ENABLED" | "DISABLED") {
            return Err(AwsError::bad_request(
                "InvalidParameter",
                "Options.CertificateTransparencyLoggingPreference must be ENABLED or DISABLED.",
            ));
        }
        cert.certificate_transparency_logging_preference = pref.to_string();
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ResendValidationEmail
// ---------------------------------------------------------------------------

pub fn resend_validation_email(
    state: &AcmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["CertificateArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "CertificateArn is required"))?;

    if !state.certificates.contains_key(arn) {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Certificate not found: {arn}"),
        ));
    }

    // Stub: email sending always succeeds
    Ok(json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("acm", "us-east-1")
    }

    fn pending_cert(arn: &str) -> Certificate {
        Certificate {
            certificate_arn: arn.to_string(),
            domain_name: "example.com".to_string(),
            subject_alternative_names: Vec::new(),
            status: "PENDING_VALIDATION".to_string(),
            validation_method: "DNS".to_string(),
            dns_validation_records: HashMap::new(),
            tags: HashMap::new(),
            created_at: 0,
            in_use_by: Vec::new(),
            certificate_transparency_logging_preference: "ENABLED".to_string(),
            certificate_type: "AMAZON_ISSUED".to_string(),
        }
    }

    #[test]
    fn get_certificate_rejects_pending_validation() {
        let state = AcmState::default();
        let arn = "arn:aws:acm:us-east-1:000000000000:certificate/pending";
        state
            .certificates
            .insert(arn.to_string(), pending_cert(arn));

        let err = get_certificate(&state, &json!({ "CertificateArn": arn }), &ctx()).unwrap_err();
        assert_eq!(err.code, "RequestInProgressException");
    }

    #[test]
    fn certificate_transparency_defaults_enabled_and_round_trips() {
        let state = AcmState::default();
        let resp =
            request_certificate(&state, &json!({ "DomainName": "example.com" }), &ctx()).unwrap();
        let arn = resp["CertificateArn"].as_str().unwrap().to_string();
        let desc =
            describe_certificate(&state, &json!({ "CertificateArn": &arn }), &ctx()).unwrap();
        assert_eq!(
            desc["Certificate"]["Options"]["CertificateTransparencyLoggingPreference"],
            "ENABLED"
        );

        update_certificate_options(
            &state,
            &json!({
                "CertificateArn": arn,
                "Options": { "CertificateTransparencyLoggingPreference": "DISABLED" }
            }),
            &ctx(),
        )
        .unwrap();
        let desc =
            describe_certificate(&state, &json!({ "CertificateArn": &arn }), &ctx()).unwrap();
        assert_eq!(
            desc["Certificate"]["Options"]["CertificateTransparencyLoggingPreference"],
            "DISABLED"
        );
    }

    #[test]
    fn update_certificate_options_rejects_invalid_preference() {
        let state = AcmState::default();
        let resp =
            request_certificate(&state, &json!({ "DomainName": "example.com" }), &ctx()).unwrap();
        let arn = resp["CertificateArn"].as_str().unwrap().to_string();
        let err = update_certificate_options(
            &state,
            &json!({
                "CertificateArn": arn,
                "Options": { "CertificateTransparencyLoggingPreference": "MAYBE" }
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
    }

    #[test]
    fn delete_certificate_rejects_when_in_use() {
        let state = AcmState::default();
        let arn = "arn:aws:acm:us-east-1:000000000000:certificate/inuse";
        let mut cert = pending_cert(arn);
        cert.status = "ISSUED".to_string();
        cert.in_use_by.push(
            "arn:aws:elasticloadbalancing:us-east-1:000000000000:loadbalancer/app/x/y".into(),
        );
        state.certificates.insert(arn.to_string(), cert);

        let err =
            delete_certificate(&state, &json!({ "CertificateArn": arn }), &ctx()).unwrap_err();
        assert_eq!(err.code, "ResourceInUseException");
        assert!(state.certificates.contains_key(arn));
    }

    #[test]
    fn delete_certificate_succeeds_when_not_in_use() {
        let state = AcmState::default();
        let arn = "arn:aws:acm:us-east-1:000000000000:certificate/free";
        let mut cert = pending_cert(arn);
        cert.status = "ISSUED".to_string();
        state.certificates.insert(arn.to_string(), cert);

        delete_certificate(&state, &json!({ "CertificateArn": arn }), &ctx()).unwrap();
        assert!(!state.certificates.contains_key(arn));
    }

    #[test]
    fn imported_certificate_is_ineligible_for_renewal() {
        let state = AcmState::default();
        let arn_resp = import_certificate(
            &state,
            &json!({
                "Certificate": "-----BEGIN CERT-----",
                "PrivateKey": "-----BEGIN KEY-----",
                "DomainName": "imp.example.com",
            }),
            &ctx(),
        )
        .unwrap();
        let arn = arn_resp["CertificateArn"].as_str().unwrap();
        let desc = describe_certificate(&state, &json!({ "CertificateArn": arn }), &ctx()).unwrap();
        assert_eq!(desc["Certificate"]["Type"], "IMPORTED");
        assert_eq!(desc["Certificate"]["RenewalEligibility"], "INELIGIBLE");
    }

    #[test]
    fn renew_certificate_rejects_imported() {
        let state = AcmState::default();
        let arn_resp = import_certificate(
            &state,
            &json!({
                "Certificate": "-----BEGIN CERT-----",
                "PrivateKey": "-----BEGIN KEY-----",
                "DomainName": "imp.example.com",
            }),
            &ctx(),
        )
        .unwrap();
        let arn = arn_resp["CertificateArn"].as_str().unwrap();
        let err = renew_certificate(&state, &json!({ "CertificateArn": arn }), &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidRequestException");
        assert!(err.message.contains("IMPORTED"));
    }

    #[test]
    fn amazon_issued_certificate_is_eligible_for_renewal() {
        let state = AcmState::default();
        let resp = request_certificate(&state, &json!({ "DomainName": "iss.example.com" }), &ctx())
            .unwrap();
        let arn = resp["CertificateArn"].as_str().unwrap();
        let desc = describe_certificate(&state, &json!({ "CertificateArn": arn }), &ctx()).unwrap();
        assert_eq!(desc["Certificate"]["Type"], "AMAZON_ISSUED");
        assert_eq!(desc["Certificate"]["RenewalEligibility"], "ELIGIBLE");
        renew_certificate(&state, &json!({ "CertificateArn": arn }), &ctx()).unwrap();
    }

    #[test]
    fn list_certificates_paginates_with_max_items_and_next_token() {
        let state = AcmState::default();
        for i in 0..3 {
            request_certificate(
                &state,
                &json!({ "DomainName": format!("d{i}.example.com") }),
                &ctx(),
            )
            .unwrap();
        }
        let first = list_certificates(&state, &json!({ "MaxItems": 2 }), &ctx()).unwrap();
        let arns: Vec<String> = first["CertificateSummaryList"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["CertificateArn"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(arns.len(), 2);
        let token = first["NextToken"].as_str().unwrap().to_string();
        let second = list_certificates(
            &state,
            &json!({ "MaxItems": 2, "NextToken": token }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(
            second["CertificateSummaryList"].as_array().unwrap().len(),
            1
        );
    }

    #[test]
    fn list_certificates_filters_by_status() {
        let state = AcmState::default();
        request_certificate(
            &state,
            &json!({ "DomainName": "active.example.com" }),
            &ctx(),
        )
        .unwrap();
        let resp = list_certificates(
            &state,
            &json!({ "CertificateStatuses": ["EXPIRED"] }),
            &ctx(),
        )
        .unwrap();
        assert!(
            resp["CertificateSummaryList"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
