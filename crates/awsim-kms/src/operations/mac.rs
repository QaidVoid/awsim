use awsim_core::{AwsError, RequestContext};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::error;
use crate::operations::keys::resolve_key_id;
use crate::state::KmsState;

fn compute_mac(secret: &[u8], message: &[u8], algorithm: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(algorithm.as_bytes());
    hasher.update(b":");
    hasher.update(secret);
    hasher.update(b":");
    hasher.update(message);
    hasher.finalize().to_vec()
}

/// AWS limits MacAlgorithm to one of the documented HMAC variants and
/// rejects anything else with InvalidParameterException. The
/// HMAC_<n> KeySpec must also match: an HMAC_256 key cannot run
/// HMAC_SHA_512 even though the algorithm is otherwise valid.
fn validate_mac_algorithm(algo: &str, key_spec: &str) -> Result<(), AwsError> {
    let expected_spec = match algo {
        "HMAC_SHA_224" => "HMAC_224",
        "HMAC_SHA_256" => "HMAC_256",
        "HMAC_SHA_384" => "HMAC_384",
        "HMAC_SHA_512" => "HMAC_512",
        _ => {
            return Err(error::invalid_parameter(format!(
                "Unsupported MacAlgorithm: {algo}. Must be one of HMAC_SHA_224 / 256 / 384 / 512."
            )));
        }
    };
    if key_spec != expected_spec {
        return Err(error::invalid_key_usage(format!(
            "MacAlgorithm {algo} requires KeySpec {expected_spec}; key has spec {key_spec}."
        )));
    }
    Ok(())
}

pub fn generate_mac(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;
    let message_b64 = input["Message"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("Message"))?;
    let algorithm = input["MacAlgorithm"]
        .as_str()
        .unwrap_or("HMAC_SHA_256")
        .to_string();

    let resolved_id = resolve_key_id(state, key_id_input)?;
    let key = state
        .keys
        .get(&resolved_id)
        .ok_or_else(|| error::not_found("Key"))?;
    if key.key_state != "Enabled" {
        return Err(error::key_disabled(&resolved_id));
    }
    if key.key_usage != "GENERATE_VERIFY_MAC" {
        return Err(error::invalid_key_usage(format!(
            "Key {resolved_id} cannot be used for MAC operations because its KeyUsage is \
             {}, not GENERATE_VERIFY_MAC",
            key.key_usage
        )));
    }
    validate_mac_algorithm(&algorithm, &key.key_spec)?;

    let message = BASE64
        .decode(message_b64)
        .map_err(|_| error::invalid_parameter("Message must be valid base64"))?;
    let mac = compute_mac(&key.secret, &message, &algorithm);

    Ok(json!({
        "KeyId": resolved_id,
        "Mac": BASE64.encode(mac),
        "MacAlgorithm": algorithm,
    }))
}

pub fn verify_mac(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;
    let message_b64 = input["Message"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("Message"))?;
    let mac_b64 = input["Mac"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("Mac"))?;
    let algorithm = input["MacAlgorithm"]
        .as_str()
        .unwrap_or("HMAC_SHA_256")
        .to_string();

    let resolved_id = resolve_key_id(state, key_id_input)?;
    let key = state
        .keys
        .get(&resolved_id)
        .ok_or_else(|| error::not_found("Key"))?;
    if key.key_state != "Enabled" {
        return Err(error::key_disabled(&resolved_id));
    }
    if key.key_usage != "GENERATE_VERIFY_MAC" {
        return Err(error::invalid_key_usage(format!(
            "Key {resolved_id} cannot be used for MAC operations because its KeyUsage is \
             {}, not GENERATE_VERIFY_MAC",
            key.key_usage
        )));
    }
    validate_mac_algorithm(&algorithm, &key.key_spec)?;

    let message = BASE64
        .decode(message_b64)
        .map_err(|_| error::invalid_parameter("Message must be valid base64"))?;
    let provided = BASE64
        .decode(mac_b64)
        .map_err(|_| error::invalid_parameter("Mac must be valid base64"))?;
    let expected = compute_mac(&key.secret, &message, &algorithm);

    Ok(json!({
        "KeyId": resolved_id,
        "MacValid": provided == expected,
        "MacAlgorithm": algorithm,
    }))
}
