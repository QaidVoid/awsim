use std::collections::HashMap;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

/// Password policy for a user pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordPolicy {
    pub minimum_length: u32,
    pub require_lowercase: bool,
    pub require_uppercase: bool,
    pub require_numbers: bool,
    pub require_symbols: bool,
    pub temporary_password_validity_days: u32,
}

impl Default for PasswordPolicy {
    fn default() -> Self {
        Self {
            minimum_length: 8,
            require_lowercase: true,
            require_uppercase: true,
            require_numbers: true,
            require_symbols: false,
            temporary_password_validity_days: 7,
        }
    }
}

/// AWS-shape constraints attached to a `String` schema attribute.
/// Encoded as decimal strings on the wire to match the Cognito API.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StringAttributeConstraints {
    pub min_length: Option<u32>,
    pub max_length: Option<u32>,
}

/// AWS-shape constraints attached to a `Number` schema attribute.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumberAttributeConstraints {
    pub min_value: Option<i64>,
    pub max_value: Option<i64>,
}

/// One entry in a user pool's schema. Mirrors Cognito's `SchemaAttributeType`.
///
/// `name` is the canonical attribute name *including* the `custom:`
/// prefix for custom attributes (so a schema lookup against a
/// `user.attributes` key is a direct equality check). Standard
/// OIDC attributes (`email`, `name`, `sub`, etc.) are stored
/// unprefixed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaAttribute {
    pub name: String,
    /// `String` | `Number` | `DateTime` | `Boolean`.
    pub attribute_data_type: String,
    pub required: bool,
    pub mutable: bool,
    /// Hidden from non-Admin* APIs. Stored for round-trip fidelity;
    /// awsim does not yet filter responses by this flag.
    #[serde(default)]
    pub developer_only_attribute: bool,
    #[serde(default)]
    pub string_attribute_constraints: Option<StringAttributeConstraints>,
    #[serde(default)]
    pub number_attribute_constraints: Option<NumberAttributeConstraints>,
}

/// AWS-defined OIDC standard attributes that every Cognito user pool
/// ships with. New pools start their schema with these so that
/// existing flows that set `email`, `name`, etc. don't trip the
/// "attribute does not exist in the schema" check.
///
/// Mirrors the Cognito console's "Required attributes" list: 19
/// String attrs, two Boolean (`*_verified`), one Number (`updated_at`).
/// `sub` is auto-generated and immutable; everything else is mutable
/// and not required by default. Pools can override `Required` /
/// `Mutable` per attribute via the `Schema` parameter on
/// `CreateUserPool`.
pub fn default_user_pool_schema() -> Vec<SchemaAttribute> {
    let s = |name: &str, required: bool, mutable: bool| SchemaAttribute {
        name: name.to_string(),
        attribute_data_type: "String".to_string(),
        required,
        mutable,
        developer_only_attribute: false,
        string_attribute_constraints: Some(StringAttributeConstraints {
            min_length: Some(0),
            max_length: Some(2048),
        }),
        number_attribute_constraints: None,
    };
    let b = |name: &str| SchemaAttribute {
        name: name.to_string(),
        attribute_data_type: "Boolean".to_string(),
        required: false,
        mutable: true,
        developer_only_attribute: false,
        string_attribute_constraints: None,
        number_attribute_constraints: None,
    };
    let n = |name: &str| SchemaAttribute {
        name: name.to_string(),
        attribute_data_type: "Number".to_string(),
        required: false,
        mutable: true,
        developer_only_attribute: false,
        string_attribute_constraints: None,
        number_attribute_constraints: None,
    };
    vec![
        s("sub", true, false),
        s("name", false, true),
        s("given_name", false, true),
        s("family_name", false, true),
        s("middle_name", false, true),
        s("nickname", false, true),
        s("preferred_username", false, true),
        s("profile", false, true),
        s("picture", false, true),
        s("website", false, true),
        s("email", false, true),
        b("email_verified"),
        s("gender", false, true),
        s("birthdate", false, true),
        s("zoneinfo", false, true),
        s("locale", false, true),
        s("phone_number", false, true),
        b("phone_number_verified"),
        s("address", false, true),
        n("updated_at"),
    ]
}

/// Maximum custom attributes per pool. Mirrors real Cognito's quota.
pub const MAX_CUSTOM_ATTRIBUTES: usize = 50;

/// Email configuration for a user pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    pub source_arn: Option<String>,
    pub reply_to_email_address: Option<String>,
    pub email_sending_account: String,
}

/// A resource server scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceServerScope {
    pub scope_name: String,
    pub scope_description: String,
}

