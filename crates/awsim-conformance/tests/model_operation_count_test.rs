//! Coverage denominators must match what the Smithy models actually
//! declare.
//!
//! The parser previously read only the service shape's `operations`
//! array, ignoring operations attached to `resources`. ECS reported a
//! denominator of 13 against a model containing 77 operation shapes, so
//! every percentage derived from it was wrong, and the totals built on
//! top of them were wrong too.

use awsim_conformance::smithy;
use std::path::PathBuf;

fn models_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("models")
}

/// Count operation shapes directly out of the JSON, independent of the
/// parser under test, so this is a real cross-check rather than the
/// parser agreeing with itself.
fn operation_shapes_in_model(service: &str) -> usize {
    let path = models_dir().join(format!("{service}.json"));
    let content =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let json: serde_json::Value = serde_json::from_str(&content).expect("parse model");
    json["shapes"]
        .as_object()
        .expect("shapes")
        .values()
        .filter(|shape| shape["type"].as_str() == Some("operation"))
        .count()
}

/// Services that attach operations to resource shapes. These are the
/// ones the old service-only walk undercounted.
#[test]
fn resource_attached_operations_are_counted() {
    for service in ["ecs", "lambda", "bedrock"] {
        let path = models_dir().join(format!("{service}.json"));
        if !path.exists() {
            continue;
        }
        let model = smithy::parse_model(&path);
        let parsed = model.operations().len();
        let declared = operation_shapes_in_model(service);

        assert!(
            parsed >= declared,
            "{service}: parser found {parsed} operations but the model \
             declares {declared} operation shapes. Operations reachable \
             only through resource shapes are being missed."
        );
    }
}

/// Every model should parse to a non-empty operation list. A silent zero
/// would make a service look fully covered at 0/0.
#[test]
fn every_model_yields_operations() {
    let dir = models_dir();
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).expect("read models dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let model = smithy::parse_model(&path);
        assert!(
            !model.operations().is_empty(),
            "{} parsed to zero operations, which would report as 0/0 and \
             read as full coverage",
            path.display()
        );
        checked += 1;
    }
    assert!(checked > 0, "no models were checked");
}

/// Operation names must be unique. Duplicates would inflate the
/// denominator and let `passed` exceed `implemented`.
#[test]
fn operation_names_are_unique_per_model() {
    for service in ["ecs", "lambda", "s3", "dynamodb"] {
        let path = models_dir().join(format!("{service}.json"));
        if !path.exists() {
            continue;
        }
        let model = smithy::parse_model(&path);
        let names: Vec<&str> = model.operations().iter().map(|o| o.name.as_str()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(
            names.len(),
            unique.len(),
            "{service} has duplicate operation names in its parsed model"
        );
    }
}
