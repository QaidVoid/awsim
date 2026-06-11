//! SAML 2.0 federation for Cognito user pools.
//!
//! Covers the two halves of the SP-initiated web-SSO flow:
//!
//!   1. `hosted-UI /authorize?identity_provider=<saml>` builds a SAML
//!      `AuthnRequest`, DEFLATE+base64+URL-encodes it (the HTTP-Redirect
//!      binding), and redirects the browser to the IdP's SSO URL. The
//!      original Cognito authorize parameters are parked under a relay token
//!      carried as `RelayState`.
//!   2. The IdP POSTs a base64 `SAMLResponse` to
//!      `/cognito/{pool_id}/saml2/idpresponse` (the HTTP-POST binding). We
//!      decode the assertion, pull the `NameID` and attribute statements,
//!      map them through the provider's `AttributeMapping`, upsert the
//!      federated user, and hand the app a Cognito authorization code.
//!
//! awsim does not cryptographically verify the IdP's XML signature: there is
//! no real IdP and no trusted key material in an offline emulator, so the
//! assertion is parsed and trusted. Everything else (issuer, NameID,
//! attribute mapping) mirrors Cognito.

use std::collections::HashMap;
use std::io::Write;

use awsim_core::AwsError;
use base64::{Engine, engine::general_purpose::STANDARD};
use flate2::Compression;
use flate2::write::DeflateEncoder;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use serde_json::Value;

use crate::state::IdentityProvider;

/// SP entity id Cognito advertises for a pool: `urn:amazon:cognito:sp:{pool}`.
pub fn sp_entity_id(pool_id: &str) -> String {
    format!("urn:amazon:cognito:sp:{pool_id}")
}

/// Resolved SAML IdP settings needed to start a sign-in.
pub struct SamlConfig {
    /// IdP SingleSignOnService URL for the HTTP-Redirect binding.
    pub sso_redirect_url: String,
}

/// Read the IdP's SSO redirect URL from the provider details: either the
/// explicit `SSORedirectBindingURI` key, or parsed out of the `MetadataFile`
/// XML (`<SingleSignOnService Binding="...HTTP-Redirect" Location=".."/>`).
pub fn parse_saml_config(idp: &IdentityProvider) -> Result<SamlConfig, AwsError> {
    if !idp.provider_type.eq_ignore_ascii_case("SAML") {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!(
                "identity_provider {} is type {}, expected SAML",
                idp.provider_name, idp.provider_type
            ),
        ));
    }
    if let Some(url) = idp
        .provider_details
        .get("SSORedirectBindingURI")
        .filter(|s| !s.is_empty())
    {
        return Ok(SamlConfig {
            sso_redirect_url: url.clone(),
        });
    }
    if let Some(metadata) = idp.provider_details.get("MetadataFile")
        && let Some(url) = sso_url_from_metadata(metadata)
    {
        return Ok(SamlConfig {
            sso_redirect_url: url,
        });
    }
    Err(AwsError::bad_request(
        "InvalidParameterException",
        format!(
            "SAML provider {} has no SSORedirectBindingURI or parsable MetadataFile",
            idp.provider_name
        ),
    ))
}

