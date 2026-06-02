use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::error;
use crate::state::{KmsCustomKeyStore, KmsState};

// ---------------------------------------------------------------------------
// CreateCustomKeyStore
// ---------------------------------------------------------------------------

pub fn create_custom_key_store(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["CustomKeyStoreName"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("CustomKeyStoreName"))?
        .to_string();

    // Check for duplicate name
    for entry in state.custom_key_stores.iter() {
        if entry.value().custom_key_store_name == name {
            return Err(AwsError::conflict(
                "CustomKeyStoreNameInUseException",
                format!("A custom key store with name '{name}' already exists"),
            ));
        }
    }

    let id = format!("cks-{}", &Uuid::new_v4().to_string().replace('-', "")[..16]);

    let cloud_hsm_cluster_id = input["CloudHsmClusterId"].as_str().map(|s| s.to_string());
    let trust_anchor_certificate = input["TrustAnchorCertificate"]
        .as_str()
        .map(|s| s.to_string());
    let custom_key_store_type = input["CustomKeyStoreType"]
        .as_str()
        .unwrap_or("AWS_CLOUDHSM")
        .to_string();
    let xks_proxy_uri_endpoint = input["XksProxyUriEndpoint"].as_str().map(|s| s.to_string());

    let store = KmsCustomKeyStore {
        custom_key_store_id: id.clone(),
        custom_key_store_name: name,
        connection_state: "DISCONNECTED".to_string(),
        cloud_hsm_cluster_id,
        trust_anchor_certificate,
        custom_key_store_type,
        xks_proxy_uri_endpoint,
    };

    state.custom_key_stores.insert(id.clone(), store);

    Ok(json!({ "CustomKeyStoreId": id }))
}

// ---------------------------------------------------------------------------
// DescribeCustomKeyStores
// ---------------------------------------------------------------------------

pub fn describe_custom_key_stores(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_id = input["CustomKeyStoreId"].as_str();
    let filter_name = input["CustomKeyStoreName"].as_str();

    let stores: Vec<Value> = state
        .custom_key_stores
        .iter()
        .filter(|e| {
            let v = e.value();
            if let Some(id) = filter_id {
                return v.custom_key_store_id == id;
            }
            if let Some(name) = filter_name {
                return v.custom_key_store_name == name;
            }
            true
        })
        .map(|e| {
            let v = e.value();
            json!({
                "CustomKeyStoreId": v.custom_key_store_id,
                "CustomKeyStoreName": v.custom_key_store_name,
                "ConnectionState": v.connection_state,
                "CustomKeyStoreType": v.custom_key_store_type,
                "CloudHsmClusterId": v.cloud_hsm_cluster_id,
                "TrustAnchorCertificate": v.trust_anchor_certificate,
                "XksProxyUriEndpoint": v.xks_proxy_uri_endpoint,
            })
        })
        .collect();

    Ok(json!({ "CustomKeyStores": stores, "Truncated": false }))
}

// ---------------------------------------------------------------------------
// DeleteCustomKeyStore
// ---------------------------------------------------------------------------

pub fn delete_custom_key_store(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input["CustomKeyStoreId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("CustomKeyStoreId"))?;

    if state.custom_key_stores.remove(id).is_none() {
        return Err(AwsError::not_found(
            "CustomKeyStoreNotFoundException",
            format!("Custom key store '{id}' does not exist"),
        ));
    }

    Ok(json!({}))
}

pub fn connect_custom_key_store(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input["CustomKeyStoreId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("CustomKeyStoreId"))?;
    let mut store = state.custom_key_stores.get_mut(id).ok_or_else(|| {
        AwsError::not_found(
            "CustomKeyStoreNotFoundException",
            format!("Custom key store '{id}' does not exist"),
        )
    })?;
    store.connection_state = "CONNECTED".to_string();
    Ok(json!({}))
}

pub fn disconnect_custom_key_store(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input["CustomKeyStoreId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("CustomKeyStoreId"))?;
    let mut store = state.custom_key_stores.get_mut(id).ok_or_else(|| {
        AwsError::not_found(
            "CustomKeyStoreNotFoundException",
            format!("Custom key store '{id}' does not exist"),
        )
    })?;
    store.connection_state = "DISCONNECTED".to_string();
    Ok(json!({}))
}

pub fn update_custom_key_store(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input["CustomKeyStoreId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("CustomKeyStoreId"))?;
    let mut store = state.custom_key_stores.get_mut(id).ok_or_else(|| {
        AwsError::not_found(
            "CustomKeyStoreNotFoundException",
            format!("Custom key store '{id}' does not exist"),
        )
    })?;

    if let Some(name) = input["NewCustomKeyStoreName"].as_str() {
        store.custom_key_store_name = name.to_string();
    }
    if let Some(cluster) = input["CloudHsmClusterId"].as_str() {
        store.cloud_hsm_cluster_id = Some(cluster.to_string());
    }
    if let Some(uri) = input["XksProxyUriEndpoint"].as_str() {
        store.xks_proxy_uri_endpoint = Some(uri.to_string());
    }

    Ok(json!({}))
}

pub fn update_primary_region(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;
    let _ = input["PrimaryRegion"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("PrimaryRegion"))?;
    let _ = crate::operations::keys::resolve_key_id(state, key_id_input)?;
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ReplicateKey (stub)
// ---------------------------------------------------------------------------

pub fn replicate_key(
    state: &KmsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let replica_region = input["ReplicaRegion"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("ReplicaRegion"))?;

    let key = crate::operations::keys::resolve_key(state, key_id_input)?;

    let replica_key_id = Uuid::new_v4().to_string();
    let replica_arn = format!(
        "arn:{}:kms:{}:{}:key/{}",
        ctx.partition, replica_region, ctx.account_id, replica_key_id
    );

    // Return stub metadata for the replica key
    let replica_metadata = json!({
        "KeyId": replica_key_id,
        "Arn": replica_arn,
        "Description": key.description,
        "KeyState": "Enabled",
        "KeySpec": key.key_spec,
        "KeyUsage": key.key_usage,
        "CreationDate": key.creation_date,
        "Enabled": true,
        "KeyManager": "CUSTOMER",
        "Origin": key.origin,
        "MultiRegion": true,
        "MultiRegionConfiguration": {
            "MultiRegionKeyType": "REPLICA",
            "PrimaryKey": {
                "Arn": key.arn,
                "Region": ctx.region,
            },
            "ReplicaKeys": [],
        },
    });

    Ok(json!({
        "ReplicaKeyMetadata": replica_metadata,
        "ReplicaPolicy": "{}",
        "ReplicaTags": [],
    }))
}
