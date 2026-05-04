pub mod authz;
mod error;
mod operations;
pub mod state;
mod util;

pub use authz::{KmsGrantLookup, KmsResourcePolicyLookup};

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
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

    fn iam_resource(&self, operation: &str, input: &Value, ctx: &RequestContext) -> Option<String> {
        let prefix = format!("arn:aws:kms:{}:{}", ctx.region, ctx.account_id);
        match operation {
            "ListKeys"
            | "ListAliases"
            | "ListRetirableGrants"
            | "DescribeCustomKeyStores"
            | "CreateKey"
            | "CreateCustomKeyStore"
            | "GenerateRandom" => Some("*".to_string()),
            "RetireGrant" => input
                .get("GrantToken")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| Some("*".to_string())),
            _ => {
                let key_id = input
                    .get("KeyId")
                    .or_else(|| input.get("ResourceArn"))
                    .and_then(|v| v.as_str())?;
                if key_id.starts_with("arn:") {
                    Some(key_id.to_string())
                } else if key_id.starts_with("alias/") {
                    Some(format!("{prefix}:{key_id}"))
                } else {
                    Some(format!("{prefix}:key/{key_id}"))
                }
            }
        }
    }
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
        // Default CreateKey uses ENCRYPT_DECRYPT — wrong for MAC.
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
        // Default origin is AWS_KMS — there is no imported material to delete.
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
    fn test_import_key_material() {
        let svc = KmsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle("CreateKey", json!({}), &ctx)).unwrap();
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

        // Verify origin is now EXTERNAL
        let described =
            block_on(svc.handle("DescribeKey", json!({ "KeyId": key_id }), &ctx)).unwrap();
        assert_eq!(
            described["KeyMetadata"]["Origin"].as_str().unwrap(),
            "EXTERNAL"
        );
    }
}
