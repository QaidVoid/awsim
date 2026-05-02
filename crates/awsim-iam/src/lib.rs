pub mod authz;
mod error;
mod ids;
mod operations;
pub mod state;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::{DeletionTask, IamState, IamStateSnapshot};

/// IAM is a global service — we use account-only namespacing.
/// The region key is always "global" for IAM state lookups.
pub const IAM_REGION: &str = "global";

/// The AWSim IAM service handler.
pub struct IamService {
    store: AccountRegionStore<IamState>,
    /// Optional handle to the gateway authz engine. When set, the
    /// policy simulator pulls in resource policies, SCPs, KMS grants
    /// — i.e. evaluates the same way the live request path would —
    /// instead of identity-only.
    authz: std::sync::OnceLock<Arc<awsim_core::AuthzEngine>>,
}

impl IamService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
            authz: std::sync::OnceLock::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<IamState> {
        self.store.get(&ctx.account_id, IAM_REGION)
    }

    /// Expose the underlying store so the gateway can wire IAM-backed
    /// principal lookup into the authz engine.
    pub fn store(&self) -> AccountRegionStore<IamState> {
        self.store.clone()
    }

    /// Wire in a handle to the gateway authz engine. Done after the
    /// engine is fully built (lookups registered) so the simulator
    /// can use the same trait-object lookups for resource policies,
    /// SCPs, and grants. Idempotent — first call wins; subsequent
    /// calls are no-ops.
    pub fn set_authz(&self, authz: Arc<awsim_core::AuthzEngine>) {
        let _ = self.authz.set(authz);
    }

    pub(crate) fn authz(&self) -> Option<&Arc<awsim_core::AuthzEngine>> {
        self.authz.get()
    }
}

impl Default for IamService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for IamService {
    fn service_name(&self) -> &str {
        "iam"
    }

    fn signing_name(&self) -> &str {
        "iam"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsQuery
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "IAM request");
        let state = self.get_state(ctx);

