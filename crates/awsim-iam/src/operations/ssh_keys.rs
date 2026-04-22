use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::{
    error::no_such_entity,
    ids::{new_ssh_public_key_id, now_iso8601},
    state::{IamState, SshPublicKey},
};

use super::{opt_str, require_str};

fn key_to_value(k: &SshPublicKey) -> Value {
    json!({
        "SSHPublicKeyId": k.ssh_public_key_id,
        "UserName": k.user_name,
        "SSHPublicKeyBody": k.ssh_public_key_body,
        "Status": k.status,
        "UploadDate": k.upload_date,
    })
}

pub fn upload_ssh_public_key(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let ssh_public_key_body = require_str(input, "SSHPublicKeyBody")?;

    let mut user = state
        .users
        .get_mut(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    let key = SshPublicKey {
        ssh_public_key_id: new_ssh_public_key_id(),
        user_name: user_name.to_string(),
        ssh_public_key_body: ssh_public_key_body.to_string(),
        status: "Active".to_string(),
        upload_date: now_iso8601(),
    };

    let result = key_to_value(&key);
    user.ssh_public_keys.push(key);

    Ok(json!({ "SSHPublicKey": result }))
}

pub fn get_ssh_public_key(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let key_id = require_str(input, "SSHPublicKeyId")?;
    let _encoding = opt_str(input, "Encoding").unwrap_or("SSH");

    let user = state
        .users
        .get(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    let key = user
        .ssh_public_keys
        .iter()
        .find(|k| k.ssh_public_key_id == key_id)
        .ok_or_else(|| no_such_entity("SSHPublicKey", key_id))?;

    Ok(json!({ "SSHPublicKey": key_to_value(key) }))
}

pub fn list_ssh_public_keys(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;

    let user = state
        .users
        .get(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    let keys: Vec<Value> = user
        .ssh_public_keys
        .iter()
        .map(|k| {
            json!({
                "UserName": k.user_name,
                "SSHPublicKeyId": k.ssh_public_key_id,
                "Status": k.status,
                "UploadDate": k.upload_date,
            })
        })
        .collect();

    Ok(json!({
        "SSHPublicKeys": { "member": keys },
        "IsTruncated": false,
    }))
}

pub fn delete_ssh_public_key(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let key_id = require_str(input, "SSHPublicKeyId")?;

    let mut user = state
        .users
        .get_mut(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    let before = user.ssh_public_keys.len();
    user.ssh_public_keys.retain(|k| k.ssh_public_key_id != key_id);

    if user.ssh_public_keys.len() == before {
        return Err(no_such_entity("SSHPublicKey", key_id));
    }

    Ok(json!({}))
}

pub fn update_ssh_public_key(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let key_id = require_str(input, "SSHPublicKeyId")?;
    let status = require_str(input, "Status")?;

    let mut user = state
        .users
        .get_mut(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    let key = user
        .ssh_public_keys
        .iter_mut()
        .find(|k| k.ssh_public_key_id == key_id)
        .ok_or_else(|| no_such_entity("SSHPublicKey", key_id))?;

    key.status = status.to_string();

    Ok(json!({}))
}
