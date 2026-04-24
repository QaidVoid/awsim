mod operations;
mod state;

pub use state::SsoAdminState;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

pub struct SsoAdminService {
    store: AccountRegionStore<SsoAdminState>,
}

impl SsoAdminService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for SsoAdminService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for SsoAdminService {
    fn service_name(&self) -> &str {
        "sso"
    }

    fn signing_name(&self) -> &str {
        "sso"
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
        debug!(operation, "SSO Admin request");
        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            "ListInstances" => operations::instances::list_instances(&state, &input, ctx),
            "CreatePermissionSet" => {
                operations::permission_sets::create_permission_set(&state, &input, ctx)
            }
            "DescribePermissionSet" => {
                operations::permission_sets::describe_permission_set(&state, &input, ctx)
            }
            "DeletePermissionSet" => {
                operations::permission_sets::delete_permission_set(&state, &input, ctx)
            }
            "ListPermissionSets" => {
                operations::permission_sets::list_permission_sets(&state, &input, ctx)
            }
            "UpdatePermissionSet" => {
                operations::permission_sets::update_permission_set(&state, &input, ctx)
            }
            "AttachManagedPolicyToPermissionSet" => {
                operations::permission_sets::attach_managed_policy(&state, &input, ctx)
            }
            "DetachManagedPolicyFromPermissionSet" => {
                operations::permission_sets::detach_managed_policy(&state, &input, ctx)
            }
            "ListManagedPoliciesInPermissionSet" => {
                operations::permission_sets::list_managed_policies(&state, &input, ctx)
            }
            "PutInlinePolicyToPermissionSet" => {
                operations::permission_sets::put_inline_policy(&state, &input, ctx)
            }
            "CreateAccountAssignment" => {
                operations::assignments::create_account_assignment(&state, &input, ctx)
            }
            "DescribeAccountAssignmentCreationStatus" => {
                operations::assignments::describe_account_assignment_creation_status(
                    &state, &input, ctx,
                )
            }
            "ListAccountAssignments" => {
                operations::assignments::list_account_assignments(&state, &input, ctx)
            }
            "DeleteAccountAssignment" => {
                operations::assignments::delete_account_assignment(&state, &input, ctx)
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
