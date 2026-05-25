use awsim_core::{AwsError, RequestContext};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};

use crate::error;
use crate::operations::keys::resolve_key;
use crate::state::KmsState;
use crate::util::random_secret;

// Valid signing algorithms accepted by the stub.
const SIGNING_ALGORITHMS: &[&str] = &[
    "RSASSA_PSS_SHA_256",
    "RSASSA_PSS_SHA_384",
    "RSASSA_PSS_SHA_512",
    "RSASSA_PKCS1_V1_5_SHA_256",
    "RSASSA_PKCS1_V1_5_SHA_384",
    "RSASSA_PKCS1_V1_5_SHA_512",
    "ECDSA_SHA_256",
    "ECDSA_SHA_384",
    "ECDSA_SHA_512",
    "SM2DSA",
];

/// AWS rejects sign/verify when SigningAlgorithm doesn't match the
/// KeySpec family: an RSA key must use RSASSA_*, an ECC NIST key must
/// use ECDSA_SHA_*, and SM2DSA pairs only with the SM2 KeySpec.
///
/// The exact ECDSA hash size is also bound to the curve (ECC_NIST_P256
/// rejects ECDSA_SHA_384). Surface that as InvalidKeyUsageException so
/// signers fix the algorithm choice instead of getting a silent stub.
fn signing_algorithm_matches_key_spec(algo: &str, key_spec: &str) -> bool {
    match (algo, key_spec) {
        // RSA: every RSASSA_* works for any RSA_* spec.
        (a, s) if a.starts_with("RSASSA_") && s.starts_with("RSA_") => true,
        // ECC NIST: hash size must match curve size.
        ("ECDSA_SHA_256", "ECC_NIST_P256" | "ECC_SECG_P256K1") => true,
        ("ECDSA_SHA_384", "ECC_NIST_P384") => true,
        ("ECDSA_SHA_512", "ECC_NIST_P521") => true,
        // SM2 has its own DSA variant.
        ("SM2DSA", "SM2") => true,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Sign
// ---------------------------------------------------------------------------

pub fn sign(state: &KmsState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let _message_b64 = input["Message"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("Message"))?;

    let signing_algorithm = input["SigningAlgorithm"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SigningAlgorithm"))?;

    if !SIGNING_ALGORITHMS.contains(&signing_algorithm) {
        return Err(error::invalid_parameter(format!(
            "Unsupported SigningAlgorithm: {signing_algorithm}"
        )));
    }

    let key = resolve_key(state, key_id_input)?;

    if key.key_state == "Disabled" {
        return Err(error::key_disabled(&key.key_id));
    }
    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&key.key_id));
    }
    if key.key_usage != "SIGN_VERIFY" {
        return Err(error::invalid_key_usage(format!(
            "Key {} cannot be used for signing because its KeyUsage is {}, not SIGN_VERIFY",
            key.key_id, key.key_usage
        )));
    }
    if !signing_algorithm_matches_key_spec(signing_algorithm, &key.key_spec) {
        return Err(error::invalid_key_usage(format!(
            "Key {} (spec {}) does not support SigningAlgorithm {signing_algorithm}.",
            key.key_id, key.key_spec
        )));
    }

    // Return a random 64-byte fake signature
    let signature_bytes = random_secret(64);
    let signature_b64 = BASE64.encode(&signature_bytes);

    Ok(json!({
        "KeyId": key.arn,
        "Signature": signature_b64,
        "SigningAlgorithm": signing_algorithm,
    }))
}

// ---------------------------------------------------------------------------
// Verify
// ---------------------------------------------------------------------------

pub fn verify(state: &KmsState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let _message_b64 = input["Message"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("Message"))?;

    let _signature_b64 = input["Signature"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("Signature"))?;

    let signing_algorithm = input["SigningAlgorithm"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SigningAlgorithm"))?;

    let key = resolve_key(state, key_id_input)?;

    if key.key_state == "Disabled" {
        return Err(error::key_disabled(&key.key_id));
    }
    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&key.key_id));
    }
    if key.key_usage != "SIGN_VERIFY" {
        return Err(error::invalid_key_usage(format!(
            "Key {} cannot be used for verifying because its KeyUsage is {}, not SIGN_VERIFY",
            key.key_id, key.key_usage
        )));
    }
    if !signing_algorithm_matches_key_spec(signing_algorithm, &key.key_spec) {
        return Err(error::invalid_key_usage(format!(
            "Key {} (spec {}) does not support SigningAlgorithm {signing_algorithm}.",
            key.key_id, key.key_spec
        )));
    }

    // Stub: always report valid
    Ok(json!({
        "KeyId": key.arn,
        "SignatureValid": true,
        "SigningAlgorithm": signing_algorithm,
    }))
}

