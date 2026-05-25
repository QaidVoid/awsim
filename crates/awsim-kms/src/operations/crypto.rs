use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use awsim_core::{AwsError, RequestContext};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use rand::RngCore;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error;
use crate::operations::keys::resolve_key;
use crate::state::KmsState;

/// Real AES-256-GCM ciphertext layout:
///   [0..36]   = key_id (UUID ASCII)
///   [36..48]  = nonce (12 random bytes)
///   [48..]    = AES-GCM ciphertext (plaintext + 16-byte auth tag)
const KEY_ID_LEN: usize = 36;
const NONCE_LEN: usize = 12;

/// Derive a 32-byte AES-256 key from the KMS key's secret bytes via
/// SHA-256. The stored secret is variable-length, but AES-256 demands
/// 32 bytes; hashing is a stable, key-id-independent KDF.
fn derive_aes_key(secret: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(secret);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// AWS-documented EncryptionContext serialized-size limit, in bytes.
/// Exceeding this on any encrypt-side call returns ValidationException.
const ENCRYPTION_CONTEXT_MAX_BYTES: usize = 4096;

/// Validate the caller-supplied EncryptionContext shape and size.
///
/// AWS rejects:
/// - non-object payloads (must be `Map<String, String>`)
/// - non-string values
/// - serialized contexts above 4 KiB
fn validate_encryption_context(ctx: Option<&Value>) -> Result<(), AwsError> {
    let Some(val) = ctx else {
        return Ok(());
    };
    let obj = val.as_object().ok_or_else(|| {
        AwsError::validation("EncryptionContext must be an object of string-to-string pairs.")
    })?;
    let mut total = 0usize;
    for (k, v) in obj {
        let s = v.as_str().ok_or_else(|| {
            AwsError::validation(format!(
                "EncryptionContext value for key '{k}' must be a string."
            ))
        })?;
        total = total.saturating_add(k.len()).saturating_add(s.len());
    }
    if total > ENCRYPTION_CONTEXT_MAX_BYTES {
        return Err(AwsError::validation(format!(
            "EncryptionContext is {total} bytes; maximum is {ENCRYPTION_CONTEXT_MAX_BYTES}."
        )));
    }
    Ok(())
}

/// Serialize EncryptionContext into the canonical AAD bytes used by AWS
/// KMS: keys sorted ascending, joined as `k1=v1&k2=v2&...`. Returns an
/// empty Vec when the input is `None` or empty so the AEAD path stays
/// uniform.
fn encryption_context_aad(ctx: Option<&Value>) -> Vec<u8> {
    let Some(obj) = ctx.and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    let mut entries: Vec<(&str, &str)> = obj
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.as_str(), s)))
        .collect();
    entries.sort_by_key(|(k, _)| *k);
    let mut out = Vec::new();
    for (i, (k, v)) in entries.iter().enumerate() {
        if i > 0 {
            out.push(b'&');
        }
        out.extend_from_slice(k.as_bytes());
        out.push(b'=');
        out.extend_from_slice(v.as_bytes());
    }
    out
}

/// AES-256-GCM encrypt with the key's derived AES key. Generates a fresh
/// 96-bit nonce per call. AAD is bound (and required at decrypt) so a
/// caller that supplied EncryptionContext on encrypt cannot decrypt
/// without supplying the same context.
fn aes_encrypt(
    key_id: &str,
    secret: &[u8],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, AwsError> {
    assert_eq!(
        key_id.len(),
        KEY_ID_LEN,
        "key_id must be a UUID string (36 chars)"
    );
    let aes_key = derive_aes_key(secret);
    let cipher = Aes256Gcm::new((&aes_key).into());
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| AwsError::internal("AES-GCM encryption failed"))?;
    let mut blob = Vec::with_capacity(KEY_ID_LEN + NONCE_LEN + ct.len());
    blob.extend_from_slice(key_id.as_bytes());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ct);
    Ok(blob)
}

