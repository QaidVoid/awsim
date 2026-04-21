mod identity;
mod jwt;
mod operations;
pub mod state;
pub mod oauth;

pub use identity::CognitoIdentityService;
pub use oauth::CognitoOAuthState;
pub use state::CognitoState;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

pub struct CognitoService {
    store: AccountRegionStore<CognitoState>,
}

impl CognitoService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
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
            "VerifySoftwareToken" => {
                operations::mfa::verify_software_token(&state, &input, ctx)
            }
            "SetUserMFAPreference" => {
                operations::mfa::set_user_mfa_preference(&state, &input, ctx)
            }
            "AdminSetUserMFAPreference" => {
                operations::mfa::admin_set_user_mfa_preference(&state, &input, ctx)
            }

            // User Pool - additional config
            "UpdateUserPool" => operations::pools::update_user_pool(&state, &input, ctx),
            "ListUserPoolClients" => {
                operations::pools::list_user_pool_clients(&state, &input, ctx)
            }
            "UpdateUserPoolClient" => {
                operations::pools::update_user_pool_client(&state, &input, ctx)
            }
            "AddCustomAttributes" => {
                operations::pools::add_custom_attributes(&state, &input, ctx)
            }

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
            "ListUsersInGroup" => {
                operations::groups::list_users_in_group(&state, &input, ctx)
            }

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
            "VerifyUserAttribute" => {
                operations::users::verify_user_attribute(&state, &input, ctx)
            }
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

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
