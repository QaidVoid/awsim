mod error;
mod ids;
mod operations;
mod state;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::IamState;

/// IAM is a global service — we use account-only namespacing.
/// The region key is always "global" for IAM state lookups.
const IAM_REGION: &str = "global";

/// The AWSim IAM service handler.
pub struct IamService {
    store: AccountRegionStore<IamState>,
}

impl IamService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<IamState> {
        self.store.get(&ctx.account_id, IAM_REGION)
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

            // Policies (managed)
            "CreatePolicy" => operations::policies::create_policy(&state, &input, ctx),
            "GetPolicy" => operations::policies::get_policy(&state, &input),
            "DeletePolicy" => operations::policies::delete_policy(&state, &input),
            "ListPolicies" => operations::policies::list_policies(&state, &input),

            // Attach/detach managed policies
            "AttachUserPolicy" => operations::policies::attach_user_policy(&state, &input),
            "DetachUserPolicy" => operations::policies::detach_user_policy(&state, &input),
            "AttachRolePolicy" => operations::policies::attach_role_policy(&state, &input),
            "DetachRolePolicy" => operations::policies::detach_role_policy(&state, &input),
            "AttachGroupPolicy" => operations::policies::attach_group_policy(&state, &input),
            "DetachGroupPolicy" => operations::policies::detach_group_policy(&state, &input),

            // Inline policies
            "PutUserPolicy" => operations::policies::put_user_policy(&state, &input),
            "PutRolePolicy" => operations::policies::put_role_policy(&state, &input),
            "PutGroupPolicy" => operations::policies::put_group_policy(&state, &input),

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
            "AddRoleToInstanceProfile" => {
                operations::instance_profiles::add_role_to_instance_profile(&state, &input)
            }
            "RemoveRoleFromInstanceProfile" => {
                operations::instance_profiles::remove_role_from_instance_profile(&state, &input)
            }

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
