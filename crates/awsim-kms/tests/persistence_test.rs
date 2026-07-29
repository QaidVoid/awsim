//! KMS keys must survive a restart. A key whose material did not come
//! back would still be listed but could no longer decrypt anything
//! previously encrypted under it, which is worse than not persisting.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_kms::KmsService;
use serde_json::json;

fn ctx() -> RequestContext {
    RequestContext::new("kms", "us-east-1")
}

async fn create_key(svc: &KmsService) -> String {
    let out = svc
        .handle("CreateKey", json!({ "Description": "test key" }), &ctx())
        .await
        .expect("CreateKey");
    out["KeyMetadata"]["KeyId"]
        .as_str()
        .expect("KeyId")
        .to_string()
}

fn round_trip(svc: &KmsService) -> KmsService {
    let bytes = svc.snapshot().expect("snapshot");
    let restored = KmsService::new();
    restored.restore(&bytes).expect("restore");
    restored
}

#[tokio::test]
async fn key_survives_round_trip() {
    let svc = KmsService::new();
    let key_id = create_key(&svc).await;

    let restored = round_trip(&svc);
    let described = restored
        .handle("DescribeKey", json!({ "KeyId": key_id }), &ctx())
        .await
        .expect("key should exist after restore");
    assert_eq!(described["KeyMetadata"]["Description"], "test key");
}

/// The important one: ciphertext produced before a restart must still
/// decrypt afterwards, which only holds if key material round-trips.
#[tokio::test]
async fn ciphertext_still_decrypts_after_restore() {
    let svc = KmsService::new();
    let key_id = create_key(&svc).await;

    let encrypted = svc
        .handle(
            "Encrypt",
            json!({ "KeyId": key_id, "Plaintext": "c2VjcmV0" }),
            &ctx(),
        )
        .await
        .expect("Encrypt");
    let blob = encrypted["CiphertextBlob"]
        .as_str()
        .expect("blob")
        .to_string();

    let restored = round_trip(&svc);
    let decrypted = restored
        .handle("Decrypt", json!({ "CiphertextBlob": blob }), &ctx())
        .await
        .expect("ciphertext from before the restore should still decrypt");
    assert_eq!(decrypted["Plaintext"], "c2VjcmV0");
}

#[tokio::test]
async fn aliases_survive_round_trip() {
    let svc = KmsService::new();
    let key_id = create_key(&svc).await;
    svc.handle(
        "CreateAlias",
        json!({ "AliasName": "alias/my-key", "TargetKeyId": key_id }),
        &ctx(),
    )
    .await
    .expect("CreateAlias");

    let restored = round_trip(&svc);
    let listed = restored
        .handle("ListAliases", json!({}), &ctx())
        .await
        .expect("ListAliases");
    let names: Vec<String> = listed["Aliases"]
        .as_array()
        .expect("Aliases")
        .iter()
        .map(|a| a["AliasName"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(
        names.iter().any(|n| n == "alias/my-key"),
        "alias lost, got {names:?}"
    );
}

#[tokio::test]
async fn restore_is_implemented() {
    let svc = KmsService::new();
    let bytes = svc.snapshot().expect("snapshot");
    assert!(svc.restore(&bytes).is_ok(), "restore should be implemented");
}
