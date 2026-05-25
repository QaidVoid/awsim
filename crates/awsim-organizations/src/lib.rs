pub mod authz;
pub mod error;
mod operations;
pub mod state;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::OrganizationsState;

pub use authz::OrganizationsScpLookup;

pub struct OrganizationsService {
    store: AccountRegionStore<OrganizationsState>,
}

impl OrganizationsService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<OrganizationsState> {
        self.store.clone()
    }
}

impl Default for OrganizationsService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for OrganizationsService {
    fn service_name(&self) -> &str {
        "organizations"
    }

    fn signing_name(&self) -> &str {
        "organizations"
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
        debug!(operation = %operation, "Organizations operation");
        let state = self.store.get(&ctx.account_id, "global");

        match operation {
            "CreateOrganization" => {
                operations::organization::create_organization(&state, &input, ctx)
            }
            "DescribeOrganization" => {
                operations::organization::describe_organization(&state, &input, ctx)
            }
            "CreateAccount" => operations::accounts::create_account(&state, &input, ctx),
            "DescribeAccount" => operations::accounts::describe_account(&state, &input, ctx),
            "ListAccounts" => operations::accounts::list_accounts(&state, &input, ctx),
            "ListAccountsForParent" => {
                operations::accounts::list_accounts_for_parent(&state, &input, ctx)
            }
            "CreateOrganizationalUnit" => operations::ous::create_ou(&state, &input, ctx),
            "DescribeOrganizationalUnit" => operations::ous::describe_ou(&state, &input, ctx),
            "ListOrganizationalUnitsForParent" => {
                operations::ous::list_ous_for_parent(&state, &input, ctx)
            }
            "CreatePolicy" => operations::policies::create_policy(&state, &input, ctx),
            "DescribePolicy" => operations::policies::describe_policy(&state, &input, ctx),
            "ListPolicies" => operations::policies::list_policies(&state, &input, ctx),
            "AttachPolicy" => operations::policies::attach_policy(&state, &input, ctx),
            "DetachPolicy" => operations::policies::detach_policy(&state, &input, ctx),
            "ListRoots" => operations::roots::list_roots(&state, &input, ctx),
            "ListChildren" => operations::roots::list_children(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
