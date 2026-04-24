use crate::chk;
use crate::runner::common::*;

pub async fn test_kms(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_kms::Client::new(&config);
    let mut results = Vec::new();

    // CreateKey
    let create_r = client
        .create_key()
        .description("conformance test key")
        .send()
        .await;
    let key_id = create_r
        .as_ref()
        .ok()
        .and_then(|r| r.key_metadata.as_ref())
        .map(|m| m.key_id.clone());
    results.push(chk!("CreateKey", create_r, verbose));

    // ListKeys
    results.push(chk!("ListKeys", client.list_keys().send().await, verbose));

    if let Some(ref kid) = key_id {
        // DescribeKey
        results.push(chk!(
            "DescribeKey",
            client.describe_key().key_id(kid).send().await,
            verbose
        ));

        // CreateAlias
        results.push(chk!(
            "CreateAlias",
            client
                .create_alias()
                .alias_name("alias/conformance-key")
                .target_key_id(kid)
                .send()
                .await,
            verbose
        ));

        // ListAliases
        results.push(chk!(
            "ListAliases",
            client.list_aliases().send().await,
            verbose
        ));

        // Encrypt
        let encrypt_r = client
            .encrypt()
            .key_id(kid)
            .plaintext(aws_sdk_kms::primitives::Blob::new(
                b"hello conformance".to_vec(),
            ))
            .send()
            .await;
        let ciphertext = encrypt_r
            .as_ref()
            .ok()
            .and_then(|r| r.ciphertext_blob.clone());
        results.push(chk!("Encrypt", encrypt_r, verbose));

        // Decrypt
        if let Some(ct) = ciphertext {
            results.push(chk!(
                "Decrypt",
                client.decrypt().ciphertext_blob(ct).send().await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("Decrypt".to_string()));
        }

        // GenerateDataKey
        results.push(chk!(
            "GenerateDataKey",
            client
                .generate_data_key()
                .key_id(kid)
                .key_spec(aws_sdk_kms::types::DataKeySpec::Aes256)
                .send()
                .await,
            verbose
        ));

        // GenerateDataKeyWithoutPlaintext
        results.push(chk!(
            "GenerateDataKeyWithoutPlaintext",
            client
                .generate_data_key_without_plaintext()
                .key_id(kid)
                .key_spec(aws_sdk_kms::types::DataKeySpec::Aes256)
                .send()
                .await,
            verbose
        ));

        // ReEncrypt — re-encrypt data from the same key to itself
        if let Some(ct_for_reencrypt) = {
            client
                .encrypt()
                .key_id(kid)
                .plaintext(aws_sdk_kms::primitives::Blob::new(b"reencrypt-me".to_vec()))
                .send()
                .await
                .ok()
                .and_then(|r| r.ciphertext_blob)
        } {
            results.push(chk!(
                "ReEncrypt",
                client
                    .re_encrypt()
                    .ciphertext_blob(ct_for_reencrypt)
                    .destination_key_id(kid)
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("ReEncrypt".to_string()));
        }

        // EnableKey / DisableKey
        results.push(chk!(
            "DisableKey",
            client.disable_key().key_id(kid).send().await,
            verbose
        ));
        results.push(chk!(
            "EnableKey",
            client.enable_key().key_id(kid).send().await,
            verbose
        ));

        // ScheduleKeyDeletion
        results.push(chk!(
            "ScheduleKeyDeletion",
            client
                .schedule_key_deletion()
                .key_id(kid)
                .pending_window_in_days(7)
                .send()
                .await,
            verbose
        ));

        // DeleteAlias
        results.push(chk!(
            "DeleteAlias",
            client
                .delete_alias()
                .alias_name("alias/conformance-key")
                .send()
                .await,
            verbose
        ));
    } else {
        for op in &[
            "DescribeKey",
            "CreateAlias",
            "ListAliases",
            "Encrypt",
            "Decrypt",
            "GenerateDataKey",
            "GenerateDataKeyWithoutPlaintext",
            "ReEncrypt",
            "DisableKey",
            "EnableKey",
            "ScheduleKeyDeletion",
            "DeleteAlias",
            // New ops
            "UpdateKeyDescription",
            "GetKeyRotationStatus",
            "EnableKeyRotation",
            "DisableKeyRotation",
            "CreateGrant",
            "ListGrants",
            "RetireGrant",
            "RevokeGrant",
            "GetKeyPolicy",
            "PutKeyPolicy",
            "ListKeyPolicies",
            "TagResource",
            "UntagResource",
            "ListResourceTags",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    // GenerateRandom — not tied to a key
    results.push(chk!(
        "GenerateRandom",
        client.generate_random().number_of_bytes(32).send().await,
        verbose
    ));

    // CreateCustomKeyStore / DescribeCustomKeyStores / DeleteCustomKeyStore
    let cks_r = client
        .create_custom_key_store()
        .custom_key_store_name("conformance-cks")
        .send()
        .await;
    let cks_id = cks_r
        .as_ref()
        .ok()
        .and_then(|r| r.custom_key_store_id.clone());
    results.push(chk!("CreateCustomKeyStore", cks_r, verbose));

    results.push(chk!(
        "DescribeCustomKeyStores",
        client.describe_custom_key_stores().send().await,
        verbose
    ));

    if let Some(ref cks) = cks_id {
        results.push(chk!(
            "DeleteCustomKeyStore",
            client
                .delete_custom_key_store()
                .custom_key_store_id(cks)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteCustomKeyStore".to_string()));
    }

    // Asymmetric key for Sign / Verify / GetPublicKey / GenerateDataKeyPair
    let asym_r = client
        .create_key()
        .key_spec(aws_sdk_kms::types::KeySpec::EccNistP256)
        .key_usage(aws_sdk_kms::types::KeyUsageType::SignVerify)
        .description("conformance asymmetric key")
        .send()
        .await;
    let asym_key_id = asym_r
        .as_ref()
        .ok()
        .and_then(|r| r.key_metadata.as_ref())
        .map(|m| m.key_id.clone());
    // We treat this CreateKey as informational (not added to results to avoid dup)
    let _ = asym_r;

    if let Some(ref akid) = asym_key_id {
        // Sign
        let msg_b64 = aws_sdk_kms::primitives::Blob::new(b"hello sign".to_vec());
        let sign_r = client
            .sign()
            .key_id(akid)
            .message(msg_b64)
            .signing_algorithm(aws_sdk_kms::types::SigningAlgorithmSpec::EcdsaSha256)
            .send()
            .await;
        let signature = sign_r.as_ref().ok().and_then(|r| r.signature.clone());
        results.push(chk!("Sign", sign_r, verbose));

        // Verify
        if let Some(sig) = signature {
            results.push(chk!(
                "Verify",
                client
                    .verify()
                    .key_id(akid)
                    .message(aws_sdk_kms::primitives::Blob::new(b"hello sign".to_vec()))
                    .signing_algorithm(aws_sdk_kms::types::SigningAlgorithmSpec::EcdsaSha256)
                    .signature(sig)
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("Verify".to_string()));
        }

        // GetPublicKey
        results.push(chk!(
            "GetPublicKey",
            client.get_public_key().key_id(akid).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("Sign".to_string()));
        results.push(OpResult::Skipped("Verify".to_string()));
        results.push(OpResult::Skipped("GetPublicKey".to_string()));
    }

    // GenerateDataKeyPair / GenerateDataKeyPairWithoutPlaintext — needs a symmetric key
    let sym_r = client
        .create_key()
        .description("conformance symmetric key for data key pair")
        .send()
        .await;
    let sym_key_id = sym_r
        .as_ref()
        .ok()
        .and_then(|r| r.key_metadata.as_ref())
        .map(|m| m.key_id.clone());
    let _ = sym_r;

    if let Some(ref skid) = sym_key_id {
        results.push(chk!(
            "GenerateDataKeyPair",
            client
                .generate_data_key_pair()
                .key_id(skid)
                .key_pair_spec(aws_sdk_kms::types::DataKeyPairSpec::EccNistP256)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "GenerateDataKeyPairWithoutPlaintext",
            client
                .generate_data_key_pair_without_plaintext()
                .key_id(skid)
                .key_pair_spec(aws_sdk_kms::types::DataKeyPairSpec::EccNistP256)
                .send()
                .await,
            verbose
        ));

        // UpdateKeyDescription
        results.push(chk!(
            "UpdateKeyDescription",
            client
                .update_key_description()
                .key_id(skid)
                .description("updated conformance description")
                .send()
                .await,
            verbose
        ));

        // GetKeyRotationStatus
        results.push(chk!(
            "GetKeyRotationStatus",
            client.get_key_rotation_status().key_id(skid).send().await,
            verbose
        ));

        // EnableKeyRotation
        results.push(chk!(
            "EnableKeyRotation",
            client.enable_key_rotation().key_id(skid).send().await,
            verbose
        ));

        // DisableKeyRotation
        results.push(chk!(
            "DisableKeyRotation",
            client.disable_key_rotation().key_id(skid).send().await,
            verbose
        ));

        // CreateGrant
        let grant_r = client
            .create_grant()
            .key_id(skid)
            .grantee_principal("arn:aws:iam::000000000000:role/ConformanceGrantee")
            .operations(aws_sdk_kms::types::GrantOperation::Encrypt)
            .send()
            .await;
        let grant_id = grant_r.as_ref().ok().and_then(|r| r.grant_id.clone());
        results.push(chk!("CreateGrant", grant_r, verbose));

        // ListGrants
        results.push(chk!(
            "ListGrants",
            client.list_grants().key_id(skid).send().await,
            verbose
        ));

        if let Some(ref gid) = grant_id {
            // RevokeGrant
            results.push(chk!(
                "RevokeGrant",
                client
                    .revoke_grant()
                    .key_id(skid)
                    .grant_id(gid)
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("RevokeGrant".to_string()));
        }

        // RetireGrant — create a second grant to retire
        let grant2_r = client
            .create_grant()
            .key_id(skid)
            .grantee_principal("arn:aws:iam::000000000000:role/ConformanceGrantee")
            .operations(aws_sdk_kms::types::GrantOperation::Decrypt)
            .send()
            .await;
        let grant2_token = grant2_r.as_ref().ok().and_then(|r| r.grant_token.clone());
        let _ = grant2_r;

        if let Some(ref tok) = grant2_token {
            results.push(chk!(
                "RetireGrant",
                client.retire_grant().grant_token(tok).send().await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("RetireGrant".to_string()));
        }

        // GetKeyPolicy
        results.push(chk!(
            "GetKeyPolicy",
            client
                .get_key_policy()
                .key_id(skid)
                .policy_name("default")
                .send()
                .await,
            verbose
        ));

        // PutKeyPolicy
        let policy_doc = r#"{"Version":"2012-10-17","Statement":[{"Sid":"Enable IAM User Permissions","Effect":"Allow","Principal":{"AWS":"arn:aws:iam::000000000000:root"},"Action":"kms:*","Resource":"*"}]}"#;
        results.push(chk!(
            "PutKeyPolicy",
            client
                .put_key_policy()
                .key_id(skid)
                .policy_name("default")
                .policy(policy_doc)
                .send()
                .await,
            verbose
        ));

        // ListKeyPolicies
        results.push(chk!(
            "ListKeyPolicies",
            client.list_key_policies().key_id(skid).send().await,
            verbose
        ));

        // TagResource (KMS)
        results.push(chk!(
            "TagResource",
            client
                .tag_resource()
                .key_id(skid)
                .tags(
                    aws_sdk_kms::types::Tag::builder()
                        .tag_key("env")
                        .tag_value("conformance")
                        .build()
                        .unwrap(),
                )
                .send()
                .await,
            verbose
        ));

        // ListResourceTags
        results.push(chk!(
            "ListResourceTags",
            client.list_resource_tags().key_id(skid).send().await,
            verbose
        ));

        // UntagResource (KMS)
        results.push(chk!(
            "UntagResource",
            client
                .untag_resource()
                .key_id(skid)
                .tag_keys("env")
                .send()
                .await,
            verbose
        ));

        // RotateKeyOnDemand
        results.push(chk!(
            "RotateKeyOnDemand",
            client.rotate_key_on_demand().key_id(skid).send().await,
            verbose
        ));

        // ListKeyRotations
        results.push(chk!(
            "ListKeyRotations",
            client.list_key_rotations().key_id(skid).send().await,
            verbose
        ));

        // GenerateMac
        let mac_r = client
            .generate_mac()
            .key_id(skid)
            .message(aws_sdk_kms::primitives::Blob::new(b"mac me".to_vec()))
            .mac_algorithm(aws_sdk_kms::types::MacAlgorithmSpec::HmacSha256)
            .send()
            .await;
        let mac_value = mac_r.as_ref().ok().and_then(|r| r.mac.clone());
        results.push(chk!("GenerateMac", mac_r, verbose));

        // VerifyMac
        if let Some(mac) = mac_value {
            results.push(chk!(
                "VerifyMac",
                client
                    .verify_mac()
                    .key_id(skid)
                    .message(aws_sdk_kms::primitives::Blob::new(b"mac me".to_vec()))
                    .mac_algorithm(aws_sdk_kms::types::MacAlgorithmSpec::HmacSha256)
                    .mac(mac)
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("VerifyMac".to_string()));
        }
    } else {
        for op in &[
            "GenerateDataKeyPair",
            "GenerateDataKeyPairWithoutPlaintext",
            "UpdateKeyDescription",
            "GetKeyRotationStatus",
            "EnableKeyRotation",
            "DisableKeyRotation",
            "CreateGrant",
            "ListGrants",
            "RevokeGrant",
            "RetireGrant",
            "GetKeyPolicy",
            "PutKeyPolicy",
            "ListKeyPolicies",
            "TagResource",
            "ListResourceTags",
            "UntagResource",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    results
}
