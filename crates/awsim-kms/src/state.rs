use dashmap::DashMap;

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
    pub creation_date: String,
    /// Random bytes used for XOR-based emulated encryption.
    pub secret: Vec<u8>,
    pub deletion_date: Option<String>,
}

/// Per-account/region KMS state.
#[derive(Debug, Default)]
pub struct KmsState {
    /// KeyId → KmsKey
    pub keys: DashMap<String, KmsKey>,
    /// alias_name (e.g. "alias/my-key") → key_id
    pub aliases: DashMap<String, String>,
}
