use std::collections::HashMap;

use dashmap::DashMap;

/// A KMS grant (simplified).
#[derive(Debug, Clone)]
pub struct KmsGrant {
    pub grant_id: String,
    pub grant_token: String,
    pub key_id: String,
    pub name: Option<String>,
    pub grantee_principal: String,
    pub operations: Vec<String>,
    /// Unix-epoch seconds when the grant token was minted. AWS lets a
    /// grant token authorize operations for 5 minutes after creation
    /// (after which the underlying grant must be used by id instead).
    pub token_created_at: u64,
    /// `Constraints.EncryptionContextEquals`: every key/value pair must
    /// match the operation's encryption context exactly, and no extra
    /// pairs may be present.
    pub encryption_context_equals: std::collections::BTreeMap<String, String>,
    /// `Constraints.EncryptionContextSubset`: every key/value pair must
    /// be present in the operation's encryption context, but additional
    /// pairs are allowed.
    pub encryption_context_subset: std::collections::BTreeMap<String, String>,
}

/// A KMS key.
#[derive(Debug, Clone)]
pub struct KmsKey {
    pub key_id: String,
    pub arn: String,
    pub description: String,
    /// "Enabled", "Disabled", "PendingDeletion"
    pub key_state: String,
    /// "SYMMETRIC_DEFAULT", "RSA_2048", "RSA_3072", "RSA_4096", "ECC_NIST_P256", etc.
    pub key_spec: String,
    /// "ENCRYPT_DECRYPT", "SIGN_VERIFY"
    pub key_usage: String,
    /// Unix epoch seconds — matches awsJson1.1 timestamp wire format.
    pub creation_date: f64,
    /// Random bytes used for XOR-based emulated encryption.
    pub secret: Vec<u8>,
    /// Unix epoch seconds at which this key is scheduled for deletion.
    pub deletion_date: Option<f64>,
    /// Whether automatic key rotation is enabled.
    pub rotation_enabled: bool,
    /// Key policy document (JSON string), keyed by policy name.
    pub policies: HashMap<String, String>,
    /// Resource tags: key → value.
    pub tags: HashMap<String, String>,
    /// Whether key material has been imported.
    pub key_material_imported: bool,
    /// "AWS_KMS" or "EXTERNAL"
    pub origin: String,
}

/// A custom key store stub.
#[derive(Debug, Clone)]
pub struct KmsCustomKeyStore {
    pub custom_key_store_id: String,
    pub custom_key_store_name: String,
    pub connection_state: String,
    pub cloud_hsm_cluster_id: Option<String>,
    pub trust_anchor_certificate: Option<String>,
    pub custom_key_store_type: String,
    pub xks_proxy_uri_endpoint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct KeyRotationEvent {
    pub key_id: String,
    pub rotation_date: f64,
    pub rotation_type: String,
}

/// Per-account/region KMS state.
#[derive(Debug, Default)]
pub struct KmsState {
    /// KeyId → KmsKey
    pub keys: DashMap<String, KmsKey>,
    /// alias_name (e.g. "alias/my-key") → key_id
    pub aliases: DashMap<String, String>,
    /// GrantId → KmsGrant
    pub grants: DashMap<String, KmsGrant>,
    /// CustomKeyStoreId → KmsCustomKeyStore
    pub custom_key_stores: DashMap<String, KmsCustomKeyStore>,
    /// KeyId -> rotation events
    pub key_rotations: DashMap<String, Vec<KeyRotationEvent>>,
}
