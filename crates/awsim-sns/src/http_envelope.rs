//! Deterministic SNS HTTP/HTTPS delivery envelope.
//!
//! AWS posts notifications to HTTP(S) subscriptions as JSON wrapped in
//! a signed envelope: every message carries `SignatureVersion=1`, a
//! base64-encoded RSA signature over a fixed canonicalisation of the
//! payload, and `SigningCertURL` pointing at the PEM the subscriber
//! uses to verify. The simulator can't issue real RSA signatures
//! without a private key per region, but the contract callers actually
//! depend on is:
//!
//! 1. The JSON shape is stable.
//! 2. The signature is deterministic given the same inputs (so test
//!    fixtures don't drift).
//! 3. `SigningCertURL` is a real URL the subscriber can reach.
//!
//! We satisfy all three with an HMAC-SHA256 signature keyed off the
//! pagination signing key (per-process random) and a cert URL that
//! points at a fixed `/_awsim/sns/SimpleNotificationService-{region}.pem`
//! mock — callers verifying signatures end-to-end need real keys, but
//! everyone else gets a byte-for-byte deterministic envelope.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use hmac::{Hmac, Mac};
use serde_json::{Map, Value, json};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Inputs needed to build an SNS HTTP notification envelope. Held
/// together as a struct so the field order isn't load-bearing and so
/// future additions (Subject, MessageAttributes, …) don't reshuffle
/// every call site.
#[derive(Debug, Clone)]
pub struct NotificationInputs<'a> {
    pub topic_arn: &'a str,
    pub message_id: &'a str,
    pub message: &'a str,
    pub timestamp: &'a str,
    pub region: &'a str,
    pub subject: Option<&'a str>,
    pub unsubscribe_url: &'a str,
    pub message_attributes: Option<&'a Map<String, Value>>,
}

/// Build the JSON envelope SNS posts to HTTP(S) subscribers. The
/// `Signature` field is an HMAC-SHA256 over the canonical string
/// described by AWS, base64-encoded; verifying it requires the
/// per-process key (out of band) — real subscribers should use
/// `SignatureVersion=2` once awsim gains a per-region keypair.
pub fn build_notification(inputs: &NotificationInputs<'_>, signing_key: &[u8]) -> Value {
    let canonical = canonical_string(inputs);
    let signature = sign(&canonical, signing_key);
    let mut obj = json!({
        "Type": "Notification",
        "MessageId": inputs.message_id,
        "TopicArn": inputs.topic_arn,
        "Message": inputs.message,
        "Timestamp": inputs.timestamp,
        "SignatureVersion": "1",
        "Signature": signature,
        "SigningCertURL": signing_cert_url(inputs.region),
        "UnsubscribeURL": inputs.unsubscribe_url,
    });
    if let Some(subject) = inputs.subject {
        obj["Subject"] = Value::String(subject.to_string());
    }
    if let Some(attrs) = inputs.message_attributes {
        obj["MessageAttributes"] = Value::Object(attrs.clone());
    }
    obj
}

/// Stable cert URL the simulator serves a stub PEM at. AWS uses a
/// region-scoped hostname; the simulator points subscribers back at
/// the same gateway so they don't have to bypass localhost DNS.
pub fn signing_cert_url(region: &str) -> String {
    format!("/_awsim/sns/SimpleNotificationService-{region}.pem")
}

/// Canonicalisation AWS documents for SignatureVersion=1: the fields
/// `Message`, `MessageId`, `Subject` (if present), `Timestamp`,
/// `TopicArn`, `Type` in alphabetical order, each followed by `\n`
/// after the key and after the value.
fn canonical_string(inputs: &NotificationInputs<'_>) -> String {
    let mut out = String::new();
    push_field(&mut out, "Message", inputs.message);
    push_field(&mut out, "MessageId", inputs.message_id);
    if let Some(subject) = inputs.subject {
        push_field(&mut out, "Subject", subject);
    }
    push_field(&mut out, "Timestamp", inputs.timestamp);
    push_field(&mut out, "TopicArn", inputs.topic_arn);
    push_field(&mut out, "Type", "Notification");
    out
}

