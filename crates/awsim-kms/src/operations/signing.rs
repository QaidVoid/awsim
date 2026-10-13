use awsim_core::{AwsError, RequestContext};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};

use crate::asymmetric;
use crate::error;
use crate::operations::keys::resolve_key;
use crate::state::{KmsKey, KmsState};
use crate::util::random_secret;

// Signing algorithms KMS defines.
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

/// Look up a key for Sign/Verify and check it can use `algorithm`.
///
/// AWS rejects sign/verify when SigningAlgorithm isn't one the key spec
/// supports: an RSA key must use RSASSA_*, an ECC key the ECDSA_SHA_*
/// whose hash matches its curve, and SM2DSA pairs only with SM2.
fn signing_key(
    state: &KmsState,
    input: &Value,
    algorithm: &str,
    verb: &str,
) -> Result<KmsKey, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;
    if !SIGNING_ALGORITHMS.contains(&algorithm) {
        return Err(error::invalid_parameter(format!(
            "Unsupported SigningAlgorithm: {algorithm}"
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
            "Key {} cannot be used for {verb} because its KeyUsage is {}, not SIGN_VERIFY",
            key.key_id, key.key_usage
        )));
    }
    if !asymmetric::signing_algorithms(&key.key_spec).contains(&algorithm) {
        return Err(error::invalid_key_usage(format!(
            "Key {} (spec {}) does not support SigningAlgorithm {algorithm}.",
            key.key_id, key.key_spec
        )));
    }
    Ok(key)
}

/// The key's PKCS#8 private key. Asymmetric keys persisted before awsim
/// kept real material get it generated and stored on first use.
fn private_key(state: &KmsState, key: &KmsKey) -> Result<Vec<u8>, AwsError> {
    if !key.private_key.is_empty() {
        return Ok(key.private_key.clone());
    }
    let Some(der) = asymmetric::generate(&key.key_spec)? else {
        return Err(AwsError::bad_request(
            "UnsupportedOperationException",
            format!(
                "awsim does not implement key material for KeySpec {}",
                key.key_spec
            ),
        ));
    };
    if let Some(mut stored) = state.keys.get_mut(&key.key_id) {
        if stored.private_key.is_empty() {
            stored.private_key = der;
        }
        return Ok(stored.private_key.clone());
    }
    Ok(der)
}

/// Decode the base64 `Message` and reduce it to the digest the
/// algorithm signs, honouring `MessageType` (RAW by default).
fn message_digest(input: &Value, algorithm: &str) -> Result<Vec<u8>, AwsError> {
    let message_b64 = input["Message"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("Message"))?;
    let message = BASE64
        .decode(message_b64)
        .map_err(|_| error::invalid_parameter("Message is not valid base64"))?;
    let message_type = input["MessageType"].as_str().unwrap_or("RAW");
    asymmetric::digest(algorithm, &message, message_type)
}

// ---------------------------------------------------------------------------
// Sign
// ---------------------------------------------------------------------------

pub fn sign(state: &KmsState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    super::grants::validate_grant_tokens(state, input)?;
    if input["KeyId"].is_null() {
        return Err(error::missing_parameter("KeyId"));
    }
    if input["Message"].is_null() {
        return Err(error::missing_parameter("Message"));
    }
    let algorithm = input["SigningAlgorithm"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SigningAlgorithm"))?;
    let key = signing_key(state, input, algorithm, "signing")?;
    let digest = message_digest(input, algorithm)?;
    let signature = asymmetric::sign(
        &key.key_spec,
        &private_key(state, &key)?,
        algorithm,
        &digest,
    )?;

    Ok(json!({
        "KeyId": key.arn,
        "Signature": BASE64.encode(signature),
        "SigningAlgorithm": algorithm,
    }))
}

// ---------------------------------------------------------------------------
// Verify
// ---------------------------------------------------------------------------

pub fn verify(state: &KmsState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    super::grants::validate_grant_tokens(state, input)?;
    if input["KeyId"].is_null() {
        return Err(error::missing_parameter("KeyId"));
    }
    if input["Message"].is_null() {
        return Err(error::missing_parameter("Message"));
    }
    let signature_b64 = input["Signature"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("Signature"))?;
    let algorithm = input["SigningAlgorithm"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SigningAlgorithm"))?;
    let key = signing_key(state, input, algorithm, "verifying")?;
    let digest = message_digest(input, algorithm)?;
    let signature = BASE64
        .decode(signature_b64)
        .map_err(|_| error::invalid_parameter("Signature is not valid base64"))?;

    let valid = asymmetric::verify(
        &key.key_spec,
        &private_key(state, &key)?,
        algorithm,
        &digest,
        &signature,
    )?;
    // AWS reports a bad signature as an error, never SignatureValid=false.
    if !valid {
        return Err(AwsError::bad_request(
            "KMSInvalidSignatureException",
            "The signature is not valid for the message and key.",
        ));
    }

    Ok(json!({
        "KeyId": key.arn,
        "SignatureValid": true,
        "SigningAlgorithm": algorithm,
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
    if key.key_spec == "SYMMETRIC_DEFAULT" || key.key_spec.starts_with("HMAC_") {
        return Err(AwsError::bad_request(
            "UnsupportedOperationException",
            "GetPublicKey is not supported for symmetric keys",
        ));
    }

    let public_key = asymmetric::public_key(&key.key_spec, &private_key(state, &key)?)?;
    let mut out = json!({
        "KeyId": key.arn,
        "PublicKey": BASE64.encode(public_key),
        "CustomerMasterKeySpec": key.key_spec,
        "KeySpec": key.key_spec,
        "KeyUsage": key.key_usage,
    });
    asymmetric::insert_algorithms(&mut out, &key.key_spec, &key.key_usage);
    Ok(out)
}

// ---------------------------------------------------------------------------
// GenerateDataKeyPair
// ---------------------------------------------------------------------------

pub fn generate_data_key_pair(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    super::grants::validate_grant_tokens(state, input)?;
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
