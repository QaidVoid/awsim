//! Secrets Manager must survive a restart. The README lists secrets as
//! covered by `--data-dir` persistence, and before this it silently was
//! not: a secret created, then a restart, then ListSecrets returned an
//! empty list.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_secretsmanager::SecretsManagerService;
use serde_json::json;

fn ctx() -> RequestContext {
    RequestContext::new("secretsmanager", "us-east-1")
}

/// Round-trip a service through snapshot and restore, the way the data
/// directory and named snapshots both do.
async fn round_trip(svc: &SecretsManagerService) -> SecretsManagerService {
    let bytes = svc.snapshot().expect("service should produce a snapshot");
    let restored = SecretsManagerService::new();
    restored.restore(&bytes).expect("restore should succeed");
    restored
}

#[tokio::test]
async fn secret_survives_a_snapshot_round_trip() {
    let svc = SecretsManagerService::new();
    svc.handle(
        "CreateSecret",
        json!({ "Name": "db-password", "SecretString": "s3cr3t" }),
        &ctx(),
    )
    .await
    .expect("CreateSecret");

    let restored = round_trip(&svc).await;

    let got = restored
        .handle(
            "GetSecretValue",
            json!({ "SecretId": "db-password" }),
            &ctx(),
        )
        .await
        .expect("secret should exist after restore");
    assert_eq!(got["SecretString"], "s3cr3t");
}

#[tokio::test]
async fn list_secrets_is_populated_after_restore() {
    let svc = SecretsManagerService::new();
    for name in ["one", "two", "three"] {
        svc.handle(
            "CreateSecret",
            json!({ "Name": name, "SecretString": "v" }),
            &ctx(),
        )
        .await
        .expect("CreateSecret");
    }

    let restored = round_trip(&svc).await;
    let listed = restored
        .handle("ListSecrets", json!({}), &ctx())
        .await
        .expect("ListSecrets");

    let names: Vec<String> = listed["SecretList"]
        .as_array()
        .expect("SecretList")
        .iter()
        .map(|s| s["Name"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(names.len(), 3, "expected all secrets back, got {names:?}");
    for expected in ["one", "two", "three"] {
        assert!(names.iter().any(|n| n == expected), "missing {expected}");
    }
}

#[tokio::test]
async fn secret_versions_and_metadata_survive() {
    let svc = SecretsManagerService::new();
    svc.handle(
        "CreateSecret",
        json!({
            "Name": "versioned",
            "SecretString": "v1",
            "Description": "a description",
        }),
        &ctx(),
    )
    .await
    .expect("CreateSecret");
    svc.handle(
        "PutSecretValue",
        json!({ "SecretId": "versioned", "SecretString": "v2" }),
        &ctx(),
    )
    .await
    .expect("PutSecretValue");

    let restored = round_trip(&svc).await;

    let current = restored
        .handle("GetSecretValue", json!({ "SecretId": "versioned" }), &ctx())
        .await
        .expect("GetSecretValue");
    assert_eq!(current["SecretString"], "v2", "current version lost");

    let described = restored
        .handle("DescribeSecret", json!({ "SecretId": "versioned" }), &ctx())
        .await
        .expect("DescribeSecret");
    assert_eq!(described["Description"], "a description");
}

#[tokio::test]
async fn restore_replaces_rather_than_merges() {
    let svc = SecretsManagerService::new();
    svc.handle(
        "CreateSecret",
        json!({ "Name": "kept", "SecretString": "v" }),
        &ctx(),
    )
    .await
    .expect("CreateSecret");
    let bytes = svc.snapshot().expect("snapshot");

    // A service holding different state must end up matching the
    // snapshot exactly, not a union of the two.
    let other = SecretsManagerService::new();
    other
        .handle(
            "CreateSecret",
            json!({ "Name": "stale", "SecretString": "v" }),
            &ctx(),
        )
        .await
        .expect("CreateSecret");
    other.restore(&bytes).expect("restore");

    let listed = other
        .handle("ListSecrets", json!({}), &ctx())
        .await
        .expect("ListSecrets");
    let names: Vec<String> = listed["SecretList"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["Name"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(names, vec!["kept".to_string()], "got {names:?}");
}

/// The default trait impl now reports non-support, so a service that
/// genuinely implements restore must not be mistaken for one that does not.
#[tokio::test]
async fn restore_is_actually_implemented() {
    let svc = SecretsManagerService::new();
    let bytes = svc.snapshot().expect("snapshot");
    let res = svc.restore(&bytes);
    assert!(res.is_ok(), "restore should be implemented: {res:?}");
}
