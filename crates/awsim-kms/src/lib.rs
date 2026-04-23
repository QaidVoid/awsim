mod error;
mod operations;
mod state;
mod util;

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
            "UpdateKeyDescription" => {
                operations::keys::update_key_description(&state, &input, ctx)
            }

            // Aliases
            "CreateAlias" => operations::aliases::create_alias(&state, &input, ctx),
            "DeleteAlias" => operations::aliases::delete_alias(&state, &input, ctx),
            "ListAliases" => operations::aliases::list_aliases(&state, &input, ctx),

            // Cryptographic operations
            "Encrypt" => operations::crypto::encrypt(&state, &input, ctx),
            "Decrypt" => operations::crypto::decrypt(&state, &input, ctx),
            "GenerateDataKey" => operations::crypto::generate_data_key(&state, &input, ctx),
            "GenerateDataKeyWithoutPlaintext" => {
                operations::crypto::generate_data_key_without_plaintext(&state, &input, ctx)
            }
            "ReEncrypt" => operations::crypto::re_encrypt(&state, &input, ctx),
            "GenerateRandom" => operations::crypto::generate_random(&state, &input, ctx),

            // Key rotation
            "GetKeyRotationStatus" => {
                operations::rotation::get_key_rotation_status(&state, &input, ctx)
            }
            "EnableKeyRotation" => {
                operations::rotation::enable_key_rotation(&state, &input, ctx)
            }
            "DisableKeyRotation" => {
                operations::rotation::disable_key_rotation(&state, &input, ctx)
            }

            // Grants
            "CreateGrant" => operations::grants::create_grant(&state, &input, ctx),
            "ListGrants" => operations::grants::list_grants(&state, &input, ctx),
            "ListRetirableGrants" => {
                operations::grants::list_retirable_grants(&state, &input, ctx)
            }
            "RetireGrant" => operations::grants::retire_grant(&state, &input, ctx),
            "RevokeGrant" => operations::grants::revoke_grant(&state, &input, ctx),

            // Key policies
            "GetKeyPolicy" => operations::policies::get_key_policy(&state, &input, ctx),
            "PutKeyPolicy" => operations::policies::put_key_policy(&state, &input, ctx),
            "ListKeyPolicies" => operations::policies::list_key_policies(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
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
        fn noop_clone(_: *const ()) -> RawWaker { noop_raw_waker() }
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
        let described = block_on(svc.handle("DescribeKey", json!({ "KeyId": key_id }), &ctx))
            .unwrap();
        assert_eq!(
            described["KeyMetadata"]["KeyId"].as_str().unwrap(),
            key_id
        );
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
        let described = block_on(svc.handle("DescribeKey", json!({ "KeyId": key_id }), &ctx))
            .unwrap();
        assert_eq!(
            described["KeyMetadata"]["KeyState"].as_str().unwrap(),
            "Disabled"
        );

        block_on(svc.handle("EnableKey", json!({ "KeyId": key_id }), &ctx)).unwrap();
        let described2 = block_on(svc.handle("DescribeKey", json!({ "KeyId": key_id }), &ctx))
            .unwrap();
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
        assert!(result["DeletionDate"].as_f64().is_some() || result["DeletionDate"].as_str().is_some());
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

        block_on(svc.handle(
            "DeleteAlias",
            json!({ "AliasName": "alias/my-key" }),
            &ctx,
        ))
        .unwrap();

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

        let decrypted = block_on(svc.handle(
            "Decrypt",
            json!({ "CiphertextBlob": ciphertext }),
            &ctx,
        ))
        .unwrap();
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
        let decrypted = block_on(svc.handle(
            "Decrypt",
            json!({ "CiphertextBlob": new_ciphertext }),
            &ctx,
        ))
        .unwrap();
        let decrypted_bytes = BASE64.decode(decrypted["Plaintext"].as_str().unwrap()).unwrap();
        assert_eq!(decrypted_bytes, b"reencrypt me");
    }

    #[test]
    fn test_unknown_operation() {
        let svc = KmsService::new();
        let ctx = ctx();
        let err = block_on(svc.handle("FooBar", json!({}), &ctx)).unwrap_err();
        assert_eq!(err.code, "UnknownOperationException");
    }
}
