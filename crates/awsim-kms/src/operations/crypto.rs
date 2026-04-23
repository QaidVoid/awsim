use awsim_core::{AwsError, RequestContext};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::error;
use crate::state::KmsState;
use crate::operations::keys::resolve_key;

/// Encode `{key_id_bytes (36 bytes)}{xor_encrypted_data}` to base64.
///
/// key_id is a UUID string (36 ASCII chars). Plaintext is XOR'd with the
/// key's secret bytes (cycling). The ciphertext blob layout:
///   [0..36]  = key_id as ASCII bytes
///   [36..]   = plaintext XOR secret (cycling)
fn xor_encrypt(key_id: &str, secret: &[u8], plaintext: &[u8]) -> Vec<u8> {
    assert_eq!(key_id.len(), 36, "key_id must be a UUID string (36 chars)");
    let mut blob = Vec::with_capacity(36 + plaintext.len());
    blob.extend_from_slice(key_id.as_bytes());
    for (i, b) in plaintext.iter().enumerate() {
        blob.push(b ^ secret[i % secret.len()]);
    }
    blob
}

fn xor_decrypt(secret: &[u8], ciphertext_payload: &[u8]) -> Vec<u8> {
    ciphertext_payload
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ secret[i % secret.len()])
        .collect()
}

fn decode_ciphertext_blob(blob_b64: &str) -> Result<(String, Vec<u8>), AwsError> {
    let blob = BASE64
        .decode(blob_b64)
        .map_err(|_| error::invalid_parameter("CiphertextBlob is not valid base64"))?;
    if blob.len() < 36 {
        return Err(error::invalid_parameter("CiphertextBlob is too short"));
    }
    let key_id_bytes = &blob[..36];
    let key_id = String::from_utf8(key_id_bytes.to_vec())
        .map_err(|_| error::invalid_parameter("CiphertextBlob contains invalid key ID"))?;
    let payload = blob[36..].to_vec();
    Ok((key_id, payload))
}

// ---------------------------------------------------------------------------
// Encrypt
// ---------------------------------------------------------------------------

pub fn encrypt(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let plaintext_b64 = input["Plaintext"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("Plaintext"))?;

    let plaintext = BASE64
        .decode(plaintext_b64)
        .map_err(|_| error::invalid_parameter("Plaintext is not valid base64"))?;

    let key = resolve_key(state, key_id_input)?;

    if key.key_state == "Disabled" {
        return Err(error::key_disabled(&key.key_id));
    }
    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&key.key_id));
    }

    let blob = xor_encrypt(&key.key_id, &key.secret, &plaintext);
    let ciphertext_b64 = BASE64.encode(&blob);

    Ok(json!({
        "CiphertextBlob": ciphertext_b64,
        "KeyId": key.key_id,
        "EncryptionAlgorithm": "SYMMETRIC_DEFAULT",
    }))
}

// ---------------------------------------------------------------------------
// Decrypt
// ---------------------------------------------------------------------------

pub fn decrypt(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ciphertext_b64 = input["CiphertextBlob"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("CiphertextBlob"))?;

    let (key_id, payload) = decode_ciphertext_blob(ciphertext_b64)?;

    let key = state
        .keys
        .get(&key_id)
        .ok_or_else(|| error::not_found("Key"))?;

    if key.key_state == "Disabled" {
        return Err(error::key_disabled(&key.key_id));
    }
    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&key.key_id));
    }

    let plaintext = xor_decrypt(&key.secret, &payload);
    let plaintext_b64 = BASE64.encode(&plaintext);

    Ok(json!({
        "Plaintext": plaintext_b64,
        "KeyId": key.key_id,
        "EncryptionAlgorithm": "SYMMETRIC_DEFAULT",
    }))
}

// ---------------------------------------------------------------------------
// GenerateDataKey
// ---------------------------------------------------------------------------

