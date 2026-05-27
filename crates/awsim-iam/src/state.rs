use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// IAM state — global per account (region is always "global" for IAM).
#[derive(Debug, Default)]
pub struct IamState {
    pub users: DashMap<String, User>,
    pub groups: DashMap<String, Group>,
    pub roles: DashMap<String, Role>,
    /// Keyed by ARN
    pub policies: DashMap<String, Policy>,
    pub instance_profiles: DashMap<String, InstanceProfile>,
    // Account-level
    pub account_aliases: std::sync::Mutex<Vec<String>>,
    pub account_password_policy: std::sync::Mutex<Option<AccountPasswordPolicy>>,
    // OIDC providers, keyed by ARN
    pub oidc_providers: DashMap<String, OidcProvider>,
    // SAML providers, keyed by ARN
    pub saml_providers: DashMap<String, SamlProvider>,
    // Server certificates, keyed by name
    pub server_certificates: DashMap<String, ServerCertificate>,
    // Virtual MFA devices, keyed by serial number (ARN)
    pub virtual_mfa_devices: DashMap<String, VirtualMfaDevice>,
    // Deletion task IDs (for service-linked roles)
    pub deletion_tasks: DashMap<String, String>,
    /// Login profiles (console passwords), keyed by user name.
    pub login_profiles: DashMap<String, LoginProfile>,
    /// Signing certificates keyed by certificate ID. Value: (UserName, body, status, upload_date).
    pub signing_certificates: DashMap<String, SigningCertificate>,
    /// Service-specific credentials keyed by id. Value tracks owner, service, status, etc.
    pub service_specific_credentials: DashMap<String, ServiceSpecificCredential>,
    /// Permissions boundary policy ARN per user (by user name).
    pub user_permissions_boundaries: DashMap<String, String>,
    /// Permissions boundary policy ARN per role (by role name).
    pub role_permissions_boundaries: DashMap<String, String>,
    /// Tracks last-used metadata for access keys keyed by access key id.
    pub access_key_last_used: DashMap<String, AccessKeyLastUsed>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletionTask {
    pub task_id: String,
    pub role_name: String,
    pub status: String,
}

/// Serializable snapshot of `IamState`.
#[derive(Debug, Serialize, Deserialize)]
pub struct IamStateSnapshot {
    pub users: Vec<User>,
    pub groups: Vec<Group>,
    pub roles: Vec<Role>,
    pub policies: Vec<Policy>,
    pub instance_profiles: Vec<InstanceProfile>,
    #[serde(default)]
    pub account_aliases: Vec<String>,
    #[serde(default)]
    pub account_password_policy: Option<AccountPasswordPolicy>,
    #[serde(default)]
    pub oidc_providers: Vec<OidcProvider>,
    #[serde(default)]
    pub saml_providers: Vec<SamlProvider>,
    #[serde(default)]
    pub server_certificates: Vec<ServerCertificate>,
    #[serde(default)]
    pub virtual_mfa_devices: Vec<VirtualMfaDevice>,
    #[serde(default)]
    pub login_profiles: Vec<LoginProfile>,
    #[serde(default)]
    pub signing_certificates: Vec<SigningCertificate>,
    #[serde(default)]
    pub service_specific_credentials: Vec<ServiceSpecificCredential>,
    #[serde(default)]
    pub user_permissions_boundaries: Vec<(String, String)>,
    #[serde(default)]
    pub role_permissions_boundaries: Vec<(String, String)>,
    #[serde(default)]
    pub access_key_last_used: Vec<(String, AccessKeyLastUsed)>,
    #[serde(default)]
    pub deletion_tasks: Vec<DeletionTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub user_name: String,
    /// AIDA... format, 20 chars
    pub user_id: String,
    pub arn: String,
    pub path: String,
    pub create_date: String,
    pub access_keys: Vec<AccessKey>,
    /// ARNs of attached managed policies
    pub attached_policies: Vec<String>,
    /// name → document
    pub inline_policies: HashMap<String, String>,
    /// group names this user belongs to
    pub groups: Vec<String>,
    #[serde(default)]
    pub tags: HashMap<String, String>,
    #[serde(default)]
    pub mfa_devices: Vec<String>,
    #[serde(default)]
    pub ssh_public_keys: Vec<SshPublicKey>,
    pub password_last_used: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessKey {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub status: String,
    pub create_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshPublicKey {
    pub ssh_public_key_id: String,
    pub user_name: String,
    pub ssh_public_key_body: String,
    pub status: String,
    pub upload_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub group_name: String,
    /// AGPA... format
    pub group_id: String,
    pub arn: String,
    pub path: String,
    pub create_date: String,
    /// User names belonging to this group
    pub members: Vec<String>,
    /// ARNs of attached managed policies
    pub attached_policies: Vec<String>,
    /// name → document
    pub inline_policies: HashMap<String, String>,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub role_name: String,
    /// AROA... format
    pub role_id: String,
    pub arn: String,
    pub path: String,
    pub assume_role_policy_document: String,
    pub description: Option<String>,
    pub create_date: String,
    /// Max session duration in seconds (default 3600)
    #[serde(default = "default_max_session_duration")]
    pub max_session_duration: u32,
    /// ARNs of attached managed policies
    pub attached_policies: Vec<String>,
    /// name → document
    pub inline_policies: HashMap<String, String>,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

fn default_max_session_duration() -> u32 {
    3600
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyVersion {
    pub version_id: String,
    pub document: String,
    pub is_default_version: bool,
    pub create_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub policy_name: String,
    /// ANPA... format
    pub policy_id: String,
    pub arn: String,
    pub path: String,
    pub description: Option<String>,
    pub policy_document: String,
    pub create_date: String,
    pub update_date: String,
    /// How many entities are attached
    pub attachment_count: u32,
    /// Policy versions (max 5)
    #[serde(default)]
    pub versions: Vec<PolicyVersion>,
    /// Current default version ID e.g. "v1"
    #[serde(default = "default_version_id")]
    pub default_version_id: String,
    /// Tags on this policy
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

fn default_version_id() -> String {
    "v1".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceProfile {
    pub instance_profile_name: String,
    /// AIPA... format
    pub instance_profile_id: String,
    pub arn: String,
    pub path: String,
    pub create_date: String,
    /// Role names associated
    pub roles: Vec<String>,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountPasswordPolicy {
    pub minimum_password_length: u32,
    pub require_symbols: bool,
    pub require_numbers: bool,
    pub require_uppercase_characters: bool,
    pub require_lowercase_characters: bool,
    pub allow_users_to_change_password: bool,
    pub max_password_age: u32,
    pub password_reuse_prevention: u32,
    pub hard_expiry: bool,
}

impl Default for AccountPasswordPolicy {
    fn default() -> Self {
        Self {
            minimum_password_length: 8,
            require_symbols: false,
            require_numbers: false,
            require_uppercase_characters: false,
            require_lowercase_characters: false,
            allow_users_to_change_password: false,
            max_password_age: 0,
            password_reuse_prevention: 0,
            hard_expiry: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcProvider {
    pub arn: String,
    pub url: String,
    pub client_id_list: Vec<String>,
    pub thumbprint_list: Vec<String>,
    pub tags: HashMap<String, String>,
    pub create_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamlProvider {
    pub arn: String,
    pub name: String,
    pub saml_metadata_document: String,
    pub tags: HashMap<String, String>,
    pub create_date: String,
    pub valid_until: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCertificate {
    pub server_certificate_name: String,
    pub server_certificate_id: String,
    pub arn: String,
    pub path: String,
    pub certificate_body: String,
    pub certificate_chain: Option<String>,
    pub upload_date: String,
    pub expiration: Option<String>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualMfaDevice {
    pub serial_number: String,
    pub base32_string_seed: Option<String>,
    pub qr_code_png: Option<String>,
    pub user: Option<String>,
    pub enable_date: Option<String>,
    pub tags: HashMap<String, String>,
    /// Lifecycle state for the device. Transitions:
    ///   `Unassigned` (just created) ->
    ///   `Active` (after EnableMFADevice with two valid codes) ->
    ///   `Unassigned` (after DeactivateMFADevice).
    /// `Resynced` marks a successful ResyncMFADevice without breaking the
    /// Active state — the field is set to a fresh `Active` afterwards.
    /// AWS doesn't expose a discrete state field on Virtual MFA, but
    /// surfacing one keeps the simulator's transitions observable.
    #[serde(default = "default_mfa_status")]
    pub status: String,
}

pub(crate) fn default_mfa_status() -> String {
    "Unassigned".to_string()
}

/// IAM login profile (console password) for a user.
///
/// The password itself is never stored; only the bcrypt hash. A
/// missing `password_hash` means the profile was created before
/// password storage was wired and any login attempt against it
/// fails. Operators should re-run `iam update-login-profile` to
/// reset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginProfile {
    pub user_name: String,
    pub create_date: String,
    /// Whether the user must reset their password on next sign-in.
    pub password_reset_required: bool,
    /// bcrypt hash of the user's console password. Optional so old
    /// snapshots that pre-date password storage still deserialise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningCertificate {
    pub user_name: String,
    pub certificate_id: String,
    pub certificate_body: String,
    pub status: String,
    pub upload_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSpecificCredential {
    pub user_name: String,
    pub service_name: String,
    pub service_user_name: String,
    pub service_specific_credential_id: String,
    pub service_password: String,
    pub status: String,
    pub create_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccessKeyLastUsed {
    pub last_used_date: Option<String>,
    pub service_name: String,
    pub region: String,
}