/// Pull the HTTP-Redirect `SingleSignOnService` Location out of an IdP
/// metadata document. Falls back to the first SSO Location of any binding.
fn sso_url_from_metadata(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut fallback: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e))
                if local_name(e.name().as_ref()) == b"SingleSignOnService" =>
            {
                let mut binding = None;
                let mut location = None;
                for attr in e.attributes().flatten() {
                    match attr.key.as_ref() {
                        b"Binding" => binding = attr.unescape_value().ok().map(|v| v.into_owned()),
                        b"Location" => {
                            location = attr.unescape_value().ok().map(|v| v.into_owned())
                        }
                        _ => {}
                    }
                }
                if let Some(loc) = location {
                    if binding
                        .as_deref()
                        .is_some_and(|b| b.ends_with("HTTP-Redirect"))
                    {
                        return Some(loc);
                    }
                    fallback.get_or_insert(loc);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    fallback
}

/// Build the redirect URL to the IdP's SSO endpoint carrying a DEFLATE+base64
/// `SAMLRequest` (HTTP-Redirect binding) and the `RelayState` token.
pub fn build_authn_request_url(
    sso_url: &str,
    sp_entity_id: &str,
    acs_url: &str,
    relay_state: &str,
    request_id: &str,
    issue_instant: &str,
) -> String {
    let authn_request = format!(
        concat!(
            r#"<samlp:AuthnRequest xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" "#,
            r#"xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion" "#,
            r#"ID="{id}" Version="2.0" IssueInstant="{instant}" "#,
            r#"Destination="{dest}" AssertionConsumerServiceURL="{acs}" "#,
            r#"ProtocolBinding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST">"#,
            r#"<saml:Issuer>{issuer}</saml:Issuer>"#,
            r#"</samlp:AuthnRequest>"#,
        ),
        id = request_id,
        instant = issue_instant,
        dest = xml_escape(sso_url),
        acs = xml_escape(acs_url),
        issuer = xml_escape(sp_entity_id),
    );

    let encoded = deflate_b64(authn_request.as_bytes());
    let sep = if sso_url.contains('?') { '&' } else { '?' };
    format!(
        "{sso_url}{sep}SAMLRequest={}&RelayState={}",
        urlencode(&encoded),
        urlencode(relay_state),
    )
}

/// Raw-DEFLATE then base64 (standard alphabet), per the HTTP-Redirect binding.
fn deflate_b64(data: &[u8]) -> String {
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
    let _ = enc.write_all(data);
    let compressed = enc.finish().unwrap_or_default();
    STANDARD.encode(compressed)
}

/// Identity extracted from a verified-enough SAML assertion.
#[derive(Debug)]
pub struct SamlAssertion {
    /// The `<saml:NameID>` value: the federated subject.
    pub name_id: String,
    /// Attribute-statement values keyed by the SAML `Attribute Name`. Values
    /// are JSON so they slot straight into [`crate::federation::map_attributes`].
    pub attributes: HashMap<String, Value>,
}

/// Parse a decoded `SAMLResponse` document, returning the subject NameID and
/// its attribute statements. Errors when no assertion / NameID is present.
pub fn parse_saml_response(xml: &[u8]) -> Result<SamlAssertion, AwsError> {
    let xml = std::str::from_utf8(xml).map_err(|_| {
        AwsError::bad_request("InvalidParameterException", "SAMLResponse is not UTF-8")
    })?;
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut name_id: Option<String> = None;
    let mut attributes: HashMap<String, Value> = HashMap::new();

    // Tracks the open <Attribute Name="..."> while reading its values, and
    // whether we are inside a <NameID> element.
    let mut current_attr: Option<String> = None;
    let mut in_name_id = false;
    let mut in_attr_value = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                b"NameID" => in_name_id = true,
                b"Attribute" => {
                    current_attr = attr_value(&e, b"Name");
                }
                b"AttributeValue" => in_attr_value = true,
                _ => {}
            },
            Ok(Event::End(e)) => match local_name(e.name().as_ref()) {
                b"NameID" => in_name_id = false,
                b"Attribute" => current_attr = None,
                b"AttributeValue" => in_attr_value = false,
                _ => {}
            },
            Ok(Event::Text(t)) => {
                let text = t.unescape().unwrap_or_default().into_owned();
                if in_name_id && name_id.is_none() {
                    name_id = Some(text);
                } else if in_attr_value && let Some(name) = &current_attr {
                    // First value wins for a given attribute (Cognito maps
                    // single-valued attributes).
                    attributes
                        .entry(name.clone())
                        .or_insert_with(|| Value::String(text));
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(AwsError::bad_request(
                    "InvalidParameterException",
                    format!("SAMLResponse is not well-formed XML: {e}"),
                ));
            }
            _ => {}
        }
    }

    let name_id = name_id.ok_or_else(|| {
        AwsError::bad_request(
            "NotAuthorizedException",
            "SAMLResponse assertion has no NameID",
        )
    })?;
    Ok(SamlAssertion {
        name_id,
        attributes,
    })
}

