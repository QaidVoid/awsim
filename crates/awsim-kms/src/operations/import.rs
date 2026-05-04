use awsim_core::{AwsError, RequestContext};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};

use crate::error;
use crate::operations::keys::resolve_key_id;
use crate::state::KmsState;
use crate::util::{now_epoch_f64, random_secret};

// ---------------------------------------------------------------------------
// GetParametersForImport
// ---------------------------------------------------------------------------

pub fn get_parameters_for_import(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let wrapping_algorithm = input["WrappingAlgorithm"]
        .as_str()
        .unwrap_or("RSAES_OAEP_SHA_256");

    let wrapping_key_spec = input["WrappingKeySpec"].as_str().unwrap_or("RSA_2048");

    let resolved_id = resolve_key_id(state, key_id_input)?;
    let key = state
        .keys
        .get(&resolved_id)
        .ok_or_else(|| error::not_found("Key"))?;

    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&resolved_id));
    }

    // Generate a fake public key / import token
    let public_key_bytes = random_secret(270);
    let import_token_bytes = random_secret(32);
    let public_key_b64 = BASE64.encode(&public_key_bytes);
    let import_token_b64 = BASE64.encode(&import_token_bytes);

    // Token expires in 24 hours
    let expiry = now_epoch_f64() + 86400.0;

    Ok(json!({
        "KeyId": key.arn,
        "PublicKey": public_key_b64,
        "ImportToken": import_token_b64,
        "ParametersValidTo": expiry,
        "WrappingAlgorithm": wrapping_algorithm,
        "WrappingKeySpec": wrapping_key_spec,
    }))
}

// ---------------------------------------------------------------------------
// ImportKeyMaterial
// ---------------------------------------------------------------------------

pub fn import_key_material(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let _encrypted_key_material = input["EncryptedKeyMaterial"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("EncryptedKeyMaterial"))?;

    let _import_token = input["ImportToken"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("ImportToken"))?;

    let resolved_id = resolve_key_id(state, key_id_input)?;
    let mut key = state
        .keys
        .get_mut(&resolved_id)
        .ok_or_else(|| error::not_found("Key"))?;

    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&resolved_id));
    }

    // ImportKeyMaterial only applies to keys created with Origin=EXTERNAL.
    // Keys originally created as AWS_KMS / AWS_CLOUDHSM cannot accept
    // imported material — AWS rejects with KMSInvalidStateException.
    if key.origin != "EXTERNAL" {
        return Err(error::kms_invalid_state(format!(
            "Key {resolved_id} was not created with Origin=EXTERNAL"
        )));
    }

    key.key_material_imported = true;
    if key.key_state == "PendingImport" {
        key.key_state = "Enabled".to_string();
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DeleteImportedKeyMaterial
// ---------------------------------------------------------------------------

pub fn delete_imported_key_material(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let resolved_id = resolve_key_id(state, key_id_input)?;
    let mut key = state
        .keys
        .get_mut(&resolved_id)
        .ok_or_else(|| error::not_found("Key"))?;

    if !key.key_material_imported {
        return Err(error::kms_invalid_state(
            "Key does not have imported key material",
        ));
    }

    key.key_material_imported = false;
    key.key_state = "PendingImport".to_string();

    Ok(json!({}))
}
