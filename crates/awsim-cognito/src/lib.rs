mod jwt;
mod operations;
mod state;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::CognitoState;

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

            // Groups
            "CreateGroup" => operations::groups::create_group(&state, &input, ctx),
            "AdminAddUserToGroup" => {
                operations::groups::admin_add_user_to_group(&state, &input, ctx)
            }
            "AdminListGroupsForUser" => {
                operations::groups::admin_list_groups_for_user(&state, &input, ctx)
            }

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