/// Strip any XML namespace prefix, returning the local element/attribute name.
fn local_name(qname: &[u8]) -> &[u8] {
    match qname.iter().position(|&b| b == b':') {
        Some(i) => &qname[i + 1..],
        None => qname,
    }
}

/// Read a named attribute off a start element.
fn attr_value(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
}

/// Minimal XML attribute/text escaping for the AuthnRequest we emit.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Percent-encode a query-parameter value.
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(s: &str) -> String {
        STANDARD.encode(s)
    }

    #[test]
    fn parses_nameid_and_attributes() {
        let xml = r#"<samlp:Response xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol"
            xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion">
          <saml:Assertion>
            <saml:Subject>
              <saml:NameID Format="...emailAddress">alice@corp.example</saml:NameID>
            </saml:Subject>
            <saml:AttributeStatement>
              <saml:Attribute Name="http://schemas.xmlsoap.org/claims/EmailAddress">
                <saml:AttributeValue>alice@corp.example</saml:AttributeValue>
              </saml:Attribute>
              <saml:Attribute Name="firstName">
                <saml:AttributeValue>Alice</saml:AttributeValue>
              </saml:Attribute>
            </saml:AttributeStatement>
          </saml:Assertion>
        </samlp:Response>"#;
        let a = parse_saml_response(xml.as_bytes()).unwrap();
        assert_eq!(a.name_id, "alice@corp.example");
        assert_eq!(
            a.attributes["http://schemas.xmlsoap.org/claims/EmailAddress"],
            "alice@corp.example"
        );
        assert_eq!(a.attributes["firstName"], "Alice");
    }

    #[test]
    fn rejects_assertion_without_nameid() {
        let xml = r#"<saml:Assertion xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion">
          <saml:AttributeStatement></saml:AttributeStatement>
        </saml:Assertion>"#;
        let err = parse_saml_response(xml.as_bytes()).unwrap_err();
        assert_eq!(err.code, "NotAuthorizedException");
    }

    #[test]
    fn base64_roundtrip_response_parses() {
        let xml = r#"<saml:Assertion xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion">
          <saml:Subject><saml:NameID>bob</saml:NameID></saml:Subject>
        </saml:Assertion>"#;
        let encoded = b64(xml);
        let decoded = STANDARD.decode(&encoded).unwrap();
        let a = parse_saml_response(&decoded).unwrap();
        assert_eq!(a.name_id, "bob");
    }

    #[test]
    fn authn_request_url_carries_samlrequest_and_relaystate() {
        let url = build_authn_request_url(
            "https://idp.example/sso",
            &sp_entity_id("us-east-1_abc"),
            "https://app.test/cognito/us-east-1_abc/saml2/idpresponse",
            "relay-token-123",
            "_reqid",
            "2026-01-01T00:00:00Z",
        );
        assert!(url.starts_with("https://idp.example/sso?SAMLRequest="));
        assert!(url.contains("&RelayState=relay-token-123"));
    }

    #[test]
    fn sso_url_parsed_from_metadata_prefers_redirect_binding() {
        let md = r#"<EntityDescriptor xmlns="urn:oasis:names:tc:SAML:2.0:metadata">
          <IDPSSODescriptor>
            <SingleSignOnService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST"
              Location="https://idp.example/post"/>
            <SingleSignOnService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect"
              Location="https://idp.example/redirect"/>
          </IDPSSODescriptor>
        </EntityDescriptor>"#;
        assert_eq!(
            sso_url_from_metadata(md).as_deref(),
            Some("https://idp.example/redirect")
        );
    }
}
