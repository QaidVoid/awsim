#![deny(warnings)]

pub mod error;
pub mod federation;
mod identity;
mod jwt;
pub mod keys;
pub mod mock_idp;
pub mod oauth;
mod operations;
pub mod password;
pub mod saml;
mod secret_hash;
mod srp;
pub mod state;
pub mod well_known;

pub use identity::CognitoIdentityService;
pub use oauth::CognitoOAuthState;
pub use operations::email::EVENT_TYPE as EMAIL_EVENT_TYPE;
pub use state::CognitoState;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, LambdaInvoker, Protocol, RequestContext, ServiceHandler, arn,
};
use serde_json::Value;
use state::UserPool;
use tracing::debug;

pub struct CognitoService {
    store: AccountRegionStore<CognitoState>,
    /// Synchronous Lambda invoker for the user-pool triggers that consume a
    /// response (PreTokenGeneration today; the custom-auth triggers later).
    /// `None` in unit tests and any context without Lambda wired up, in which
    /// case trigger application is skipped.
    lambda_invoker: Option<Arc<dyn LambdaInvoker>>,
}

impl CognitoService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
            lambda_invoker: None,
        }
    }

    /// Attach the synchronous Lambda invoker used to run user-pool triggers.
    pub fn with_lambda_invoker(mut self, invoker: Arc<dyn LambdaInvoker>) -> Self {
        self.lambda_invoker = Some(invoker);
        self
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<CognitoState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }

    /// Return the `Arc<CognitoState>` for a given account+region so the OAuth
    /// router can share the same user-pool state without needing a store clone.
    pub fn state_for(&self, account_id: &str, region: &str) -> Arc<CognitoState> {
        self.store.get(account_id, region)
    }
}

impl Default for CognitoService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for CognitoService {
    fn service_name(&self) -> &str {
        "cognito-idp"
    }

    fn signing_name(&self) -> &str {
        "cognito-idp"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_1
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "Cognito request");
        let state = self.get_state(ctx);
        // Make the synchronous Lambda invoker reachable from the trigger-
        // consuming operations without threading it through every helper.
        if let Some(invoker) = &self.lambda_invoker {
            state.lambda_invoker.set(invoker.clone());
        }