fn push_field(buf: &mut String, key: &str, value: &str) {
    buf.push_str(key);
    buf.push('\n');
    buf.push_str(value);
    buf.push('\n');
}

fn sign(canonical: &str, signing_key: &[u8]) -> String {
    let mut mac =
        HmacSha256::new_from_slice(signing_key).expect("HMAC-SHA256 accepts any key length");
    mac.update(canonical.as_bytes());
    BASE64.encode(mac.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture<'a>() -> NotificationInputs<'a> {
        NotificationInputs {
            topic_arn: "arn:aws:sns:us-east-1:000000000000:demo",
            message_id: "11111111-1111-1111-1111-111111111111",
            message: "hello",
            timestamp: "1970-01-01T00:00:00.000Z",
            region: "us-east-1",
            subject: Some("greeting"),
            unsubscribe_url: "https://example.com/unsubscribe?token=abc",
            message_attributes: None,
        }
    }

    #[test]
    fn envelope_carries_required_signature_fields() {
        let env = build_notification(&fixture(), b"test-key");
        assert_eq!(env["Type"], "Notification");
        assert_eq!(env["SignatureVersion"], "1");
        assert!(env["Signature"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(
            env["SigningCertURL"]
                .as_str()
                .unwrap()
                .contains("SimpleNotificationService"),
            "{env}"
        );
        assert_eq!(
            env["UnsubscribeURL"],
            "https://example.com/unsubscribe?token=abc"
        );
    }

    #[test]
    fn signature_is_deterministic_for_identical_inputs() {
        let key = b"deterministic-key";
        let one = build_notification(&fixture(), key);
        let two = build_notification(&fixture(), key);
        assert_eq!(one["Signature"], two["Signature"]);
    }

    #[test]
    fn signature_changes_when_message_changes() {
        let key = b"deterministic-key";
        let baseline = build_notification(&fixture(), key);
        let mut altered = fixture();
        altered.message = "different";
        let other = build_notification(&altered, key);
        assert_ne!(baseline["Signature"], other["Signature"]);
    }

    #[test]
    fn signature_changes_when_signing_key_changes() {
        let baseline = build_notification(&fixture(), b"key-a");
        let other = build_notification(&fixture(), b"key-b");
        assert_ne!(baseline["Signature"], other["Signature"]);
    }

    #[test]
    fn omits_subject_when_absent() {
        let mut inputs = fixture();
        inputs.subject = None;
        let env = build_notification(&inputs, b"k");
        assert!(env.get("Subject").is_none(), "{env}");
    }

    #[test]
    fn canonical_string_orders_fields_alphabetically() {
        let canonical = canonical_string(&fixture());
        // Expected canonicalisation per AWS docs.
        let expected = concat!(
            "Message\nhello\n",
            "MessageId\n11111111-1111-1111-1111-111111111111\n",
            "Subject\ngreeting\n",
            "Timestamp\n1970-01-01T00:00:00.000Z\n",
            "TopicArn\narn:aws:sns:us-east-1:000000000000:demo\n",
            "Type\nNotification\n",
        );
        assert_eq!(canonical, expected);
    }

    #[test]
    fn signing_cert_url_is_region_scoped() {
        assert!(signing_cert_url("eu-west-1").contains("eu-west-1"));
        assert_ne!(signing_cert_url("us-east-1"), signing_cert_url("eu-west-1"));
    }

    #[test]
    fn message_attributes_round_trip_into_envelope() {
        let mut attrs = Map::new();
        attrs.insert("k".into(), json!({"Type": "String", "Value": "v"}));
        let mut inputs = fixture();
        inputs.message_attributes = Some(&attrs);
        let env = build_notification(&inputs, b"k");
        assert_eq!(env["MessageAttributes"]["k"]["Value"], "v");
    }
}