/// A resource server registered with a user pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceServer {
    pub identifier: String,
    pub name: String,
    pub scopes: Vec<ResourceServerScope>,
    pub user_pool_id: String,
}

/// An identity provider registered with a user pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityProvider {
    pub provider_name: String,
    pub provider_type: String,
    pub provider_details: HashMap<String, String>,
    pub attribute_mapping: HashMap<String, String>,
    pub idp_identifiers: Vec<String>,
    pub creation_date: u64,
    pub last_modified_date: u64,
    pub user_pool_id: String,
}

/// UI customization for a user pool (or specific client).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiCustomization {
    pub css: Option<String>,
    pub image_url: Option<String>,
    pub creation_date: u64,
    pub last_modified_date: u64,
}

/// Managed login branding entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedLoginBranding {
    pub branding_id: String,
    pub user_pool_id: String,
    pub client_id: Option<String>,
    pub settings: serde_json::Value,
    pub assets: Vec<serde_json::Value>,
    pub creation_date: u64,
    pub last_modified_date: u64,
}

/// Advanced Security risk configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfiguration {
    pub client_id: Option<String>,
    pub compromised_credentials_config: Option<serde_json::Value>,
    pub account_takeover_config: Option<serde_json::Value>,
    pub risk_exception_config: Option<serde_json::Value>,
}

/// A user import job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserImportJob {
    pub job_id: String,
    pub user_pool_id: String,
    pub job_name: String,
    /// Created | Pending | InProgress | Stopping | Stopped | Succeeded | Failed | Expired
    pub status: String,
    pub cloud_watch_logs_role_arn: Option<String>,
    pub pre_signed_url: Option<String>,
    pub creation_date: u64,
    pub start_date: Option<u64>,
    pub completion_date: Option<u64>,
    pub imported_users: u64,
    pub skipped_users: u64,
    pub failed_users: u64,
}

/// Log delivery configuration entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogDeliveryConfiguration {
    pub log_configurations: Vec<serde_json::Value>,
}

/// A secondary client secret descriptor for a user pool client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSecretDescriptor {
    pub client_secret_id: String,
    pub client_secret_value: String,
    pub create_date: u64,
}

/// A user pool terms entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermsEntry {
    pub terms_id: String,
    pub user_pool_id: String,
    pub client_id: Option<String>,
    pub terms_name: String,
    pub terms_source: String,
    pub enforcement: String,
    pub links: HashMap<String, String>,
    pub creation_date: u64,
    pub last_modified_date: u64,
}

/// A WebAuthn credential registered for a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnCredential {
    pub credential_id: String,
    pub friendly_credential_name: Option<String>,
    pub relying_party_id: String,
    pub authenticator_attachment: Option<String>,
    pub authenticator_transports: Vec<String>,
    pub created_at: u64,
}

/// A Cognito User Pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPool {
    pub id: String,
    pub name: String,
    pub arn: String,
    pub clients: HashMap<String, UserPoolClient>,
    pub users: HashMap<String, CognitoUser>,
    pub groups: HashMap<String, CognitoGroup>,
    pub created_date: u64,
    // Extended fields
    pub policies: PasswordPolicy,
    pub mfa_configuration: String, // OFF, OPTIONAL, ON
    pub software_token_mfa_enabled: bool,
    pub auto_verified_attributes: Vec<String>,
    /// Attributes (`email`, `phone_number`) treated as the canonical Username.
    /// When set, the `Username` field on every API is the matching attribute
    /// value and the corresponding attribute is pinned to it.
    #[serde(default)]
    pub username_attributes: Vec<String>,
    /// Attributes accepted as sign-in aliases (`email`, `phone_number`,
    /// `preferred_username`).
    #[serde(default)]
    pub alias_attributes: Vec<String>,
    pub lambda_config: HashMap<String, String>, // trigger_type → function_arn
    pub schema: Vec<SchemaAttribute>,
    pub email_configuration: Option<EmailConfig>,
    pub domain: Option<String>,
    pub resource_servers: Vec<ResourceServer>,
    pub identity_providers: Vec<IdentityProvider>,
    pub tags: HashMap<String, String>,
    /// UI customization keyed by "pool" or client_id.
    pub ui_customizations: HashMap<String, UiCustomization>,
    /// Managed login branding entries.
    pub managed_login_brandings: Vec<ManagedLoginBranding>,
    /// Risk configurations keyed by client_id (or "pool" for pool-level).
    pub risk_configurations: Vec<RiskConfiguration>,
    /// User import jobs.
    pub import_jobs: Vec<UserImportJob>,
    /// Log delivery configuration.
    pub log_delivery_configuration: Option<LogDeliveryConfiguration>,
    /// Terms entries.
    pub terms: Vec<TermsEntry>,
    /// Test-only fixture for the CUSTOM_AUTH flow: when set, awsim emits a
    /// CUSTOM_CHALLENGE on InitiateAuth(CUSTOM_AUTH) and only accepts
    /// `ChallengeResponses["ANSWER"]` equal to this string. Without it,
    /// CUSTOM_AUTH still emits a challenge but accepts any non-empty
    /// answer (the production wiring would invoke a Lambda
    /// VerifyAuthChallengeResponse trigger to make the call).
    #[serde(default)]
    pub custom_auth_expected_answer: Option<String>,
    /// Public ChallengeParameters echoed back to the client on
    /// CUSTOM_CHALLENGE. Configurable per pool so tests can pin them.
    #[serde(default)]
    pub custom_auth_challenge_parameters: HashMap<String, String>,
}

