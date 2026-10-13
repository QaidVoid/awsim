mod asymmetric;
pub mod authz;
pub mod error;
mod operations;
pub mod state;
mod util;

pub use authz::{KmsGrantLookup, KmsKeyResolver, KmsResourcePolicyLookup};

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler, arn};
use serde_json::Value;
use tracing::debug;

use state::KmsState;

/// The KMS service handler.
pub struct KmsService {
    store: AccountRegionStore<KmsState>,
}

impl KmsService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<KmsState> {
        self.store.clone()
    }
}

impl Default for KmsService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for KmsService {
    fn service_name(&self) -> &str {
        "kms"
    }

    fn signing_name(&self) -> &str {
        "kms"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_1
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "KMS request");
        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            // Key lifecycle
            "CreateKey" => operations::keys::create_key(&state, &input, ctx),
            "DescribeKey" => operations::keys::describe_key(&state, &input, ctx),
            "ListKeys" => operations::keys::list_keys(&state, &input, ctx),
            "EnableKey" => operations::keys::enable_key(&state, &input, ctx),
            "DisableKey" => operations::keys::disable_key(&state, &input, ctx),
            "ScheduleKeyDeletion" => operations::keys::schedule_key_deletion(&state, &input, ctx),
            "CancelKeyDeletion" => operations::keys::cancel_key_deletion(&state, &input, ctx),
            "UpdateKeyDescription" => operations::keys::update_key_description(&state, &input, ctx),

            // Aliases
            "CreateAlias" => operations::aliases::create_alias(&state, &input, ctx),
            "DeleteAlias" => operations::aliases::delete_alias(&state, &input, ctx),
            "ListAliases" => operations::aliases::list_aliases(&state, &input, ctx),
            "UpdateAlias" => operations::aliases::update_alias(&state, &input, ctx),

            // Cryptographic operations
            "Encrypt" => operations::crypto::encrypt(&state, &input, ctx),
            "Decrypt" => operations::crypto::decrypt(&state, &input, ctx),
            "GenerateDataKey" => operations::crypto::generate_data_key(&state, &input, ctx),
            "GenerateDataKeyWithoutPlaintext" => {
                operations::crypto::generate_data_key_without_plaintext(&state, &input, ctx)
            }
            "ReEncrypt" => operations::crypto::re_encrypt(&state, &input, ctx),
            "GenerateRandom" => operations::crypto::generate_random(&state, &input, ctx),
            "GenerateDataKeyPair" => {
                operations::signing::generate_data_key_pair(&state, &input, ctx)
            }
            "GenerateDataKeyPairWithoutPlaintext" => {
                operations::signing::generate_data_key_pair_without_plaintext(&state, &input, ctx)
            }

            // Asymmetric / signing operations
            "Sign" => operations::signing::sign(&state, &input, ctx),
            "Verify" => operations::signing::verify(&state, &input, ctx),
            "GetPublicKey" => operations::signing::get_public_key(&state, &input, ctx),
            "DeriveSharedSecret" => operations::signing::derive_shared_secret(&state, &input, ctx),

            // Key rotation
            "GetKeyRotationStatus" => {
                operations::rotation::get_key_rotation_status(&state, &input, ctx)
            }
            "EnableKeyRotation" => operations::rotation::enable_key_rotation(&state, &input, ctx),
            "DisableKeyRotation" => operations::rotation::disable_key_rotation(&state, &input, ctx),
            "RotateKeyOnDemand" => operations::rotation::rotate_key_on_demand(&state, &input, ctx),
            "ListKeyRotations" => operations::rotation::list_key_rotations(&state, &input, ctx),

            // MAC
            "GenerateMac" => operations::mac::generate_mac(&state, &input, ctx),
            "VerifyMac" => operations::mac::verify_mac(&state, &input, ctx),

            // Grants
            "CreateGrant" => operations::grants::create_grant(&state, &input, ctx),
            "ListGrants" => operations::grants::list_grants(&state, &input, ctx),
            "ListRetirableGrants" => operations::grants::list_retirable_grants(&state, &input, ctx),
            "RetireGrant" => operations::grants::retire_grant(&state, &input, ctx),
            "RevokeGrant" => operations::grants::revoke_grant(&state, &input, ctx),

            // Key policies
            "GetKeyPolicy" => operations::policies::get_key_policy(&state, &input, ctx),
            "PutKeyPolicy" => operations::policies::put_key_policy(&state, &input, ctx),
            "ListKeyPolicies" => operations::policies::list_key_policies(&state, &input, ctx),

            // Custom key stores
            "CreateCustomKeyStore" => {
                operations::keystores::create_custom_key_store(&state, &input, ctx)
            }
            "DescribeCustomKeyStores" => {
                operations::keystores::describe_custom_key_stores(&state, &input, ctx)
            }
            "DeleteCustomKeyStore" => {
                operations::keystores::delete_custom_key_store(&state, &input, ctx)
            }
            "ConnectCustomKeyStore" => {
                operations::keystores::connect_custom_key_store(&state, &input, ctx)
            }
            "DisconnectCustomKeyStore" => {
                operations::keystores::disconnect_custom_key_store(&state, &input, ctx)
            }
            "UpdateCustomKeyStore" => {
                operations::keystores::update_custom_key_store(&state, &input, ctx)
            }

