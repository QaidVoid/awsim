use dashmap::DashMap;
use std::collections::HashMap;

/// A single version of a secret value.
#[derive(Debug, Clone)]
pub struct SecretVersion {
    pub version_id: String,
    pub secret_string: Option<String>,
    /// base64-encoded binary value
    pub secret_binary: Option<String>,
    /// e.g. ["AWSCURRENT"], ["AWSPREVIOUS"]
    pub stages: Vec<String>,
    /// Unix epoch seconds (f64) — matches awsJson1.1 timestamp wire format.
    pub created_date: f64,
}

/// A secret and all its versions.
#[derive(Debug, Clone)]
pub struct Secret {
    pub arn: String,
    pub name: String,
    pub description: String,
    /// version_id → SecretVersion
    pub versions: HashMap<String, SecretVersion>,
    pub current_version_id: String,
    pub tags: HashMap<String, String>,
    /// Unix epoch seconds (f64) — matches awsJson1.1 timestamp wire format.
    pub created_date: f64,
    /// Unix epoch seconds (f64).
    pub last_changed_date: f64,
    /// Unix epoch seconds (f64), or None if not scheduled for deletion.
    pub deleted_date: Option<f64>,
    /// Whether automatic rotation is enabled.
    pub rotation_enabled: bool,
    /// ARN of the Lambda function that performs rotation.
    pub rotation_lambda_arn: Option<String>,
    /// Days between automatic rotations.
    pub rotation_automatically_after_days: Option<u64>,
    /// KMS key ARN/alias used to encrypt secret values at rest. None
    /// means the AWS-managed `aws/secretsmanager` key (unsurfaced in
    /// Describe responses, matching AWS).
    pub kms_key_id: Option<String>,
    /// Unix epoch seconds — last time RotateSecret successfully ran.
    /// `None` until the first rotation completes.
    pub last_rotated_date: Option<f64>,
    /// Unix epoch seconds — last time the secret value was retrieved
    /// (any GetSecretValue call). Surfaces in Describe / ListSecrets.
    pub last_accessed_date: Option<f64>,
}

/// Per-account/region Secrets Manager state.
#[derive(Debug, Default)]
pub struct SecretsState {
    /// name → Secret (primary index)
    pub secrets: DashMap<String, Secret>,
    /// secret name → JSON resource policy string
    pub resource_policies: DashMap<String, String>,
}