        match operation {
            // User Pools
            "CreateUserPool" => operations::pools::create_user_pool(&state, &input, ctx),
            "DeleteUserPool" => operations::pools::delete_user_pool(&state, &input, ctx),
            "DescribeUserPool" => operations::pools::describe_user_pool(&state, &input, ctx),
            "ListUserPools" => operations::pools::list_user_pools(&state, &input, ctx),

            // User Pool Clients
            "CreateUserPoolClient" => {
                operations::pools::create_user_pool_client(&state, &input, ctx)
            }
            "DescribeUserPoolClient" => {
                operations::pools::describe_user_pool_client(&state, &input, ctx)
            }
            "DeleteUserPoolClient" => {
                operations::pools::delete_user_pool_client(&state, &input, ctx)
            }

            // User management
            "SignUp" => operations::users::sign_up(&state, &input, ctx),
            "ConfirmSignUp" => operations::users::confirm_sign_up(&state, &input, ctx),
            "AdminConfirmSignUp" => operations::users::admin_confirm_sign_up(&state, &input, ctx),
            "AdminCreateUser" => operations::users::admin_create_user(&state, &input, ctx),
            "AdminDeleteUser" => operations::users::admin_delete_user(&state, &input, ctx),
            "AdminGetUser" => operations::users::admin_get_user(&state, &input, ctx),
            "AdminSetUserPassword" => {
                operations::users::admin_set_user_password(&state, &input, ctx)
            }
            "ListUsers" => operations::users::list_users(&state, &input, ctx),
            "GetUser" => operations::users::get_user(&state, &input, ctx),

            // Password flows
            "ForgotPassword" => operations::users::forgot_password(&state, &input, ctx),
            "ConfirmForgotPassword" => {
                operations::users::confirm_forgot_password(&state, &input, ctx)
            }
            "ChangePassword" => operations::users::change_password(&state, &input, ctx),
            "GlobalSignOut" => operations::users::global_sign_out(&state, &input, ctx),

            // Auth flows
            "InitiateAuth" => operations::auth::initiate_auth(&state, &input, ctx),
            "AdminInitiateAuth" => operations::auth::admin_initiate_auth(&state, &input, ctx),
            "RespondToAuthChallenge" => {
                operations::auth::respond_to_auth_challenge(&state, &input, ctx)
            }
            "AdminRespondToAuthChallenge" => {
                operations::auth::admin_respond_to_auth_challenge(&state, &input, ctx)
            }

            // MFA configuration
            "SetUserPoolMfaConfig" => {
                operations::mfa::set_user_pool_mfa_config(&state, &input, ctx)
            }
            "GetUserPoolMfaConfig" => {
                operations::mfa::get_user_pool_mfa_config(&state, &input, ctx)
            }
            "AssociateSoftwareToken" => {
                operations::mfa::associate_software_token(&state, &input, ctx)
            }
            "VerifySoftwareToken" => operations::mfa::verify_software_token(&state, &input, ctx),
            "SetUserMFAPreference" => operations::mfa::set_user_mfa_preference(&state, &input, ctx),
            "AdminSetUserMFAPreference" => {
                operations::mfa::admin_set_user_mfa_preference(&state, &input, ctx)
            }

            // User Pool - additional config
            "UpdateUserPool" => operations::pools::update_user_pool(&state, &input, ctx),
            "ListUserPoolClients" => operations::pools::list_user_pool_clients(&state, &input, ctx),
            "UpdateUserPoolClient" => {
                operations::pools::update_user_pool_client(&state, &input, ctx)
            }
            "AddCustomAttributes" => operations::pools::add_custom_attributes(&state, &input, ctx),

            // Groups
            "CreateGroup" => operations::groups::create_group(&state, &input, ctx),
            "GetGroup" => operations::groups::get_group(&state, &input, ctx),
            "UpdateGroup" => operations::groups::update_group(&state, &input, ctx),
            "DeleteGroup" => operations::groups::delete_group(&state, &input, ctx),
            "ListGroups" => operations::groups::list_groups(&state, &input, ctx),
            "AdminAddUserToGroup" => {
                operations::groups::admin_add_user_to_group(&state, &input, ctx)
            }
            "AdminRemoveUserFromGroup" => {
                operations::groups::admin_remove_user_from_group(&state, &input, ctx)
            }
            "AdminListGroupsForUser" => {
                operations::groups::admin_list_groups_for_user(&state, &input, ctx)
            }
            "ListUsersInGroup" => operations::groups::list_users_in_group(&state, &input, ctx),

            // Additional user management
            "AdminEnableUser" => operations::users::admin_enable_user(&state, &input, ctx),
            "AdminDisableUser" => operations::users::admin_disable_user(&state, &input, ctx),
            "AdminResetUserPassword" => {
                operations::users::admin_reset_user_password(&state, &input, ctx)
            }
            "AdminUpdateUserAttributes" => {
                operations::users::admin_update_user_attributes(&state, &input, ctx)
            }
            "AdminDeleteUserAttributes" => {
                operations::users::admin_delete_user_attributes(&state, &input, ctx)
            }
            "UpdateUserAttributes" => {
                operations::users::update_user_attributes(&state, &input, ctx)
            }
            "DeleteUserAttributes" => {
                operations::users::delete_user_attributes(&state, &input, ctx)
            }
            "DeleteUser" => operations::users::delete_user(&state, &input, ctx),
            "ResendConfirmationCode" => {
                operations::users::resend_confirmation_code(&state, &input, ctx)
            }
            "GetUserAttributeVerificationCode" => {
                operations::users::get_user_attribute_verification_code(&state, &input, ctx)
            }
            "VerifyUserAttribute" => operations::users::verify_user_attribute(&state, &input, ctx),
            "AdminUserGlobalSignOut" => {
                operations::users::admin_user_global_sign_out(&state, &input, ctx)
            }
            "RevokeToken" => operations::users::revoke_token(&state, &input, ctx),
            "AdminListUserAuthEvents" => {
                operations::users::admin_list_user_auth_events(&state, &input, ctx)
            }

            // Resource Servers
            "CreateResourceServer" => {
                operations::resource_servers::create_resource_server(&state, &input, ctx)
            }
            "DescribeResourceServer" => {
                operations::resource_servers::describe_resource_server(&state, &input, ctx)
            }
            "UpdateResourceServer" => {
                operations::resource_servers::update_resource_server(&state, &input, ctx)
            }
            "DeleteResourceServer" => {
                operations::resource_servers::delete_resource_server(&state, &input, ctx)
            }
            "ListResourceServers" => {
                operations::resource_servers::list_resource_servers(&state, &input, ctx)
            }

            // Identity Providers
            "CreateIdentityProvider" => {
                operations::identity_providers::create_identity_provider(&state, &input, ctx)
            }
            "DescribeIdentityProvider" => {
                operations::identity_providers::describe_identity_provider(&state, &input, ctx)
            }
            "UpdateIdentityProvider" => {
                operations::identity_providers::update_identity_provider(&state, &input, ctx)
            }
            "DeleteIdentityProvider" => {
                operations::identity_providers::delete_identity_provider(&state, &input, ctx)
            }
            "ListIdentityProviders" => {
                operations::identity_providers::list_identity_providers(&state, &input, ctx)
            }
            "GetIdentityProviderByIdentifier" => {
                operations::identity_providers::get_identity_provider_by_identifier(
                    &state, &input, ctx,
                )
            }

            // Domain management
            "CreateUserPoolDomain" => {
                operations::domain::create_user_pool_domain(&state, &input, ctx)
            }
            "DescribeUserPoolDomain" => {
                operations::domain::describe_user_pool_domain(&state, &input, ctx)
            }
            "DeleteUserPoolDomain" => {
                operations::domain::delete_user_pool_domain(&state, &input, ctx)
            }

            // Tags
            "TagResource" => operations::tags::tag_resource(&state, &input, ctx),
            "UntagResource" => operations::tags::untag_resource(&state, &input, ctx),
            "ListTagsForResource" => operations::tags::list_tags_for_resource(&state, &input, ctx),

            // Device tracking (user-level)
            "ConfirmDevice" => operations::devices::confirm_device(&state, &input, ctx),
            "GetDevice" => operations::devices::get_device(&state, &input, ctx),
            "ListDevices" => operations::devices::list_devices(&state, &input, ctx),
            "UpdateDeviceStatus" => operations::devices::update_device_status(&state, &input, ctx),
            "ForgetDevice" => operations::devices::forget_device(&state, &input, ctx),

            // Device tracking (admin)
            "AdminGetDevice" => operations::devices::admin_get_device(&state, &input, ctx),
            "AdminListDevices" => operations::devices::admin_list_devices(&state, &input, ctx),
            "AdminUpdateDeviceStatus" => {
                operations::devices::admin_update_device_status(&state, &input, ctx)
            }
            "AdminForgetDevice" => operations::devices::admin_forget_device(&state, &input, ctx),

            // UI Customization & Branding
            "SetUICustomization" => operations::branding::set_ui_customization(&state, &input, ctx),
            "GetUICustomization" => operations::branding::get_ui_customization(&state, &input, ctx),
            "CreateManagedLoginBranding" => {
                operations::branding::create_managed_login_branding(&state, &input, ctx)
            }
            "DescribeManagedLoginBranding" => {
                operations::branding::describe_managed_login_branding(&state, &input, ctx)
            }
            "DescribeManagedLoginBrandingByClient" => {
                operations::branding::describe_managed_login_branding_by_client(&state, &input, ctx)
            }
            "UpdateManagedLoginBranding" => {
                operations::branding::update_managed_login_branding(&state, &input, ctx)
            }
            "DeleteManagedLoginBranding" => {
                operations::branding::delete_managed_login_branding(&state, &input, ctx)
            }

            // Risk Configuration
            "SetRiskConfiguration" => operations::risk::set_risk_configuration(&state, &input, ctx),
            "DescribeRiskConfiguration" => {
                operations::risk::describe_risk_configuration(&state, &input, ctx)
            }
            "UpdateAuthEventFeedback" => {
                operations::risk::update_auth_event_feedback(&state, &input, ctx)
            }
            "AdminUpdateAuthEventFeedback" => {
                operations::risk::admin_update_auth_event_feedback(&state, &input, ctx)
            }

            // Provider linking
            "AdminLinkProviderForUser" => {
                operations::identity_providers::admin_link_provider_for_user(&state, &input, ctx)
            }
            "AdminDisableProviderForUser" => {
                operations::identity_providers::admin_disable_provider_for_user(&state, &input, ctx)
            }

            // User import jobs
            "CreateUserImportJob" => {
                operations::import::create_user_import_job(&state, &input, ctx)
            }
            "DescribeUserImportJob" => {
                operations::import::describe_user_import_job(&state, &input, ctx)
            }
            "StartUserImportJob" => operations::import::start_user_import_job(&state, &input, ctx),
            "StopUserImportJob" => operations::import::stop_user_import_job(&state, &input, ctx),
            "ListUserImportJobs" => operations::import::list_user_import_jobs(&state, &input, ctx),
            "GetCSVHeader" => operations::import::get_csv_header(&state, &input, ctx),

            // Domain management (additional)
            "UpdateUserPoolDomain" => {
                operations::domain::update_user_pool_domain(&state, &input, ctx)
            }

            // Pool-level misc
            "GetSigningCertificate" => {
                operations::pools::get_signing_certificate(&state, &input, ctx)
            }
            "GetLogDeliveryConfiguration" => {
                operations::pools::get_log_delivery_configuration(&state, &input, ctx)
            }
            "SetLogDeliveryConfiguration" => {
                operations::pools::set_log_delivery_configuration(&state, &input, ctx)
            }

            // Auth misc
            "GetTokensFromRefreshToken" => {
                operations::auth::get_tokens_from_refresh_token(&state, &input, ctx)
            }
            "GetUserAuthFactors" => operations::auth::get_user_auth_factors(&state, &input, ctx),

            // Additional client secrets
            "AddUserPoolClientSecret" => {
                operations::client_secrets::add_user_pool_client_secret(&state, &input, ctx)
            }
            "DeleteUserPoolClientSecret" => {
                operations::client_secrets::delete_user_pool_client_secret(&state, &input, ctx)
            }
            "ListUserPoolClientSecrets" => {
                operations::client_secrets::list_user_pool_client_secrets(&state, &input, ctx)
            }

            // Legacy MFA settings
            "AdminSetUserSettings" => {
                operations::user_settings::admin_set_user_settings(&state, &input, ctx)
            }
            "SetUserSettings" => operations::user_settings::set_user_settings(&state, &input, ctx),

            // WebAuthn
            "StartWebAuthnRegistration" => {
                operations::webauthn::start_webauthn_registration(&state, &input, ctx)
            }
            "CompleteWebAuthnRegistration" => {
                operations::webauthn::complete_webauthn_registration(&state, &input, ctx)
            }
            "DeleteWebAuthnCredential" => {
                operations::webauthn::delete_webauthn_credential(&state, &input, ctx)
            }
            "ListWebAuthnCredentials" => {
                operations::webauthn::list_webauthn_credentials(&state, &input, ctx)
            }

            // Terms
            "CreateTerms" => operations::terms::create_terms(&state, &input, ctx),
            "UpdateTerms" => operations::terms::update_terms(&state, &input, ctx),
            "DeleteTerms" => operations::terms::delete_terms(&state, &input, ctx),
            "DescribeTerms" => operations::terms::describe_terms(&state, &input, ctx),
            "ListTerms" => operations::terms::list_terms(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    /// Declare IAM actions only for Cognito's management plane. The
    /// public user-plane operations (`SignUp`, `InitiateAuth`,
    /// `GetUser`, password/MFA/device/WebAuthn self-service, refresh and
    /// revoke) are authorized by the app client and the caller's access
    /// token rather than SigV4 credentials, exactly as in real Cognito.
    /// Returning `None` for those leaves them ungated so end-user auth
    /// flows keep working when `AWSIM_IAM_ENFORCE` is on; the gateway
    /// only runs `authz.check` when both this and `iam_resource` are
    /// `Some`.
    fn iam_action(&self, operation: &str) -> Option<String> {
        match operation {
            // User pools, clients, and pool-level configuration.
            "CreateUserPool"
            | "DeleteUserPool"
            | "DescribeUserPool"
            | "ListUserPools"
            | "UpdateUserPool"
            | "AddCustomAttributes"
            | "CreateUserPoolClient"
            | "DescribeUserPoolClient"
            | "DeleteUserPoolClient"
            | "ListUserPoolClients"
            | "UpdateUserPoolClient"
            | "AddUserPoolClientSecret"
            | "DeleteUserPoolClientSecret"
            | "ListUserPoolClientSecrets"
            | "GetSigningCertificate"
            | "GetLogDeliveryConfiguration"
            | "SetLogDeliveryConfiguration"
            // MFA configuration (pool-level).
            | "SetUserPoolMfaConfig"
            | "GetUserPoolMfaConfig"
            // Admin user management.
            | "AdminConfirmSignUp"
            | "AdminCreateUser"
            | "AdminDeleteUser"
            | "AdminGetUser"
            | "AdminSetUserPassword"
            | "AdminInitiateAuth"
            | "AdminRespondToAuthChallenge"
            | "AdminSetUserMFAPreference"
            | "AdminEnableUser"
            | "AdminDisableUser"
            | "AdminResetUserPassword"
            | "AdminUpdateUserAttributes"
            | "AdminDeleteUserAttributes"
            | "AdminUserGlobalSignOut"
            | "AdminListUserAuthEvents"
            | "AdminUpdateAuthEventFeedback"
            | "AdminSetUserSettings"
            | "ListUsers"
            // Groups.
            | "CreateGroup"
            | "GetGroup"
            | "UpdateGroup"
            | "DeleteGroup"
            | "ListGroups"
            | "ListUsersInGroup"
            | "AdminAddUserToGroup"
            | "AdminRemoveUserFromGroup"
            | "AdminListGroupsForUser"
            // Resource servers.
            | "CreateResourceServer"
            | "DescribeResourceServer"
            | "UpdateResourceServer"
            | "DeleteResourceServer"
            | "ListResourceServers"
            // Identity providers and provider linking.
            | "CreateIdentityProvider"
            | "DescribeIdentityProvider"
            | "UpdateIdentityProvider"
            | "DeleteIdentityProvider"
            | "ListIdentityProviders"
            | "GetIdentityProviderByIdentifier"
            | "AdminLinkProviderForUser"
            | "AdminDisableProviderForUser"
            // Domains.
            | "CreateUserPoolDomain"
            | "DescribeUserPoolDomain"
            | "DeleteUserPoolDomain"
            | "UpdateUserPoolDomain"
            // Tags.
            | "TagResource"
            | "UntagResource"
            | "ListTagsForResource"
            // Admin device tracking.
            | "AdminGetDevice"
            | "AdminListDevices"
            | "AdminUpdateDeviceStatus"
            | "AdminForgetDevice"
            // UI customization and branding.
            | "SetUICustomization"
            | "GetUICustomization"
            | "CreateManagedLoginBranding"
            | "DescribeManagedLoginBranding"
            | "DescribeManagedLoginBrandingByClient"
            | "UpdateManagedLoginBranding"
            | "DeleteManagedLoginBranding"
            // Risk configuration.
            | "SetRiskConfiguration"
            | "DescribeRiskConfiguration"
            // User import jobs.
            | "CreateUserImportJob"
            | "DescribeUserImportJob"
            | "StartUserImportJob"
            | "StopUserImportJob"
            | "ListUserImportJobs"
            | "GetCSVHeader"
            // Terms.
            | "CreateTerms"
            | "UpdateTerms"
            | "DeleteTerms"
            | "DescribeTerms"
            | "ListTerms" => Some(format!("cognito-idp:{operation}")),
            _ => None,
        }
    }

    /// Resource ARN for the management-plane operations declared in
    /// [`Self::iam_action`]. Kept in lockstep with `iam_action` via the
    /// early `?`: user-plane operations return `None` here too, so the
    /// gateway's `(Some, Some)` guard never fires for them.
    fn iam_resource(&self, operation: &str, input: &Value, ctx: &RequestContext) -> Option<String> {
        self.iam_action(operation)?;

        // Tag operations carry the target ARN directly.
        if let Some(resource_arn) = input.get("ResourceArn").and_then(|v| v.as_str()) {
            return Some(resource_arn.to_string());
        }

        // Account-level operations with no specific pool.
        if matches!(operation, "CreateUserPool" | "ListUserPools") {
            return Some("*".to_string());
        }

        // Everything else scopes to a single user pool; fall back to the
        // account wildcard if the request omits the pool id.
        match input.get("UserPoolId").and_then(|v| v.as_str()) {
            Some(pool_id) => Some(arn::build(
                ctx,
                "cognito-idp",
                format!("userpool/{pool_id}"),
            )),
            None => Some("*".to_string()),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let entries = self.store.iter_all();
        let snap: Vec<(String, String, CognitoSnapshot)> = entries
            .into_iter()
            .map(|((account, region), state)| {
                let pools: HashMap<String, UserPool> = state
                    .user_pools
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect();
                let domains: HashMap<String, String> = state
                    .domain_pool_map
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect();
                let resource_tags: HashMap<String, HashMap<String, String>> = state
                    .resource_tags
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect();
                (
                    account,
                    region,
                    CognitoSnapshot {
                        pools,
                        domains,
                        resource_tags,
                    },
                )
            })
            .collect();
        serde_json::to_vec(&snap).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: Vec<(String, String, CognitoSnapshot)> =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;
        for (account, region, cs) in snap {
            let state = self.store.get(&account, &region);
            state.user_pools.clear();
            state.domain_pool_map.clear();
            state.resource_tags.clear();
            state.mfa_sessions.clear();
            state.confirmation_codes.clear();
            state.confirmation_codes_issued.clear();
            state.srp_sessions.clear();
            for (id, mut pool) in cs.pools {
                // Back-compat: snapshots written before schema bootstrap
                // landed have an empty `schema`. Repopulate with the
                // standard OIDC attrs so attribute-write paths can
                // validate against them. Custom attrs that were already
                // set on users are unaffected.
                if pool.schema.is_empty() {
                    pool.schema = crate::state::default_user_pool_schema();
                }
                state.user_pools.insert(id, pool);
            }
            for (domain, pool_id) in cs.domains {
                state.domain_pool_map.insert(domain, pool_id);
            }
            for (arn, tags) in cs.resource_tags {
                state.resource_tags.insert(arn, tags);
            }
        }
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CognitoSnapshot {
    pools: HashMap<String, UserPool>,
    domains: HashMap<String, String>,
    resource_tags: HashMap<String, HashMap<String, String>>,
}

#[cfg(test)]
mod authz_tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> RequestContext {
        RequestContext::new("cognito-idp", "us-east-1")
    }

    #[test]
    fn management_ops_declare_action_and_pool_resource() {
        let svc = CognitoService::new();
        let input = json!({ "UserPoolId": "us-east-1_pool" });
        for op in [
            "CreateUserPoolClient",
            "AdminCreateUser",
            "ListUsers",
            "SetUserPoolMfaConfig",
            "AdminInitiateAuth",
        ] {
            assert_eq!(
                svc.iam_action(op),
                Some(format!("cognito-idp:{op}")),
                "{op} should be IAM-gated"
            );
            assert_eq!(
                svc.iam_resource(op, &input, &ctx()),
                Some(
                    "arn:aws:cognito-idp:us-east-1:000000000000:userpool/us-east-1_pool"
                        .to_string()
                ),
                "{op} should scope to the user pool ARN"
            );
        }
    }

    #[test]
    fn public_user_plane_ops_are_ungated() {
        let svc = CognitoService::new();
        let input = json!({ "UserPoolId": "us-east-1_pool" });
        // These are authorized by the app client / access token, never
        // SigV4, so the gateway must skip its IAM check for them.
        for op in [
            "SignUp",
            "ConfirmSignUp",
            "InitiateAuth",
            "RespondToAuthChallenge",
            "GetUser",
            "ChangePassword",
            "ForgotPassword",
            "ConfirmForgotPassword",
            "GlobalSignOut",
            "RevokeToken",
            "AssociateSoftwareToken",
            "ConfirmDevice",
            "StartWebAuthnRegistration",
            "GetTokensFromRefreshToken",
        ] {
            assert_eq!(svc.iam_action(op), None, "{op} must stay ungated");
            assert_eq!(svc.iam_resource(op, &input, &ctx()), None);
        }
    }

    #[test]
    fn account_level_and_tag_ops_resolve_special_resources() {
        let svc = CognitoService::new();
        // No specific pool -> account wildcard.
        for op in ["CreateUserPool", "ListUserPools"] {
            assert_eq!(
                svc.iam_resource(op, &json!({}), &ctx()),
                Some("*".to_string())
            );
        }
        // Tag ops carry the target ARN directly.
        let tag_arn = "arn:aws:cognito-idp:us-east-1:000000000000:userpool/us-east-1_tagged";
        assert_eq!(
            svc.iam_resource("TagResource", &json!({ "ResourceArn": tag_arn }), &ctx()),
            Some(tag_arn.to_string())
        );
    }
}