/// A Cognito User Pool App Client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct UserPoolClient {
    pub client_id: String,
    pub client_name: String,
    pub user_pool_id: String,
    pub explicit_auth_flows: Vec<String>,
    pub created_date: u64,
    // Extended fields
    pub client_secret: Option<String>,
    pub callback_urls: Vec<String>,
    pub logout_urls: Vec<String>,
    pub allowed_oauth_flows: Vec<String>,
    pub allowed_oauth_scopes: Vec<String>,
    pub supported_identity_providers: Vec<String>,
    pub access_token_validity: u64,  // seconds
    pub id_token_validity: u64,      // seconds
    pub refresh_token_validity: u64, // seconds
    pub additional_client_secrets: Vec<ClientSecretDescriptor>,
}

/// Device info tracked per user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_key: String,
    pub device_group_key: String,
    pub device_name: Option<String>,
    pub remembered: bool,
    pub created_date: u64,
    pub last_authenticated_date: u64,
    pub last_modified_date: u64,
}

/// An external identity provider linked to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkedProvider {
    pub provider_name: String,
    pub provider_attribute_name: String,
    pub provider_attribute_value: String,
}

/// A Cognito user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitoUser {
    pub username: String,
    pub sub: String,
    /// Bcrypt hash of the user's password. Always set (the empty password
    /// case is normalised by [`crate::password::hash`]) and never logged.
    /// Field is named `password` on the wire for snapshot compatibility,
    /// but its value is *only* a bcrypt hash, never plaintext.
    #[serde(rename = "password")]
    pub password_hash: String,
    /// Hex-encoded 16-byte SRP salt, computed alongside `password_hash`
    /// when a password is set. Only used by USER_SRP_AUTH.
    #[serde(default)]
    pub srp_salt: Option<String>,
    /// Hex-encoded SRP verifier (g^x mod N), computed alongside the salt.
    /// Only used by USER_SRP_AUTH.
    #[serde(default)]
    pub srp_verifier: Option<String>,
    pub attributes: HashMap<String, String>,
    /// CONFIRMED | UNCONFIRMED | FORCE_CHANGE_PASSWORD | RESET_REQUIRED
    pub status: String,
    pub enabled: bool,
    pub groups: Vec<String>,
    pub created_date: u64,
    /// Pending verification codes, keyed by attribute name.
    pub pending_verifications: HashMap<String, String>,
    /// Issue time (Unix seconds) for each entry in `pending_verifications`,
    /// used to enforce code expiry. Codes whose key is in
    /// `pending_verifications` but missing here are treated as legacy
    /// pre-expiry entries and rejected as expired (fail-closed) so an
    /// imported snapshot can't make a stale code re-usable.
    #[serde(default)]
    pub pending_verifications_issued: HashMap<String, u64>,
    /// Consecutive failed attempts to consume any verification code on this
    /// user (sign-up confirmation, forgot-password, attribute-verify). Reset
    /// to 0 on a successful match.
    #[serde(default)]
    pub code_failed_attempts: u32,
    /// Unix seconds until which this user is rate-limited from submitting
    /// any code. Set after `code_failed_attempts` crosses the threshold,
    /// cleared once expired or after a successful match.
    #[serde(default)]
    pub code_locked_until_secs: Option<u64>,
    /// Revoked refresh tokens for this user.
    pub revoked_refresh_tokens: Vec<String>,
    /// Whether MFA is enabled for this user.
    pub mfa_enabled: bool,
    /// Preferred MFA method: "SOFTWARE_TOKEN_MFA" or "SMS_MFA"
    pub mfa_preferred: Option<String>,
    /// Base32-encoded TOTP secret.
    pub totp_secret: Option<String>,
    /// Whether TOTP has been verified by the user.
    pub totp_verified: bool,
    /// Registered devices for this user.
    pub devices: Vec<DeviceInfo>,
    /// Externally linked identity providers.
    pub linked_providers: Vec<LinkedProvider>,
    /// MFA options (legacy SetUserSettings/AdminSetUserSettings).
    pub mfa_options: Vec<HashMap<String, String>>,
    /// WebAuthn credentials registered for this user.
    pub webauthn_credentials: Vec<WebAuthnCredential>,
    /// In-flight WebAuthn registration challenges keyed by credential id placeholder.
    pub webauthn_pending_challenge: Option<String>,
    /// Consecutive failed login attempts since the last success or unlock.
    /// Reset to 0 on a successful login.
    #[serde(default)]
    pub failed_login_attempts: u32,
    /// Unix timestamp (seconds) until which authentication should be rejected
    /// with `NotAuthorizedException`. Cleared automatically once it expires.
    #[serde(default)]
    pub locked_until_secs: Option<u64>,
    /// Bounded ring of recent advanced-security events (sign-ins, sign-ups,
    /// password changes). Surfaced via AdminListUserAuthEvents and consulted
    /// by adaptive-auth-style risk decisions.
    #[serde(default)]
    pub auth_events: Vec<AuthEvent>,
}