pub fn generate_data_key(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let key_spec = input["KeySpec"].as_str().unwrap_or("AES_256");
    let data_key_len: usize = match key_spec {
        "AES_128" => 16,
        "AES_256" => 32,
        _ => {
            return Err(error::invalid_parameter(
                "KeySpec must be AES_128 or AES_256",
            ));
        }
    };

    let key = resolve_key(state, key_id_input)?;
    if key.key_state == "Disabled" {
        return Err(error::key_disabled(&key.key_id));
    }
    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&key.key_id));
    }

    // Generate a random data key
    let mut data_key_bytes = Vec::with_capacity(data_key_len);
    while data_key_bytes.len() < data_key_len {
        data_key_bytes.extend_from_slice(Uuid::new_v4().as_bytes());
    }
    data_key_bytes.truncate(data_key_len);

    let plaintext_b64 = BASE64.encode(&data_key_bytes);
    let blob = xor_encrypt(&key.key_id, &key.secret, &data_key_bytes);
    let ciphertext_b64 = BASE64.encode(&blob);

    Ok(json!({
        "Plaintext": plaintext_b64,
        "CiphertextBlob": ciphertext_b64,
        "KeyId": key.key_id,
    }))
}

// ---------------------------------------------------------------------------
// GenerateDataKeyWithoutPlaintext
// ---------------------------------------------------------------------------

pub fn generate_data_key_without_plaintext(
    state: &KmsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let mut result = generate_data_key(state, input, ctx)?;
    // Remove Plaintext from response
    result.as_object_mut().map(|m| m.remove("Plaintext"));
    Ok(result)
}

// ---------------------------------------------------------------------------
// GenerateRandom
// ---------------------------------------------------------------------------

pub fn generate_random(
    _state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let number_of_bytes = input["NumberOfBytes"]
        .as_u64()
        .unwrap_or(32) as usize;

    if number_of_bytes < 1 || number_of_bytes > 1024 {
        return Err(error::invalid_parameter(
            "NumberOfBytes must be between 1 and 1024",
        ));
    }

    let random_bytes = crate::util::random_secret(number_of_bytes);
    let plaintext = BASE64.encode(&random_bytes);

    Ok(json!({ "Plaintext": plaintext }))
}

// ---------------------------------------------------------------------------
// ReEncrypt
// ---------------------------------------------------------------------------

pub fn re_encrypt(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ciphertext_b64 = input["CiphertextBlob"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("CiphertextBlob"))?;

    let dest_key_id_input = input["DestinationKeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("DestinationKeyId"))?;

    // Decrypt with source key
    let (src_key_id, payload) = decode_ciphertext_blob(ciphertext_b64)?;
    let src_key = state
        .keys
        .get(&src_key_id)
        .ok_or_else(|| error::not_found("SourceKey"))?
        .clone();

    if src_key.key_state == "Disabled" {
        return Err(error::key_disabled(&src_key.key_id));
    }
    if src_key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&src_key.key_id));
    }

    let plaintext = xor_decrypt(&src_key.secret, &payload);

    // Resolve dest key
    let dest_key_id = crate::operations::keys::resolve_key_id(state, dest_key_id_input)?;
    let dest_key = state
        .keys
        .get(&dest_key_id)
        .ok_or_else(|| error::not_found("DestinationKey"))?
        .clone();

    if dest_key.key_state == "Disabled" {
        return Err(error::key_disabled(&dest_key.key_id));
    }
    if dest_key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&dest_key.key_id));
    }

    // Re-encrypt with destination key
    let new_blob = xor_encrypt(&dest_key.key_id, &dest_key.secret, &plaintext);
    let new_ciphertext_b64 = BASE64.encode(&new_blob);

    Ok(json!({
        "CiphertextBlob": new_ciphertext_b64,
        "KeyId": dest_key.key_id,
        "SourceKeyId": src_key.key_id,
        "SourceEncryptionAlgorithm": "SYMMETRIC_DEFAULT",
        "DestinationEncryptionAlgorithm": "SYMMETRIC_DEFAULT",
    }))
}