fn aes_decrypt(secret: &[u8], nonce_and_ct: &[u8], aad: &[u8]) -> Result<Vec<u8>, AwsError> {
    if nonce_and_ct.len() < NONCE_LEN + 16 {
        return Err(AwsError::bad_request(
            "InvalidCiphertextException",
            "CiphertextBlob is too short to contain a nonce and GCM tag.",
        ));
    }
    let aes_key = derive_aes_key(secret);
    let cipher = Aes256Gcm::new((&aes_key).into());
    let nonce = Nonce::from_slice(&nonce_and_ct[..NONCE_LEN]);
    cipher
        .decrypt(
            nonce,
            Payload {
                msg: &nonce_and_ct[NONCE_LEN..],
                aad,
            },
        )
        .map_err(|_| {
            AwsError::bad_request(
                "InvalidCiphertextException",
                "Ciphertext failed authenticated decryption",
            )
        })
}

/// Exported helper: AES-GCM-encrypt plaintext with a key's secret,
/// prefixed with key_id. Used by other modules (e.g. signing) that need
/// to produce a ciphertext blob.
pub fn encrypt_raw(key_id: &str, secret: &[u8], plaintext: &[u8]) -> Vec<u8> {
    aes_encrypt(key_id, secret, plaintext, &[]).unwrap_or_else(|_| Vec::new())
}

fn decode_ciphertext_blob(blob_b64: &str) -> Result<(String, Vec<u8>), AwsError> {
    let blob = BASE64.decode(blob_b64).map_err(|_| {
        AwsError::bad_request(
            "InvalidCiphertextException",
            "CiphertextBlob is not valid base64.",
        )
    })?;
    if blob.len() < KEY_ID_LEN + NONCE_LEN {
        return Err(AwsError::bad_request(
            "InvalidCiphertextException",
            "CiphertextBlob is too short to contain a KMS key id and nonce.",
        ));
    }
    let key_id_bytes = &blob[..KEY_ID_LEN];
    let key_id = String::from_utf8(key_id_bytes.to_vec()).map_err(|_| {
        AwsError::bad_request(
            "InvalidCiphertextException",
            "CiphertextBlob key-id segment is not valid UTF-8.",
        )
    })?;
    let payload = blob[KEY_ID_LEN..].to_vec();
    Ok((key_id, payload))
}

// ---------------------------------------------------------------------------
// Encrypt
// ---------------------------------------------------------------------------

pub fn encrypt(state: &KmsState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
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

    validate_encryption_context(input.get("EncryptionContext"))?;
    let aad = encryption_context_aad(input.get("EncryptionContext"));
    let blob = aes_encrypt(&key.key_id, &key.secret, &plaintext, &aad)?;
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

pub fn decrypt(state: &KmsState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
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

    validate_encryption_context(input.get("EncryptionContext"))?;
    let aad = encryption_context_aad(input.get("EncryptionContext"));
    let plaintext = aes_decrypt(&key.secret, &payload, &aad)?;
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
    validate_encryption_context(input.get("EncryptionContext"))?;
    let aad = encryption_context_aad(input.get("EncryptionContext"));
    let blob = aes_encrypt(&key.key_id, &key.secret, &data_key_bytes, &aad)?;
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
    let number_of_bytes = input["NumberOfBytes"].as_u64().unwrap_or(32) as usize;

    if !(1..=1024).contains(&number_of_bytes) {
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

    validate_encryption_context(input.get("SourceEncryptionContext"))?;
    let src_aad = encryption_context_aad(input.get("SourceEncryptionContext"));
    let plaintext = aes_decrypt(&src_key.secret, &payload, &src_aad)?;

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
    validate_encryption_context(input.get("DestinationEncryptionContext"))?;
    let dest_aad = encryption_context_aad(input.get("DestinationEncryptionContext"));
    let new_blob = aes_encrypt(&dest_key.key_id, &dest_key.secret, &plaintext, &dest_aad)?;
    let new_ciphertext_b64 = BASE64.encode(&new_blob);

    Ok(json!({
        "CiphertextBlob": new_ciphertext_b64,
        "KeyId": dest_key.key_id,
        "SourceKeyId": src_key.key_id,
        "SourceEncryptionAlgorithm": "SYMMETRIC_DEFAULT",
        "DestinationEncryptionAlgorithm": "SYMMETRIC_DEFAULT",
    }))
}