/// A single recorded advanced-security event for a Cognito user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthEvent {
    pub event_id: String,
    /// "SignIn" | "PasswordChange" | "SignUp" | "ForgotPassword"
    pub event_type: String,
    pub creation_date: u64,
    /// "Pass" | "Fail" | "InProgress"
    pub event_response: String,
    /// "Low" | "Medium" | "High"
    pub risk_level: String,
    /// "NoRisk" | "AccountTakeover" | "Block"
    pub risk_decision: String,
    pub compromised_credentials_detected: bool,
    /// Optional user feedback ("Valid" | "Invalid").
    pub feedback_value: Option<String>,
}

/// A Cognito User Pool group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitoGroup {
    pub group_name: String,
    pub description: Option<String>,
    pub role_arn: Option<String>,
    pub precedence: Option<u32>,
    pub user_pool_id: String,
    pub created_date: u64,
}

/// A simple revocation store for invalidated tokens.
#[derive(Debug, Default, Clone)]
pub struct TokenRevocationStore {
    /// Set of access token strings that have been signed out.
    pub revoked: DashMap<String, ()>,
}

/// An in-flight MFA session produced after credentials are validated but before
/// the TOTP challenge response is received.
#[derive(Debug, Clone)]
pub struct MfaSession {
    pub pool_id: String,
    pub username: String,
    /// Unix seconds at which this session was issued. Sessions older than
    /// the auth_session_validity (5 minutes by default) are rejected, so
    /// a leaked session id can't be replayed indefinitely.
    pub issued_at: u64,
}

/// Pending SRP exchange: emitted on InitiateAuth(USER_SRP_AUTH) and
/// consumed by RespondToAuthChallenge(PASSWORD_VERIFIER).
#[derive(Debug, Clone)]
pub struct SrpSession {
    pub pool_id: String,
    pub username: String,
    pub client_id: String,
    /// Server private key b, hex-encoded.
    pub b_priv_hex: String,
    /// Server public key B, hex-encoded.
    pub b_pub_hex: String,
    /// Salt (hex) the client received in the challenge.
    pub salt_hex: String,
    /// Opaque secret block the client must echo back.
    pub secret_block_b64: String,
    /// Unix seconds at which this session was issued.
    pub issued_at: u64,
}

/// Per-account/region Cognito state.
#[derive(Debug, Default, Clone)]
pub struct CognitoState {
    /// PoolId → UserPool
    pub user_pools: DashMap<String, UserPool>,
    /// Revoked tokens (GlobalSignOut).
    pub revoked_tokens: TokenRevocationStore,
    /// Domain → PoolId mapping for domain lookups.
    pub domain_pool_map: DashMap<String, String>,
    /// ResourceArn → Tags for tag management.
    pub resource_tags: DashMap<String, HashMap<String, String>>,
    /// In-flight MFA sessions: session_id → (pool_id, username).
    pub mfa_sessions: DashMap<String, MfaSession>,
    /// Pending confirmation codes: "pool_id:username" -> code.
    pub confirmation_codes: DashMap<String, String>,
    /// Issue times (Unix seconds) for entries in `confirmation_codes`.
    /// Treated the same way as `pending_verifications_issued` on the
    /// user record: missing entry == expired.
    pub confirmation_codes_issued: DashMap<String, u64>,
    /// In-flight SRP exchanges: session_id → SrpSession.
    pub srp_sessions: DashMap<String, SrpSession>,
}
