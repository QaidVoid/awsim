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
    /// Unix epoch seconds — matches awsJson1.1 timestamp wire format.
    pub creation_date: f64,
    /// Random bytes used for XOR-based emulated encryption.
    pub secret: Vec<u8>,
    /// Unix epoch seconds at which this key is scheduled for deletion.
    pub deletion_date: Option<f64>,
}

/// Per-account/region KMS state.
#[derive(Debug, Default)]
pub struct KmsState {
    /// KeyId → KmsKey
    pub keys: DashMap<String, KmsKey>,
    /// alias_name (e.g. "alias/my-key") → key_id
    pub aliases: DashMap<String, String>,
}