            // Multi-region replication
            "ReplicateKey" => operations::keystores::replicate_key(&state, &input, ctx),
            "UpdatePrimaryRegion" => {
                operations::keystores::update_primary_region(&state, &input, ctx)
            }

            // Key import
            "GetParametersForImport" => {
                operations::import::get_parameters_for_import(&state, &input, ctx)
            }
            "ImportKeyMaterial" => operations::import::import_key_material(&state, &input, ctx),
            "DeleteImportedKeyMaterial" => {
                operations::import::delete_imported_key_material(&state, &input, ctx)
            }

            // Resource tagging
            "TagResource" => operations::tagging::tag_resource(&state, &input, ctx),
            "UntagResource" => operations::tagging::untag_resource(&state, &input, ctx),
            "ListResourceTags" => operations::tagging::list_resource_tags(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn iam_action(&self, operation: &str) -> Option<String> {
        match operation {
            "CreateKey"
            | "DescribeKey"
            | "ListKeys"
            | "EnableKey"
            | "DisableKey"
            | "ScheduleKeyDeletion"
            | "CancelKeyDeletion"
            | "UpdateKeyDescription"
            | "CreateAlias"
            | "DeleteAlias"
            | "ListAliases"
            | "UpdateAlias"
            | "Encrypt"
            | "Decrypt"
            | "GenerateDataKey"
            | "GenerateDataKeyWithoutPlaintext"
            | "ReEncrypt"
            | "ReEncryptFrom"
            | "ReEncryptTo"
            | "GenerateRandom"
            | "GenerateDataKeyPair"
            | "GenerateDataKeyPairWithoutPlaintext"
            | "Sign"
            | "Verify"
            | "GetPublicKey"
            | "DeriveSharedSecret"
            | "GetKeyRotationStatus"
            | "EnableKeyRotation"
            | "DisableKeyRotation"
            | "RotateKeyOnDemand"
            | "ListKeyRotations"
            | "GenerateMac"
            | "VerifyMac"
            | "CreateGrant"
            | "ListGrants"
            | "ListRetirableGrants"
            | "RetireGrant"
            | "RevokeGrant"
            | "GetKeyPolicy"
            | "PutKeyPolicy"
            | "ListKeyPolicies"
            | "CreateCustomKeyStore"
            | "DescribeCustomKeyStores"
            | "DeleteCustomKeyStore"
            | "ConnectCustomKeyStore"
            | "DisconnectCustomKeyStore"
            | "UpdateCustomKeyStore"
            | "ReplicateKey"
            | "UpdatePrimaryRegion"
            | "GetParametersForImport"
            | "ImportKeyMaterial"
            | "DeleteImportedKeyMaterial"
            | "TagResource"
            | "UntagResource"
            | "ListResourceTags" => Some(format!("kms:{operation}")),
            _ => None,
        }
    }

    fn iam_checks(
        &self,
        operation: &str,
        input: &Value,
        ctx: &RequestContext,
    ) -> Vec<(String, String)> {
        let Some(action) = self.iam_action(operation) else {
            return Vec::new();
        };
        let state = self.store.get(&ctx.account_id, &ctx.region);
        // AWS authorizes a key named by id, alias or ARN as the key ARN
        // the reference resolves to. References to unknown keys keep
        // their literal ARN so the handler can report NotFound.
        let key_arn = |key_ref: &str| -> String {
            let resolved = operations::keys::resolve_key_id(&state, key_ref)
                .ok()
                .and_then(|id| state.keys.get(&id).map(|k| k.arn.clone()));
            match resolved {
                Some(arn) => arn,
                None if key_ref.starts_with("arn:") => key_ref.to_string(),
                None if key_ref.starts_with("alias/") => arn::build(ctx, "kms", key_ref),
                None => arn::build(ctx, "kms", format!("key/{key_ref}")),
            }
        };
        let field = |name: &str| input.get(name).and_then(Value::as_str);
        // Decrypt and ReEncrypt may name their key only inside the ciphertext.
        let ciphertext_key = || {
            field("CiphertextBlob")
                .and_then(|blob| operations::crypto::decode_ciphertext_blob(blob).ok())
                .map(|(key_id, _)| key_arn(&key_id))
        };

        match operation {
            "ListKeys"
            | "ListAliases"
            | "ListRetirableGrants"
            | "DescribeCustomKeyStores"
            | "CreateKey"
            | "CreateCustomKeyStore"
            | "GenerateRandom" => vec![(action, "*".to_string())],
            // Alias changes need the action on the alias and on its key.
            "CreateAlias" | "UpdateAlias" | "DeleteAlias" => {
                let Some(alias) = field("AliasName") else {
                    return Vec::new();
                };
                let target = field("TargetKeyId").unwrap_or(alias);
                vec![
                    (action.clone(), arn::build(ctx, "kms", alias)),
                    (action, key_arn(target)),
                ]
            }
            "Decrypt" => field("KeyId")
                .map(key_arn)
                .or_else(ciphertext_key)
                .map(|key| vec![(action, key)])
                .unwrap_or_default(),
            "ReEncrypt" => {
                let mut checks = Vec::new();
                if let Some(source) = field("SourceKeyId").map(key_arn).or_else(ciphertext_key) {
                    checks.push(("kms:ReEncryptFrom".to_string(), source));
                }
                if let Some(destination) = field("DestinationKeyId") {
                    checks.push(("kms:ReEncryptTo".to_string(), key_arn(destination)));
                }
                checks
            }
            "RetireGrant" => {
                let by_token = || {
                    let token = field("GrantToken")?;
                    let grant = state.grants.iter().find(|g| g.grant_token == token)?;
                    Some(key_arn(&grant.key_id))
                };
                let key = field("KeyId").map(key_arn).or_else(by_token);
                vec![(action, key.unwrap_or_else(|| "*".to_string()))]
            }
            _ => field("KeyId")
                .or_else(|| field("ResourceArn"))
                .map(|key| vec![(action, key_arn(key))])
                .unwrap_or_default(),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut buckets = Vec::new();
        for ((account_id, region), state) in self.store.iter_all() {
            buckets.push(KmsBucketSnapshot {
                account_id,
                region,
                keys: state
                    .keys
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect(),
                aliases: state
                    .aliases
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect(),
                grants: state
                    .grants
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect(),
                custom_key_stores: state
                    .custom_key_stores
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect(),
                key_rotations: state
                    .key_rotations
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect(),
            });
        }
        serde_json::to_vec(&KmsSnapshot { buckets }).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snapshot: KmsSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        for bucket in snapshot.buckets {
            let state = self.store.get(&bucket.account_id, &bucket.region);
            state.keys.clear();
            state.aliases.clear();
            state.grants.clear();
            state.custom_key_stores.clear();
            state.key_rotations.clear();
            for (k, v) in bucket.keys {
                state.keys.insert(k, v);
            }
            for (k, v) in bucket.aliases {
                state.aliases.insert(k, v);
            }
            for (k, v) in bucket.grants {
                state.grants.insert(k, v);
            }
            for (k, v) in bucket.custom_key_stores {
                state.custom_key_stores.insert(k, v);
            }
            for (k, v) in bucket.key_rotations {
                state.key_rotations.insert(k, v);
            }
        }
        Ok(())
    }
}

/// One (account, region) pair's KMS state on disk.
///
/// Key material is included: a key whose bytes did not survive a restart
/// would still be listed but could no longer decrypt anything previously
/// encrypted under it, which is a worse outcome than not persisting at all.
#[derive(serde::Serialize, serde::Deserialize)]
struct KmsBucketSnapshot {
    account_id: String,
    region: String,
    keys: Vec<(String, state::KmsKey)>,
    aliases: Vec<(String, String)>,
    grants: Vec<(String, state::KmsGrant)>,
    custom_key_stores: Vec<(String, state::KmsCustomKeyStore)>,
    key_rotations: Vec<(String, Vec<state::KeyRotationEvent>)>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct KmsSnapshot {
    buckets: Vec<KmsBucketSnapshot>,
}

#[cfg(test)]
mod tests {
    use awsim_core::{RequestContext, ServiceHandler};
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use serde_json::json;

    use super::KmsService;

    fn ctx() -> RequestContext {
        RequestContext::new("kms", "us-east-1")
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        fn noop_clone(_: *const ()) -> RawWaker {
            noop_raw_waker()
        }
        fn noop(_: *const ()) {}
        fn noop_raw_waker() -> RawWaker {
            static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(f);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => {}
            }
        }
    }

    #[test]
    fn test_create_key_symmetric() {
        let svc = KmsService::new();
        let ctx = ctx();
        let result = block_on(svc.handle(
            "CreateKey",
            json!({ "KeySpec": "SYMMETRIC_DEFAULT", "Description": "test key" }),
            &ctx,
        ))
        .unwrap();
        let meta = &result["KeyMetadata"];
        assert!(meta["KeyId"].as_str().is_some());
        assert_eq!(meta["KeyState"].as_str().unwrap(), "Enabled");
        assert_eq!(meta["KeySpec"].as_str().unwrap(), "SYMMETRIC_DEFAULT");
    }

    #[test]
    fn test_create_key_invalid_usage() {
        let svc = KmsService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateKey",
            json!({ "KeySpec": "SYMMETRIC_DEFAULT", "KeyUsage": "SIGN_VERIFY" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn test_describe_key() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();
        let described =
            block_on(svc.handle("DescribeKey", json!({ "KeyId": key_id }), &ctx)).unwrap();
        assert_eq!(described["KeyMetadata"]["KeyId"].as_str().unwrap(), key_id);
    }

    #[test]
    fn test_list_keys() {
        let svc = KmsService::new();
        let ctx = ctx();
        block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let result = block_on(svc.handle("ListKeys", json!({}), &ctx)).unwrap();
        assert_eq!(result["Keys"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_enable_disable_key() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        block_on(svc.handle("DisableKey", json!({ "KeyId": key_id }), &ctx)).unwrap();
        let described =
            block_on(svc.handle("DescribeKey", json!({ "KeyId": key_id }), &ctx)).unwrap();
        assert_eq!(
            described["KeyMetadata"]["KeyState"].as_str().unwrap(),
            "Disabled"
        );

        block_on(svc.handle("EnableKey", json!({ "KeyId": key_id }), &ctx)).unwrap();
        let described2 =
            block_on(svc.handle("DescribeKey", json!({ "KeyId": key_id }), &ctx)).unwrap();
        assert_eq!(
            described2["KeyMetadata"]["KeyState"].as_str().unwrap(),
            "Enabled"
        );
    }

    #[test]
    fn test_schedule_key_deletion() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        let result = block_on(svc.handle(
            "ScheduleKeyDeletion",
            json!({ "KeyId": key_id, "PendingWindowInDays": 7 }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["KeyId"].as_str().unwrap(), key_id);
        assert!(
            result["DeletionDate"].as_f64().is_some() || result["DeletionDate"].as_str().is_some()
        );
    }

    #[test]
    fn test_create_list_delete_alias() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        block_on(svc.handle(
            "CreateAlias",
            json!({ "AliasName": "alias/my-key", "TargetKeyId": key_id }),
            &ctx,
        ))
        .unwrap();

        let aliases = block_on(svc.handle("ListAliases", json!({}), &ctx)).unwrap();
        assert_eq!(aliases["Aliases"].as_array().unwrap().len(), 1);

        block_on(svc.handle("DeleteAlias", json!({ "AliasName": "alias/my-key" }), &ctx)).unwrap();

        let aliases2 = block_on(svc.handle("ListAliases", json!({}), &ctx)).unwrap();
        assert_eq!(aliases2["Aliases"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_encrypt_resolves_alias_arn() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();
        block_on(svc.handle(
            "CreateAlias",
            json!({ "AliasName": "alias/my-key", "TargetKeyId": key_id }),
            &ctx,
        ))
        .unwrap();

        // Alias ARN form. Must resolve through resolve_key_id even though
        // the prefix `arn:aws:kms:` is shared with key ARNs.
        let alias_arn = "arn:aws:kms:us-east-1:000000000000:alias/my-key";
        let resp = block_on(svc.handle(
            "Encrypt",
            json!({
                "KeyId": alias_arn,
                "Plaintext": BASE64.encode(b"secret"),
            }),
            &ctx,
        ))
        .unwrap();
        assert!(resp["CiphertextBlob"].as_str().is_some());
    }

    #[test]
    fn test_alias_must_start_with_alias() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        let err = block_on(svc.handle(
            "CreateAlias",
            json!({ "AliasName": "my-key", "TargetKeyId": key_id }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        let plaintext = "Hello, KMS!";
        let plaintext_b64 = BASE64.encode(plaintext.as_bytes());

        let encrypted = block_on(svc.handle(
            "Encrypt",
            json!({ "KeyId": key_id, "Plaintext": plaintext_b64 }),
            &ctx,
        ))
        .unwrap();
        let ciphertext = encrypted["CiphertextBlob"].as_str().unwrap();

        let decrypted =
            block_on(svc.handle("Decrypt", json!({ "CiphertextBlob": ciphertext }), &ctx)).unwrap();
        let decrypted_b64 = decrypted["Plaintext"].as_str().unwrap();
        let decrypted_bytes = BASE64.decode(decrypted_b64).unwrap();
        assert_eq!(decrypted_bytes, plaintext.as_bytes());
    }

    #[test]
    fn test_encrypt_decrypt_with_encryption_context() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();
        let plaintext_b64 = BASE64.encode(b"context-bound");
        let ec = json!({ "purpose": "test", "tenant": "alpha" });

        let encrypted = block_on(svc.handle(
            "Encrypt",
            json!({
                "KeyId": key_id,
                "Plaintext": plaintext_b64,
                "EncryptionContext": ec,
            }),
            &ctx,
        ))
        .unwrap();
        let ciphertext = encrypted["CiphertextBlob"].as_str().unwrap();

        // Same context -> succeeds.
        let ok = block_on(svc.handle(
            "Decrypt",
            json!({ "CiphertextBlob": ciphertext, "EncryptionContext": ec }),
            &ctx,
        ))
        .unwrap();
        let pt = BASE64.decode(ok["Plaintext"].as_str().unwrap()).unwrap();
        assert_eq!(pt, b"context-bound");

        // Different context -> InvalidCiphertextException via auth tag failure.
        let err = block_on(svc.handle(
            "Decrypt",
            json!({
                "CiphertextBlob": ciphertext,
                "EncryptionContext": { "purpose": "wrong" },
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidCiphertextException");
    }

    #[test]
    fn test_encrypt_disabled_key() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        block_on(svc.handle("DisableKey", json!({ "KeyId": key_id }), &ctx)).unwrap();

        let err = block_on(svc.handle(
            "Encrypt",
            json!({ "KeyId": key_id, "Plaintext": BASE64.encode(b"secret") }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "DisabledException");
    }

    #[test]
    fn test_generate_data_key_aes_256() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        let result = block_on(svc.handle(
            "GenerateDataKey",
            json!({ "KeyId": key_id, "KeySpec": "AES_256" }),
            &ctx,
        ))
        .unwrap();

        let pt = result["Plaintext"].as_str().unwrap();
        let decoded = BASE64.decode(pt).unwrap();
        assert_eq!(decoded.len(), 32);
        assert!(result["CiphertextBlob"].as_str().is_some());
    }

    #[test]
    fn test_generate_data_key_without_plaintext() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        let result = block_on(svc.handle(
            "GenerateDataKeyWithoutPlaintext",
            json!({ "KeyId": key_id, "KeySpec": "AES_256" }),
            &ctx,
        ))
        .unwrap();
        assert!(result["Plaintext"].is_null());
        assert!(result["CiphertextBlob"].as_str().is_some());
    }

    #[test]
    fn test_re_encrypt() {
        let svc = KmsService::new();
        let ctx = ctx();
        let k1 = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let k1_id = k1["KeyMetadata"]["KeyId"].as_str().unwrap();
        let k2 = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let k2_id = k2["KeyMetadata"]["KeyId"].as_str().unwrap();

        let plaintext_b64 = BASE64.encode(b"reencrypt me");
        let encrypted = block_on(svc.handle(
            "Encrypt",
            json!({ "KeyId": k1_id, "Plaintext": plaintext_b64 }),
            &ctx,
        ))
        .unwrap();
        let ciphertext = encrypted["CiphertextBlob"].as_str().unwrap();

        let reencrypted = block_on(svc.handle(
            "ReEncrypt",
            json!({ "CiphertextBlob": ciphertext, "DestinationKeyId": k2_id }),
            &ctx,
        ))
        .unwrap();
        let new_ciphertext = reencrypted["CiphertextBlob"].as_str().unwrap();
        assert_ne!(ciphertext, new_ciphertext);

        // Decrypt the re-encrypted blob
        let decrypted =
            block_on(svc.handle("Decrypt", json!({ "CiphertextBlob": new_ciphertext }), &ctx))
                .unwrap();
        let decrypted_bytes = BASE64
            .decode(decrypted["Plaintext"].as_str().unwrap())
            .unwrap();
        assert_eq!(decrypted_bytes, b"reencrypt me");
    }

    #[test]
    fn test_iam_checks_re_encrypt_uses_from_and_to_actions() {
        let svc = KmsService::new();
        let ctx = ctx();
        let mut arns = Vec::new();
        for _ in 0..2 {
            let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
            arns.push(created["KeyMetadata"]["Arn"].as_str().unwrap().to_string());
        }
        let encrypted = block_on(svc.handle(
            "Encrypt",
            json!({ "KeyId": arns[0], "Plaintext": BASE64.encode(b"x") }),
            &ctx,
        ))
        .unwrap();
        let checks = svc.iam_checks(
            "ReEncrypt",
            &json!({
                "CiphertextBlob": encrypted["CiphertextBlob"],
                "DestinationKeyId": arns[1],
            }),
            &ctx,
        );
        assert_eq!(
            checks,
            vec![
                ("kms:ReEncryptFrom".to_string(), arns[0].clone()),
                ("kms:ReEncryptTo".to_string(), arns[1].clone()),
            ]
        );
    }

    #[test]
    fn test_unknown_operation() {
        let svc = KmsService::new();
        let ctx = ctx();
        let err = block_on(svc.handle("FooBar", json!({}), &ctx)).unwrap_err();
        assert_eq!(err.code, "UnknownOperationException");
    }

    #[test]
    fn test_sign_and_verify() {
        let svc = KmsService::new();
        let ctx = ctx();
        // Create an asymmetric key with SIGN_VERIFY usage
        let created = block_on(svc.handle(
            "CreateKey",
            json!({ "KeySpec": "RSA_2048", "KeyUsage": "SIGN_VERIFY" }),
            &ctx,
        ))
        .unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        let message_b64 = BASE64.encode(b"hello world");

        let signed = block_on(svc.handle(
            "Sign",
            json!({
                "KeyId": key_id,
                "Message": message_b64,
                "SigningAlgorithm": "RSASSA_PSS_SHA_256",
            }),
            &ctx,
        ))
        .unwrap();
        let signature = signed["Signature"].as_str().unwrap();
        assert!(!signature.is_empty());

        let verified = block_on(svc.handle(
            "Verify",
            json!({
                "KeyId": key_id,
                "Message": message_b64,
                "Signature": signature,
                "SigningAlgorithm": "RSASSA_PSS_SHA_256",
            }),
            &ctx,
        ))
        .unwrap();
        assert!(verified["SignatureValid"].as_bool().unwrap());
    }

    #[test]
    fn test_generate_mac_rejects_non_mac_key_usage() {
        let svc = KmsService::new();
        let ctx = ctx();
        // Default CreateKey uses ENCRYPT_DECRYPT. Wrong for MAC.
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();
        let err = block_on(svc.handle(
            "GenerateMac",
            json!({ "KeyId": key_id, "Message": BASE64.encode(b"data") }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidKeyUsageException");
    }

    #[test]
    fn test_derive_shared_secret_rejects_non_key_agreement_key() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();
        let err = block_on(svc.handle(
            "DeriveSharedSecret",
            json!({
                "KeyId": key_id,
                "PublicKey": BASE64.encode(b"public"),
                "KeyAgreementAlgorithm": "ECDH",
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidKeyUsageException");
    }

    #[test]
    fn test_sign_symmetric_key_fails() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        let err = block_on(svc.handle(
            "Sign",
            json!({
                "KeyId": key_id,
                "Message": BASE64.encode(b"data"),
                "SigningAlgorithm": "RSASSA_PSS_SHA_256",
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidKeyUsageException");
    }

    #[test]
    fn test_get_public_key() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle(
            "CreateKey",
            json!({ "KeySpec": "RSA_2048", "KeyUsage": "SIGN_VERIFY" }),
            &ctx,
        ))
        .unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        let result =
            block_on(svc.handle("GetPublicKey", json!({ "KeyId": key_id }), &ctx)).unwrap();
        assert!(result["PublicKey"].as_str().is_some());
        assert_eq!(result["KeySpec"].as_str().unwrap(), "RSA_2048");
    }

    #[test]
    fn test_get_public_key_symmetric_fails() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        let err =
            block_on(svc.handle("GetPublicKey", json!({ "KeyId": key_id }), &ctx)).unwrap_err();
        assert_eq!(err.code, "UnsupportedOperationException");
    }

    fn create_signing_key(svc: &KmsService, ctx: &RequestContext, key_spec: &str) -> String {
        let created = block_on(svc.handle(
            "CreateKey",
            json!({ "KeySpec": key_spec, "KeyUsage": "SIGN_VERIFY" }),
            ctx,
        ))
        .unwrap();
        created["KeyMetadata"]["KeyId"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn public_key_der(svc: &KmsService, ctx: &RequestContext, key_id: &str) -> Vec<u8> {
        let result = block_on(svc.handle("GetPublicKey", json!({ "KeyId": key_id }), ctx)).unwrap();
        BASE64
            .decode(result["PublicKey"].as_str().unwrap())
            .unwrap()
    }

    fn kms_sign(
        svc: &KmsService,
        ctx: &RequestContext,
        key_id: &str,
        algorithm: &str,
        message: &[u8],
        message_type: &str,
    ) -> Vec<u8> {
        let signed = block_on(svc.handle(
            "Sign",
            json!({
                "KeyId": key_id,
                "Message": BASE64.encode(message),
                "MessageType": message_type,
                "SigningAlgorithm": algorithm,
            }),
            ctx,
        ))
        .unwrap();
        BASE64
            .decode(signed["Signature"].as_str().unwrap())
            .unwrap()
    }

    #[test]
    fn test_get_public_key_ecc_returns_spki() {
        let svc = KmsService::new();
        let ctx = ctx();
        for (spec, algorithm, der_len) in [
            ("ECC_NIST_P256", "ECDSA_SHA_256", 91),
            ("ECC_NIST_P384", "ECDSA_SHA_384", 120),
            ("ECC_NIST_P521", "ECDSA_SHA_512", 158),
            ("ECC_SECG_P256K1", "ECDSA_SHA_256", 88),
        ] {
            let key_id = create_signing_key(&svc, &ctx, spec);
            let result =
                block_on(svc.handle("GetPublicKey", json!({ "KeyId": key_id }), &ctx)).unwrap();
            assert_eq!(result["KeySpec"], spec);
            assert_eq!(result["CustomerMasterKeySpec"], spec);
            assert_eq!(result["SigningAlgorithms"], json!([algorithm]));
            assert!(result.get("EncryptionAlgorithms").is_none());
            let der = BASE64
                .decode(result["PublicKey"].as_str().unwrap())
                .unwrap();
            assert_eq!(der.len(), der_len, "{spec} SPKI length");

            let described =
                block_on(svc.handle("DescribeKey", json!({ "KeyId": key_id }), &ctx)).unwrap();
            assert_eq!(
                described["KeyMetadata"]["SigningAlgorithms"],
                json!([algorithm])
            );
        }
    }

    #[test]
    fn test_sign_ecc_returns_der_signature_that_verifies() {
        use p256::ecdsa::signature::Verifier;
        use p256::pkcs8::DecodePublicKey;

        let svc = KmsService::new();
        let ctx = ctx();
        let key_id = create_signing_key(&svc, &ctx, "ECC_NIST_P256");
        let public_key =
            p256::PublicKey::from_public_key_der(&public_key_der(&svc, &ctx, &key_id)).unwrap();
        let verifier = p256::ecdsa::VerifyingKey::from(public_key);

        let signature = kms_sign(&svc, &ctx, &key_id, "ECDSA_SHA_256", b"hello", "RAW");
        assert_eq!(signature[0], 0x30, "DER SEQUENCE");
        assert!((8..=72).contains(&signature.len()));
        let parsed = p256::ecdsa::Signature::from_der(&signature).unwrap();
        verifier.verify(b"hello", &parsed).unwrap();

        let verified = block_on(svc.handle(
            "Verify",
            json!({
                "KeyId": key_id,
                "Message": BASE64.encode(b"hello"),
                "Signature": BASE64.encode(&signature),
                "SigningAlgorithm": "ECDSA_SHA_256",
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(verified["SignatureValid"], true);

        let err = block_on(svc.handle(
            "Verify",
            json!({
                "KeyId": key_id,
                "Message": BASE64.encode(b"tampered"),
                "Signature": BASE64.encode(&signature),
                "SigningAlgorithm": "ECDSA_SHA_256",
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "KMSInvalidSignatureException");
    }

    #[test]
    fn test_sign_digest_message_type_on_every_curve() {
        use sha2::Digest;

        let svc = KmsService::new();
        let ctx = ctx();
        for (spec, algorithm, digest) in [
            (
                "ECC_NIST_P256",
                "ECDSA_SHA_256",
                sha2::Sha256::digest(b"m").to_vec(),
            ),
            (
                "ECC_NIST_P384",
                "ECDSA_SHA_384",
                sha2::Sha384::digest(b"m").to_vec(),
            ),
            (
                "ECC_NIST_P521",
                "ECDSA_SHA_512",
                sha2::Sha512::digest(b"m").to_vec(),
            ),
            (
                "ECC_SECG_P256K1",
                "ECDSA_SHA_256",
                sha2::Sha256::digest(b"m").to_vec(),
            ),
        ] {
            let key_id = create_signing_key(&svc, &ctx, spec);
            let signature = kms_sign(&svc, &ctx, &key_id, algorithm, &digest, "DIGEST");
            assert_eq!(signature[0], 0x30, "{spec} DER SEQUENCE");

            // A DIGEST signature verifies against the RAW message it hashes.
            let verified = block_on(svc.handle(
                "Verify",
                json!({
                    "KeyId": key_id,
                    "Message": BASE64.encode(b"m"),
                    "MessageType": "RAW",
                    "Signature": BASE64.encode(&signature),
                    "SigningAlgorithm": algorithm,
                }),
                &ctx,
            ))
            .unwrap();
            assert_eq!(verified["SignatureValid"], true, "{spec}");

            let err = block_on(svc.handle(
                "Sign",
                json!({
                    "KeyId": key_id,
                    "Message": BASE64.encode(&digest[1..]),
                    "MessageType": "DIGEST",
                    "SigningAlgorithm": algorithm,
                }),
                &ctx,
            ))
            .unwrap_err();
            assert_eq!(err.code, "ValidationException", "{spec}");
        }
    }

    #[test]
    fn test_sign_rsa_verifies_with_public_key() {
        use rsa::pkcs8::DecodePublicKey;
        use sha2::Digest;

        let svc = KmsService::new();
        let ctx = ctx();
        let key_id = create_signing_key(&svc, &ctx, "RSA_2048");
        let public_key =
            rsa::RsaPublicKey::from_public_key_der(&public_key_der(&svc, &ctx, &key_id)).unwrap();
        let digest = sha2::Sha256::digest(b"hello");

        let signature = kms_sign(
            &svc,
            &ctx,
            &key_id,
            "RSASSA_PKCS1_V1_5_SHA_256",
            b"hello",
            "RAW",
        );
        assert_eq!(signature.len(), 256);
        public_key
            .verify(
                rsa::Pkcs1v15Sign::new::<sha2::Sha256>(),
                &digest,
                &signature,
            )
            .unwrap();

        let signature = kms_sign(&svc, &ctx, &key_id, "RSASSA_PSS_SHA_256", b"hello", "RAW");
        public_key
            .verify(rsa::Pss::new::<sha2::Sha256>(), &digest, &signature)
            .unwrap();
    }

    #[test]
    fn test_generate_data_key_pair() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        let result = block_on(svc.handle(
            "GenerateDataKeyPair",
            json!({ "KeyId": key_id, "KeyPairSpec": "RSA_2048" }),
            &ctx,
        ))
        .unwrap();
        assert!(result["PublicKey"].as_str().is_some());
        assert!(result["PrivateKeyPlaintext"].as_str().is_some());
        assert!(result["PrivateKeyCiphertextBlob"].as_str().is_some());
    }

    #[test]
    fn test_generate_data_key_pair_without_plaintext() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        let result = block_on(svc.handle(
            "GenerateDataKeyPairWithoutPlaintext",
            json!({ "KeyId": key_id, "KeyPairSpec": "RSA_2048" }),
            &ctx,
        ))
        .unwrap();
        assert!(result["PublicKey"].as_str().is_some());
        assert!(result["PrivateKeyPlaintext"].is_null());
        assert!(result["PrivateKeyCiphertextBlob"].as_str().is_some());
    }

    #[test]
    fn test_tag_resource_and_list() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        block_on(svc.handle(
            "TagResource",
            json!({
                "KeyId": key_id,
                "Tags": [
                    { "TagKey": "env", "TagValue": "prod" },
                    { "TagKey": "team", "TagValue": "infra" }
                ]
            }),
            &ctx,
        ))
        .unwrap();

        let tags =
            block_on(svc.handle("ListResourceTags", json!({ "KeyId": key_id }), &ctx)).unwrap();
        assert_eq!(tags["Tags"].as_array().unwrap().len(), 2);

        block_on(svc.handle(
            "UntagResource",
            json!({ "KeyId": key_id, "TagKeys": ["env"] }),
            &ctx,
        ))
        .unwrap();

        let tags2 =
            block_on(svc.handle("ListResourceTags", json!({ "KeyId": key_id }), &ctx)).unwrap();
        assert_eq!(tags2["Tags"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_custom_key_store_lifecycle() {
        let svc = KmsService::new();
        let ctx = ctx();

        let created = block_on(svc.handle(
            "CreateCustomKeyStore",
            json!({ "CustomKeyStoreName": "my-store" }),
            &ctx,
        ))
        .unwrap();
        let store_id = created["CustomKeyStoreId"].as_str().unwrap();
        assert!(store_id.starts_with("cks-"));

        let listed = block_on(svc.handle("DescribeCustomKeyStores", json!({}), &ctx)).unwrap();
        assert_eq!(listed["CustomKeyStores"].as_array().unwrap().len(), 1);

        block_on(svc.handle(
            "DeleteCustomKeyStore",
            json!({ "CustomKeyStoreId": store_id }),
            &ctx,
        ))
        .unwrap();

        let listed2 = block_on(svc.handle("DescribeCustomKeyStores", json!({}), &ctx)).unwrap();
        assert_eq!(listed2["CustomKeyStores"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_get_parameters_for_import() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        let result = block_on(svc.handle(
            "GetParametersForImport",
            json!({
                "KeyId": key_id,
                "WrappingAlgorithm": "RSAES_OAEP_SHA_256",
                "WrappingKeySpec": "RSA_2048"
            }),
            &ctx,
        ))
        .unwrap();
        assert!(result["PublicKey"].as_str().is_some());
        assert!(result["ImportToken"].as_str().is_some());
    }

    #[test]
    fn test_delete_imported_key_material_without_import_returns_kms_invalid_state() {
        let svc = KmsService::new();
        let ctx = ctx();
        // Default origin is AWS_KMS. There is no imported material to delete.
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        let err = block_on(svc.handle(
            "DeleteImportedKeyMaterial",
            json!({ "KeyId": key_id }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "KMSInvalidStateException");
        assert_eq!(err.status.as_u16(), 409);
    }

    #[test]
    fn test_create_key_external_origin_starts_pending_import() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateKey", json!({ "Origin": "EXTERNAL" }), &ctx)).unwrap();
        let metadata = &created["KeyMetadata"];
        assert_eq!(metadata["KeyState"], json!("PendingImport"));
        assert_eq!(metadata["Origin"], json!("EXTERNAL"));
        assert_eq!(metadata["Enabled"], json!(false));
    }

    #[test]
    fn test_import_key_material_rejects_aws_kms_origin_key() {
        let svc = KmsService::new();
        let ctx = ctx();
        // Default Origin=AWS_KMS. Must reject ImportKeyMaterial.
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();
        let err = block_on(svc.handle(
            "ImportKeyMaterial",
            json!({
                "KeyId": key_id,
                "EncryptedKeyMaterial": BASE64.encode(b"fake-material"),
                "ImportToken": BASE64.encode(b"fake-token"),
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "KMSInvalidStateException");
    }

    #[test]
    fn test_import_key_material_flips_external_key_to_enabled() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateKey", json!({ "Origin": "EXTERNAL" }), &ctx)).unwrap();
        let key_id = created["KeyMetadata"]["KeyId"].as_str().unwrap();

        block_on(svc.handle(
            "ImportKeyMaterial",
            json!({
                "KeyId": key_id,
                "EncryptedKeyMaterial": BASE64.encode(b"fake-material"),
                "ImportToken": BASE64.encode(b"fake-token"),
            }),
            &ctx,
        ))
        .unwrap();

        let described =
            block_on(svc.handle("DescribeKey", json!({ "KeyId": key_id }), &ctx)).unwrap();
        assert_eq!(described["KeyMetadata"]["KeyState"], json!("Enabled"));
        assert_eq!(described["KeyMetadata"]["Origin"], json!("EXTERNAL"));
    }

    #[test]
    fn test_list_keys_paginates() {
        let svc = KmsService::new();
        let ctx = ctx();
        for _ in 0..3 {
            block_on(svc.handle("CreateKey", json!({ "KeySpec": "SYMMETRIC_DEFAULT" }), &ctx))
                .unwrap();
        }

        let mut seen: Vec<String> = Vec::new();
        let mut marker: Option<String> = None;
        loop {
            let mut input = json!({ "Limit": 2 });
            if let Some(m) = &marker {
                input["Marker"] = json!(m);
            }
            let page = block_on(svc.handle("ListKeys", input, &ctx)).unwrap();
            for key in page["Keys"].as_array().unwrap() {
                seen.push(key["KeyId"].as_str().unwrap().to_string());
            }
            match page["NextMarker"].as_str() {
                Some(m) => {
                    assert_eq!(page["Truncated"], json!(true));
                    marker = Some(m.to_string());
                }
                None => {
                    assert_eq!(page["Truncated"], json!(false));
                    break;
                }
            }
        }

        seen.sort();
        seen.dedup();
        assert_eq!(
            seen.len(),
            3,
            "every key returned exactly once across pages"
        );
    }
}