// ---------------------------------------------------------------------------
// GetPublicKey
// ---------------------------------------------------------------------------

pub fn get_public_key(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let key = resolve_key(state, key_id_input)?;

    if key.key_state == "Disabled" {
        return Err(error::key_disabled(&key.key_id));
    }
    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&key.key_id));
    }
    if key.key_spec == "SYMMETRIC_DEFAULT" {
        return Err(AwsError::bad_request(
            "UnsupportedOperationException",
            "GetPublicKey is not supported for symmetric keys",
        ));
    }

    // Return a mock 294-byte DER-encoded RSA-2048 public key stub.
    // Real callers will not cryptographically verify with this, which is fine for simulation.
    let mut mock_der = vec![0x30u8, 0x82, 0x01, 0x22]; // SEQUENCE header
    mock_der.extend_from_slice(&key.secret); // 32 bytes from key material
    // Pad to ~270 bytes total with deterministic zeros
    mock_der.resize(270, 0x00);
    let public_key_b64 = BASE64.encode(&mock_der);

    Ok(json!({
        "KeyId": key.arn,
        "PublicKey": public_key_b64,
        "KeySpec": key.key_spec,
        "KeyUsage": key.key_usage,
        "EncryptionAlgorithms": if key.key_usage == "ENCRYPT_DECRYPT" {
            json!(["RSAES_OAEP_SHA_1", "RSAES_OAEP_SHA_256"])
        } else {
            json!([])
        },
        "SigningAlgorithms": if key.key_usage == "SIGN_VERIFY" {
            json!(["RSASSA_PKCS1_V1_5_SHA_256", "RSASSA_PSS_SHA_256"])
        } else {
            json!([])
        },
    }))
}

// ---------------------------------------------------------------------------
// GenerateDataKeyPair
// ---------------------------------------------------------------------------

pub fn generate_data_key_pair(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let key_pair_spec = input["KeyPairSpec"].as_str().unwrap_or("RSA_2048");

    let key = resolve_key(state, key_id_input)?;
    if key.key_state == "Disabled" {
        return Err(error::key_disabled(&key.key_id));
    }
    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&key.key_id));
    }

    let private_key_bytes = random_secret(32);
    let public_key_bytes = random_secret(32);
    let private_key_plaintext_b64 = BASE64.encode(&private_key_bytes);
    let public_key_b64 = BASE64.encode(&public_key_bytes);

    // Encrypt the private key using the wrapping key (AES-256-GCM).
    use crate::operations::crypto::encrypt_raw;
    let encrypted_private_key_bytes = encrypt_raw(&key.key_id, &key.secret, &private_key_bytes);
    let private_key_ciphertext_blob_b64 = BASE64.encode(&encrypted_private_key_bytes);

    Ok(json!({
        "PrivateKeyCiphertextBlob": private_key_ciphertext_blob_b64,
        "PrivateKeyPlaintext": private_key_plaintext_b64,
        "PublicKey": public_key_b64,
        "KeyId": key.arn,
        "KeyPairSpec": key_pair_spec,
    }))
}

// ---------------------------------------------------------------------------
// GenerateDataKeyPairWithoutPlaintext
// ---------------------------------------------------------------------------

pub fn generate_data_key_pair_without_plaintext(
    state: &KmsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let mut result = generate_data_key_pair(state, input, ctx)?;
    result
        .as_object_mut()
        .map(|m| m.remove("PrivateKeyPlaintext"));
    Ok(result)
}

// ---------------------------------------------------------------------------
// DeriveSharedSecret
// ---------------------------------------------------------------------------

pub fn derive_shared_secret(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let _public_key_b64 = input["PublicKey"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("PublicKey"))?;

    let key_agreement_algorithm = input["KeyAgreementAlgorithm"].as_str().unwrap_or("ECDH");

    let key = resolve_key(state, key_id_input)?;
    if key.key_state == "Disabled" {
        return Err(error::key_disabled(&key.key_id));
    }
    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&key.key_id));
    }
    if key.key_usage != "KEY_AGREEMENT" {
        return Err(error::invalid_key_usage(format!(
            "Key {} cannot be used for DeriveSharedSecret because its KeyUsage is {}, \
             not KEY_AGREEMENT",
            key.key_id, key.key_usage
        )));
    }

    // Return a fake 32-byte shared secret
    let shared_secret_bytes = random_secret(32);
    let shared_secret_b64 = BASE64.encode(&shared_secret_bytes);

    Ok(json!({
        "KeyId": key.arn,
        "SharedSecret": shared_secret_b64,
        "KeyAgreementAlgorithm": key_agreement_algorithm,
        "KeyOrigin": key.origin,
    }))
}