        match operation {
            // Users
            "CreateUser" => operations::users::create_user(&state, &input, ctx),
            "GetUser" => operations::users::get_user(&state, &input),
            "DeleteUser" => operations::users::delete_user(&state, &input),
            "ListUsers" => operations::users::list_users(&state, &input),
            "UpdateUser" => operations::users::update_user(&state, &input),

            // Access Keys
            "CreateAccessKey" => operations::users::create_access_key(&state, &input, ctx),
            "DeleteAccessKey" => operations::users::delete_access_key(&state, &input),
            "ListAccessKeys" => operations::users::list_access_keys(&state, &input),

            // Groups
            "CreateGroup" => operations::groups::create_group(&state, &input, ctx),
            "GetGroup" => operations::groups::get_group(&state, &input),
            "DeleteGroup" => operations::groups::delete_group(&state, &input),
            "ListGroups" => operations::groups::list_groups(&state, &input),
            "AddUserToGroup" => operations::groups::add_user_to_group(&state, &input),
            "RemoveUserFromGroup" => operations::groups::remove_user_from_group(&state, &input),

            // Roles
            "CreateRole" => operations::roles::create_role(&state, &input, ctx),
            "GetRole" => operations::roles::get_role(&state, &input),
            "DeleteRole" => operations::roles::delete_role(&state, &input),
            "ListRoles" => operations::roles::list_roles(&state, &input),
            "UpdateAssumeRolePolicy" => {
                operations::roles::update_assume_role_policy(&state, &input)
            }
            "UpdateRole" => operations::roles::update_role(&state, &input),
            "UpdateRoleDescription" => operations::roles::update_role_description(&state, &input),

            // Policies (managed)
            "CreatePolicy" => operations::policies::create_policy(&state, &input, ctx),
            "GetPolicy" => operations::policies::get_policy(&state, &input),
            "DeletePolicy" => operations::policies::delete_policy(&state, &input),
            "ListPolicies" => operations::policies::list_policies(&state, &input),

            // Policy versions
            "CreatePolicyVersion" => operations::policies::create_policy_version(&state, &input),
            "DeletePolicyVersion" => operations::policies::delete_policy_version(&state, &input),
            "GetPolicyVersion" => operations::policies::get_policy_version(&state, &input),
            "ListPolicyVersions" => operations::policies::list_policy_versions(&state, &input),
            "SetDefaultPolicyVersion" => {
                operations::policies::set_default_policy_version(&state, &input)
            }

            // Attach/detach managed policies
            "AttachUserPolicy" => operations::policies::attach_user_policy(&state, &input),
            "DetachUserPolicy" => operations::policies::detach_user_policy(&state, &input),
            "AttachRolePolicy" => operations::policies::attach_role_policy(&state, &input),
            "DetachRolePolicy" => operations::policies::detach_role_policy(&state, &input),
            "AttachGroupPolicy" => operations::policies::attach_group_policy(&state, &input),
            "DetachGroupPolicy" => operations::policies::detach_group_policy(&state, &input),

            // List attached managed policies
            "ListAttachedUserPolicies" => {
                operations::policies::list_attached_user_policies(&state, &input)
            }
            "ListAttachedRolePolicies" => {
                operations::policies::list_attached_role_policies(&state, &input)
            }
            "ListAttachedGroupPolicies" => {
                operations::policies::list_attached_group_policies(&state, &input)
            }

            // Inline policies — put
            "PutUserPolicy" => operations::policies::put_user_policy(&state, &input),
            "PutRolePolicy" => operations::policies::put_role_policy(&state, &input),
            "PutGroupPolicy" => operations::policies::put_group_policy(&state, &input),

            // Inline policies — user
            "GetUserPolicy" => operations::users::get_user_policy(&state, &input),
            "DeleteUserPolicy" => operations::users::delete_user_policy(&state, &input),
            "ListUserPolicies" => operations::users::list_user_policies(&state, &input),

            // Inline policies — role
            "GetRolePolicy" => operations::roles::get_role_policy(&state, &input),
            "DeleteRolePolicy" => operations::roles::delete_role_policy(&state, &input),
            "ListRolePolicies" => operations::roles::list_role_policies(&state, &input),

            // Inline policies — group
            "GetGroupPolicy" => operations::groups::get_group_policy(&state, &input),
            "DeleteGroupPolicy" => operations::groups::delete_group_policy(&state, &input),
            "ListGroupPolicies" => operations::groups::list_group_policies(&state, &input),

            // Entity queries
            "ListGroupsForUser" => operations::users::list_groups_for_user(&state, &input),
            "ListEntitiesForPolicy" => {
                operations::policies::list_entities_for_policy(&state, &input)
            }

            // Policy tags
            "TagPolicy" => operations::policies::tag_policy(&state, &input),
            "UntagPolicy" => operations::policies::untag_policy(&state, &input),
            "ListPolicyTags" => operations::policies::list_policy_tags(&state, &input),

            // Instance Profiles
            "CreateInstanceProfile" => {
                operations::instance_profiles::create_instance_profile(&state, &input, ctx)
            }
            "DeleteInstanceProfile" => {
                operations::instance_profiles::delete_instance_profile(&state, &input)
            }
            "GetInstanceProfile" => {
                operations::instance_profiles::get_instance_profile(&state, &input)
            }
            "ListInstanceProfiles" => {
                operations::instance_profiles::list_instance_profiles(&state, &input)
            }
            "ListInstanceProfilesForRole" => {
                operations::instance_profiles::list_instance_profiles_for_role(&state, &input)
            }
            "AddRoleToInstanceProfile" => {
                operations::instance_profiles::add_role_to_instance_profile(&state, &input)
            }
            "RemoveRoleFromInstanceProfile" => {
                operations::instance_profiles::remove_role_from_instance_profile(&state, &input)
            }

            // ── User Tags ─────────────────────────────────────────────────────
            "TagUser" => operations::tags::tag_user(&state, &input),
            "UntagUser" => operations::tags::untag_user(&state, &input),
            "ListUserTags" => operations::tags::list_user_tags(&state, &input),

            // ── Role Tags ─────────────────────────────────────────────────────
            "TagRole" => operations::tags::tag_role(&state, &input),
            "UntagRole" => operations::tags::untag_role(&state, &input),
            "ListRoleTags" => operations::tags::list_role_tags(&state, &input),

            // ── Instance Profile Tags ─────────────────────────────────────────
            "TagInstanceProfile" => operations::tags::tag_instance_profile(&state, &input),
            "UntagInstanceProfile" => operations::tags::untag_instance_profile(&state, &input),
            "ListInstanceProfileTags" => {
                operations::tags::list_instance_profile_tags(&state, &input)
            }

            // ── Account Aliases ───────────────────────────────────────────────
            "CreateAccountAlias" => operations::account::create_account_alias(&state, &input),
            "DeleteAccountAlias" => operations::account::delete_account_alias(&state, &input),
            "ListAccountAliases" => operations::account::list_account_aliases(&state, &input),

            // ── Password Policy ───────────────────────────────────────────────
            "GetAccountPasswordPolicy" => {
                operations::account::get_account_password_policy(&state, &input)
            }
            "UpdateAccountPasswordPolicy" => {
                operations::account::update_account_password_policy(&state, &input)
            }
            "DeleteAccountPasswordPolicy" => {
                operations::account::delete_account_password_policy(&state, &input)
            }

            // ── Account Summary / Auth Details ────────────────────────────────
            "GetAccountSummary" => operations::account::get_account_summary(&state, &input),
            "GetAccountAuthorizationDetails" => {
                operations::account::get_account_authorization_details(&state, &input)
            }

            // ── OIDC Providers ────────────────────────────────────────────────
            "CreateOpenIDConnectProvider" => {
                operations::oidc::create_open_id_connect_provider(&state, &input, ctx)
            }
            "GetOpenIDConnectProvider" => {
                operations::oidc::get_open_id_connect_provider(&state, &input)
            }
            "ListOpenIDConnectProviders" => {
                operations::oidc::list_open_id_connect_providers(&state, &input)
            }
            "DeleteOpenIDConnectProvider" => {
                operations::oidc::delete_open_id_connect_provider(&state, &input)
            }
            "AddClientIDToOpenIDConnectProvider" => {
                operations::oidc::add_client_id_to_open_id_connect_provider(&state, &input)
            }
            "RemoveClientIDFromOpenIDConnectProvider" => {
                operations::oidc::remove_client_id_from_open_id_connect_provider(&state, &input)
            }
            "UpdateOpenIDConnectProviderThumbprint" => {
                operations::oidc::update_open_id_connect_provider_thumbprint(&state, &input)
            }

            // ── SAML Providers ────────────────────────────────────────────────
            "CreateSAMLProvider" => operations::saml::create_saml_provider(&state, &input, ctx),
            "GetSAMLProvider" => operations::saml::get_saml_provider(&state, &input),
            "ListSAMLProviders" => operations::saml::list_saml_providers(&state, &input),
            "DeleteSAMLProvider" => operations::saml::delete_saml_provider(&state, &input),
            "UpdateSAMLProvider" => operations::saml::update_saml_provider(&state, &input),

            // ── Server Certificates ───────────────────────────────────────────
            "UploadServerCertificate" => {
                operations::certificates::upload_server_certificate(&state, &input, ctx)
            }
            "GetServerCertificate" => {
                operations::certificates::get_server_certificate(&state, &input)
            }
            "ListServerCertificates" => {
                operations::certificates::list_server_certificates(&state, &input)
            }
            "DeleteServerCertificate" => {
                operations::certificates::delete_server_certificate(&state, &input)
            }
            "TagServerCertificate" => {
                operations::certificates::tag_server_certificate(&state, &input)
            }
            "UntagServerCertificate" => {
                operations::certificates::untag_server_certificate(&state, &input)
            }
            "ListServerCertificateTags" => {
                operations::certificates::list_server_certificate_tags(&state, &input)
            }

            // ── Virtual MFA Devices ───────────────────────────────────────────
            "CreateVirtualMFADevice" => {
                operations::mfa::create_virtual_mfa_device(&state, &input, ctx)
            }
            "ListVirtualMFADevices" => operations::mfa::list_virtual_mfa_devices(&state, &input),
            "DeleteVirtualMFADevice" => operations::mfa::delete_virtual_mfa_device(&state, &input),
            "EnableMFADevice" => operations::mfa::enable_mfa_device(&state, &input),
            "DeactivateMFADevice" => operations::mfa::deactivate_mfa_device(&state, &input),
            "ListMFADevices" => operations::mfa::list_mfa_devices(&state, &input),

            // ── SSH Public Keys ───────────────────────────────────────────────
            "UploadSSHPublicKey" => operations::ssh_keys::upload_ssh_public_key(&state, &input),
            "GetSSHPublicKey" => operations::ssh_keys::get_ssh_public_key(&state, &input),
            "ListSSHPublicKeys" => operations::ssh_keys::list_ssh_public_keys(&state, &input),
            "DeleteSSHPublicKey" => operations::ssh_keys::delete_ssh_public_key(&state, &input),
            "UpdateSSHPublicKey" => operations::ssh_keys::update_ssh_public_key(&state, &input),

            // ── Login Profiles ────────────────────────────────────────────────
            "CreateLoginProfile" => operations::users::create_login_profile(&state, &input),
            "GetLoginProfile" => operations::users::get_login_profile(&state, &input),
            "UpdateLoginProfile" => operations::users::update_login_profile(&state, &input),
            "DeleteLoginProfile" => operations::users::delete_login_profile(&state, &input),

            // ── Misc stubs ────────────────────────────────────────────────────
            "ListServiceSpecificCredentials" => {
                operations::misc::list_service_specific_credentials(&state, &input)
            }
            "ListSigningCertificates" => {
                operations::misc::list_signing_certificates(&state, &input)
            }
            "SimulateCustomPolicy" => {
                operations::misc::simulate_custom_policy(&state, self.authz(), &input)
            }
            "SimulatePrincipalPolicy" => {
                operations::misc::simulate_principal_policy(&state, self.authz(), &input)
            }
            "GetContextKeysForCustomPolicy" => {
                operations::misc::get_context_keys_for_custom_policy(&state, &input)
            }
            "GetContextKeysForPrincipalPolicy" => {
                operations::misc::get_context_keys_for_principal_policy(&state, &input)
            }

            // ── Service-Linked Roles ──────────────────────────────────────────
            "CreateServiceLinkedRole" => {
                operations::service_linked_roles::create_service_linked_role(&state, &input, ctx)
            }
            "DeleteServiceLinkedRole" => {
                operations::service_linked_roles::delete_service_linked_role(&state, &input)
            }
            "GetServiceLinkedRoleDeletionStatus" => {
                operations::service_linked_roles::get_service_linked_role_deletion_status(
                    &state, &input,
                )
            }

            // ── Credential Report ─────────────────────────────────────────────
            "GenerateCredentialReport" => {
                operations::credential_report::generate_credential_report(&state, &input)
            }
            "GetCredentialReport" => {
                operations::credential_report::get_credential_report(&state, &input)
            }

            // ── Service Last Accessed Details ─────────────────────────────────
            "GenerateServiceLastAccessedDetails" => {
                operations::credential_report::generate_service_last_accessed_details(
                    &state, &input,
                )
            }
            "GetServiceLastAccessedDetails" => {
                operations::credential_report::get_service_last_accessed_details(&state, &input)
            }
            "GetServiceLastAccessedDetailsWithEntities" => {
                operations::misc::get_service_last_accessed_details_with_entities(&state, &input)
            }

            // ── Permissions Boundaries ────────────────────────────────────────
            "PutUserPermissionsBoundary" => {
                operations::users::put_user_permissions_boundary(&state, &input)
            }
            "DeleteUserPermissionsBoundary" => {
                operations::users::delete_user_permissions_boundary(&state, &input)
            }
            "PutRolePermissionsBoundary" => {
                operations::roles::put_role_permissions_boundary(&state, &input)
            }
            "DeleteRolePermissionsBoundary" => {
                operations::roles::delete_role_permissions_boundary(&state, &input)
            }

            // ── Access Keys (extended) ────────────────────────────────────────
            "GetAccessKeyLastUsed" => operations::users::get_access_key_last_used(&state, &input),
            "UpdateAccessKey" => operations::users::update_access_key(&state, &input),
            "ChangePassword" => operations::users::change_password(&state, &input),

            // ── Group / Server Certificate updates ────────────────────────────
            "UpdateGroup" => operations::groups::update_group(&state, &input),
            "UpdateServerCertificate" => {
                operations::certificates::update_server_certificate(&state, &input)
            }

            // ── MFA Device extras ─────────────────────────────────────────────
            "GetMFADevice" => operations::mfa::get_mfa_device(&state, &input),
            "ResyncMFADevice" => operations::mfa::resync_mfa_device(&state, &input),
            "TagMFADevice" => operations::mfa::tag_mfa_device(&state, &input),
            "UntagMFADevice" => operations::mfa::untag_mfa_device(&state, &input),
            "ListMFADeviceTags" => operations::mfa::list_mfa_device_tags(&state, &input),

            // ── Signing Certificates ──────────────────────────────────────────
            "UploadSigningCertificate" => {
                operations::misc::upload_signing_certificate(&state, &input)
            }
            "UpdateSigningCertificate" => {
                operations::misc::update_signing_certificate(&state, &input)
            }
            "DeleteSigningCertificate" => {
                operations::misc::delete_signing_certificate(&state, &input)
            }

            // ── Service-Specific Credentials ──────────────────────────────────
            "CreateServiceSpecificCredential" => {
                operations::misc::create_service_specific_credential(&state, &input)
            }
            "DeleteServiceSpecificCredential" => {
                operations::misc::delete_service_specific_credential(&state, &input)
            }
            "ResetServiceSpecificCredential" => {
                operations::misc::reset_service_specific_credential(&state, &input)
            }
            "UpdateServiceSpecificCredential" => {
                operations::misc::update_service_specific_credential(&state, &input)
            }

            // ── Policy lookup helpers ─────────────────────────────────────────
            "ListPoliciesGrantingServiceAccess" => {
                operations::misc::list_policies_granting_service_access(&state, &input)
            }
            "SetSecurityTokenServicePreferences" => {
                operations::misc::set_security_token_service_preferences(&state, &input)
            }
            "GenerateOrganizationsAccessReport" => {
                operations::misc::generate_organizations_access_report(&state, &input)
            }
            "GetOrganizationsAccessReport" => {
                operations::misc::get_organizations_access_report(&state, &input)
            }

            // ── OIDC / SAML provider tags ─────────────────────────────────────
            "TagOpenIDConnectProvider" => {
                operations::tags::tag_open_id_connect_provider(&state, &input)
            }
            "UntagOpenIDConnectProvider" => {
                operations::tags::untag_open_id_connect_provider(&state, &input)
            }
            "ListOpenIDConnectProviderTags" => {
                operations::tags::list_open_id_connect_provider_tags(&state, &input)
            }
            "TagSAMLProvider" => operations::tags::tag_saml_provider(&state, &input),
            "UntagSAMLProvider" => operations::tags::untag_saml_provider(&state, &input),
            "ListSAMLProviderTags" => operations::tags::list_saml_provider_tags(&state, &input),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn iam_action(&self, operation: &str) -> Option<String> {
        Some(format!("iam:{operation}"))
    }

    fn iam_resource(&self, operation: &str, input: &Value, ctx: &RequestContext) -> Option<String> {
        let prefix = format!("arn:aws:iam::{}", ctx.account_id);
        match operation {
            "ListUsers"
            | "ListGroups"
            | "ListRoles"
            | "ListPolicies"
            | "ListInstanceProfiles"
            | "ListAccountAliases"
            | "ListOpenIDConnectProviders"
            | "ListSAMLProviders"
            | "ListServerCertificates"
            | "ListVirtualMFADevices"
            | "GetAccountSummary"
            | "GetAccountPasswordPolicy"
            | "UpdateAccountPasswordPolicy"
            | "DeleteAccountPasswordPolicy"
            | "GetAccountAuthorizationDetails"
            | "GenerateCredentialReport"
            | "GetCredentialReport"
            | "GenerateServiceLastAccessedDetails"
            | "GetServiceLastAccessedDetails"
            | "GetServiceLastAccessedDetailsWithEntities"
            | "SimulateCustomPolicy"
            | "SimulatePrincipalPolicy"
            | "GetContextKeysForCustomPolicy"
            | "GetContextKeysForPrincipalPolicy"
            | "ListServiceSpecificCredentials"
            | "ListSigningCertificates"
            | "CreateAccountAlias"
            | "DeleteAccountAlias" => Some("*".to_string()),
            op if op.contains("User")
                && !op.contains("LoginProfile")
                && !op.contains("AccessKey")
                && !op.contains("SSHPublicKey")
                && !op.contains("MFADevice")
                && !op.contains("ServiceSpecificCredential") =>
            {
                input
                    .get("UserName")
                    .and_then(|v| v.as_str())
                    .map(|n| format!("{prefix}:user/{n}"))
            }
            "CreateLoginProfile"
            | "GetLoginProfile"
            | "UpdateLoginProfile"
            | "DeleteLoginProfile"
            | "CreateAccessKey"
            | "DeleteAccessKey"
            | "ListAccessKeys"
            | "UpdateAccessKey"
            | "GetAccessKeyLastUsed"
            | "ChangePassword"
            | "UploadSSHPublicKey"
            | "GetSSHPublicKey"
            | "ListSSHPublicKeys"
            | "DeleteSSHPublicKey"
            | "UpdateSSHPublicKey"
            | "EnableMFADevice"
            | "DeactivateMFADevice"
            | "ListMFADevices"
            | "PutUserPermissionsBoundary"
            | "DeleteUserPermissionsBoundary"
            | "ListGroupsForUser" => input
                .get("UserName")
                .and_then(|v| v.as_str())
                .map(|n| format!("{prefix}:user/{n}")),
            op if op.contains("Role")
                && !op.contains("InstanceProfile")
                && !op.contains("ServiceLinkedRole") =>
            {
                input
                    .get("RoleName")
                    .and_then(|v| v.as_str())
                    .map(|n| format!("{prefix}:role/{n}"))
            }
            "PutRolePermissionsBoundary" | "DeleteRolePermissionsBoundary" => input
                .get("RoleName")
                .and_then(|v| v.as_str())
                .map(|n| format!("{prefix}:role/{n}")),
            "CreateServiceLinkedRole"
            | "DeleteServiceLinkedRole"
            | "GetServiceLinkedRoleDeletionStatus" => input
                .get("RoleName")
                .and_then(|v| v.as_str())
                .map(|n| format!("{prefix}:role/aws-service-role/{n}"))
                .or(Some("*".to_string())),
            op if op.contains("Group") && !op.contains("ListGroupsForUser") => input
                .get("GroupName")
                .and_then(|v| v.as_str())
                .map(|n| format!("{prefix}:group/{n}")),
            op if op.contains("InstanceProfile") => input
                .get("InstanceProfileName")
                .and_then(|v| v.as_str())
                .map(|n| format!("{prefix}:instance-profile/{n}")),
            "CreatePolicy" => input
                .get("PolicyName")
                .and_then(|v| v.as_str())
                .map(|n| format!("{prefix}:policy/{n}")),
            op if op.contains("Policy") => {
                if let Some(arn) = input.get("PolicyArn").and_then(|v| v.as_str()) {
                    Some(arn.to_string())
                } else if let Some(name) = input.get("PolicyName").and_then(|v| v.as_str()) {
                    Some(format!("{prefix}:policy/{name}"))
                } else {
                    Some("*".to_string())
                }
            }
            op if op.contains("OpenIDConnectProvider") => input
                .get("OpenIDConnectProviderArn")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    input
                        .get("Url")
                        .and_then(|v| v.as_str())
                        .map(|u| format!("{prefix}:oidc-provider/{u}"))
                }),
            op if op.contains("SAMLProvider") => input
                .get("SAMLProviderArn")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    input
                        .get("Name")
                        .and_then(|v| v.as_str())
                        .map(|n| format!("{prefix}:saml-provider/{n}"))
                }),
            op if op.contains("ServerCertificate") => input
                .get("ServerCertificateName")
                .and_then(|v| v.as_str())
                .map(|n| format!("{prefix}:server-certificate/{n}")),
            op if op.contains("VirtualMFADevice") || op.contains("MFADevice") => input
                .get("SerialNumber")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    input
                        .get("VirtualMFADeviceName")
                        .and_then(|v| v.as_str())
                        .map(|n| format!("{prefix}:mfa/{n}"))
                }),
            _ => Some("*".to_string()),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut snapshot = IamStateSnapshot {
            users: vec![],
            groups: vec![],
            roles: vec![],
            policies: vec![],
            instance_profiles: vec![],
            account_aliases: vec![],
            account_password_policy: None,
            oidc_providers: vec![],
            saml_providers: vec![],
            server_certificates: vec![],
            virtual_mfa_devices: vec![],
            login_profiles: vec![],
            signing_certificates: vec![],
            service_specific_credentials: vec![],
            user_permissions_boundaries: vec![],
            role_permissions_boundaries: vec![],
            access_key_last_used: vec![],
            deletion_tasks: vec![],
        };

        for (_, state) in self.store.iter_all() {
            snapshot
                .users
                .extend(state.users.iter().map(|e| e.value().clone()));
            snapshot
                .groups
                .extend(state.groups.iter().map(|e| e.value().clone()));
            snapshot
                .roles
                .extend(state.roles.iter().map(|e| e.value().clone()));
            snapshot
                .policies
                .extend(state.policies.iter().map(|e| e.value().clone()));
            snapshot
                .instance_profiles
                .extend(state.instance_profiles.iter().map(|e| e.value().clone()));
            if let Ok(aliases) = state.account_aliases.lock() {
                snapshot.account_aliases.extend(aliases.clone());
            }
            if let Ok(policy) = state.account_password_policy.lock()
                && snapshot.account_password_policy.is_none()
            {
                snapshot.account_password_policy = policy.clone();
            }
            snapshot
                .oidc_providers
                .extend(state.oidc_providers.iter().map(|e| e.value().clone()));
            snapshot
                .saml_providers
                .extend(state.saml_providers.iter().map(|e| e.value().clone()));
            snapshot
                .server_certificates
                .extend(state.server_certificates.iter().map(|e| e.value().clone()));
            snapshot
                .virtual_mfa_devices
                .extend(state.virtual_mfa_devices.iter().map(|e| e.value().clone()));
            snapshot
                .login_profiles
                .extend(state.login_profiles.iter().map(|e| e.value().clone()));
            snapshot
                .signing_certificates
                .extend(state.signing_certificates.iter().map(|e| e.value().clone()));
            snapshot.service_specific_credentials.extend(
                state
                    .service_specific_credentials
                    .iter()
                    .map(|e| e.value().clone()),
            );
            snapshot.user_permissions_boundaries.extend(
                state
                    .user_permissions_boundaries
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone())),
            );
            snapshot.role_permissions_boundaries.extend(
                state
                    .role_permissions_boundaries
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone())),
            );
            snapshot.access_key_last_used.extend(
                state
                    .access_key_last_used
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone())),
            );
            snapshot
                .deletion_tasks
                .extend(state.deletion_tasks.iter().map(|e| DeletionTask {
                    task_id: e.key().clone(),
                    role_name: e.value().clone(),
                    status: "SUCCEEDED".to_string(),
                }));
        }

        serde_json::to_vec(&snapshot).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snapshot: IamStateSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;

        // IAM is global — always use the "global" region key.
        // Derive the account from the ARN of the first entity, or fall back to default.
        let account_id = snapshot
            .users
            .first()
            .map(|u| {
                // ARN: arn:aws:iam::{account}:user/{name}
                let parts: Vec<&str> = u.arn.splitn(6, ':').collect();
                if parts.len() >= 5 {
                    parts[4].to_string()
                } else {
                    "000000000000".to_string()
                }
            })
            .or_else(|| {
                snapshot.roles.first().map(|r| {
                    let parts: Vec<&str> = r.arn.splitn(6, ':').collect();
                    if parts.len() >= 5 {
                        parts[4].to_string()
                    } else {
                        "000000000000".to_string()
                    }
                })
            })
            .unwrap_or_else(|| "000000000000".to_string());

        let state = self.store.get(&account_id, IAM_REGION);

        for user in snapshot.users {
            state.users.insert(user.user_name.clone(), user);
        }
        for group in snapshot.groups {
            state.groups.insert(group.group_name.clone(), group);
        }
        for role in snapshot.roles {
            state.roles.insert(role.role_name.clone(), role);
        }
        for policy in snapshot.policies {
            state.policies.insert(policy.arn.clone(), policy);
        }
        for ip in snapshot.instance_profiles {
            state
                .instance_profiles
                .insert(ip.instance_profile_name.clone(), ip);
        }
        if !snapshot.account_aliases.is_empty()
            && let Ok(mut aliases) = state.account_aliases.lock()
        {
            *aliases = snapshot.account_aliases;
        }
        if let Ok(mut policy) = state.account_password_policy.lock() {
            *policy = snapshot.account_password_policy;
        }
        for provider in snapshot.oidc_providers {
            state.oidc_providers.insert(provider.arn.clone(), provider);
        }
        for provider in snapshot.saml_providers {
            state.saml_providers.insert(provider.arn.clone(), provider);
        }
        for cert in snapshot.server_certificates {
            state
                .server_certificates
                .insert(cert.server_certificate_name.clone(), cert);
        }
        for device in snapshot.virtual_mfa_devices {
            state
                .virtual_mfa_devices
                .insert(device.serial_number.clone(), device);
        }
        for profile in snapshot.login_profiles {
            state
                .login_profiles
                .insert(profile.user_name.clone(), profile);
        }
        for cert in snapshot.signing_certificates {
            state
                .signing_certificates
                .insert(cert.certificate_id.clone(), cert);
        }
        for cred in snapshot.service_specific_credentials {
            state
                .service_specific_credentials
                .insert(cred.service_specific_credential_id.clone(), cred);
        }
        for (user_name, boundary_arn) in snapshot.user_permissions_boundaries {
            state
                .user_permissions_boundaries
                .insert(user_name, boundary_arn);
        }
        for (role_name, boundary_arn) in snapshot.role_permissions_boundaries {
            state
                .role_permissions_boundaries
                .insert(role_name, boundary_arn);
        }
        for (key_id, last_used) in snapshot.access_key_last_used {
            state.access_key_last_used.insert(key_id, last_used);
        }
        for task in snapshot.deletion_tasks {
            state.deletion_tasks.insert(task.task_id, task.role_name);
        }

        Ok(())
    }
}
