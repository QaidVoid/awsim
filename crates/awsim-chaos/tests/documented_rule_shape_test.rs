//! The rule body shown in `docs/guide/chaos.md` must be postable to the
//! endpoint documented on the same page.
//!
//! It was not: `id`, `created_at` and `injection_count` are server-owned
//! but were required on the wire, because the endpoint deserialised
//! straight into the stored struct. The CLI worked (it generates an id),
//! so the HTTP API documented alongside it silently did not.

use awsim_chaos::ChaosRule;

/// Copied verbatim from the chaos guide, comments stripped.
const DOCUMENTED_BODY: &str = r#"{
  "service": { "kind": "exact", "value": "s3" },
  "operation": { "kind": "any" },
  "probability": 0.05,
  "effect": {
    "kind": "error",
    "status": 503,
    "code": "SlowDown",
    "message": "Please reduce your request rate."
  },
  "enabled": true,
  "label": "preset: flaky-s3"
}"#;

#[test]
fn documented_body_deserializes() {
    let rule: ChaosRule =
        serde_json::from_str(DOCUMENTED_BODY).expect("the documented rule body must deserialize");
    assert_eq!(rule.probability, 0.05);
    assert!(rule.enabled);
    assert_eq!(rule.label.as_deref(), Some("preset: flaky-s3"));
}

#[test]
fn server_owned_fields_are_not_required() {
    // Each of these is filled in by the create handler, so none may be
    // mandatory on the wire.
    let rule: ChaosRule = serde_json::from_str(DOCUMENTED_BODY).expect("deserialize");
    assert!(
        rule.id.is_empty(),
        "id should default rather than be required"
    );
    assert_eq!(rule.created_at, 0);
    assert_eq!(rule.injection_count, 0);
}

#[test]
fn a_supplied_id_is_still_accepted() {
    // Existing HTTP callers had to send an id, since it was the only way
    // the endpoint worked. They must keep working.
    let body = r#"{
      "id": "caller-supplied",
      "service": { "kind": "any" },
      "operation": { "kind": "any" },
      "probability": 1.0,
      "effect": { "kind": "error", "status": 500, "code": "X", "message": "y" }
    }"#;
    let rule: ChaosRule = serde_json::from_str(body).expect("deserialize");
    assert_eq!(rule.id, "caller-supplied");
}

#[test]
fn a_rule_round_trips_through_serialization() {
    let rule: ChaosRule = serde_json::from_str(DOCUMENTED_BODY).expect("deserialize");
    let encoded = serde_json::to_string(&rule).expect("serialize");
    let decoded: ChaosRule = serde_json::from_str(&encoded).expect("re-deserialize");
    assert_eq!(decoded.probability, rule.probability);
    assert_eq!(decoded.label, rule.label);
}
