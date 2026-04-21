use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::{
    error::{resource_already_exists, resource_not_found},
    ids::{new_ec2_id, now_iso8601},
    state::{Ec2State, KeyPair},
};

use super::require_str;

fn fake_pem_material(key_name: &str) -> String {
    format!(
        "-----BEGIN RSA PRIVATE KEY-----\n\
         MIIEowIBAAKCAQEA{key_name}FAKEKEYDATA0000000000000000000000000000\n\
         0000000000000000000000000000000000000000000000000000000000000000\n\
         -----END RSA PRIVATE KEY-----"
    )
}

fn fake_fingerprint() -> String {
    // Produce a fake 59-char fingerprint like "aa:bb:cc:dd:..."
    let id = new_ec2_id("fp");
    let hex = &id[3..]; // strip prefix
    format!(
        "{0}:{1}:{2}:{3}:{4}:{5}:{6}:{7}",
        &hex[0..2],
        &hex[0..2],
        &hex[0..2],
        &hex[0..2],
        &hex[0..2],
        &hex[0..2],
        &hex[0..2],
        &hex[0..2],
    )
}

pub fn key_pair_to_summary(kp: &KeyPair) -> Value {
    json!({
        "keyName": kp.key_name,
        "keyFingerprint": kp.key_fingerprint,
        "createTime": kp.create_time,
    })
}

pub fn create_key_pair(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let key_name = require_str(input, "KeyName")?.to_string();

    if state.key_pairs.contains_key(&key_name) {
        return Err(resource_already_exists("key pair", &key_name));
    }

    let key_fingerprint = fake_fingerprint();
    let key_material = fake_pem_material(&key_name);

    let kp = KeyPair {
        key_name: key_name.clone(),
        key_fingerprint: key_fingerprint.clone(),
        key_material: key_material.clone(),
        create_time: now_iso8601(),
    };

    state.key_pairs.insert(key_name.clone(), kp);

    Ok(json!({
        "keyName": key_name,
        "keyFingerprint": key_fingerprint,
        "keyMaterial": key_material,
    }))
}

pub fn delete_key_pair(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let key_name = require_str(input, "KeyName")?;

    if state.key_pairs.remove(key_name).is_none() {
        return Err(resource_not_found("key pair", key_name));
    }

    Ok(json!({}))
}

pub fn describe_key_pairs(state: &Ec2State, _input: &Value) -> Result<Value, AwsError> {
    let pairs: Vec<Value> = state
        .key_pairs
        .iter()
        .map(|entry| key_pair_to_summary(&entry))
        .collect();

    Ok(json!({ "keySet": { "item": pairs } }))
}
