mod operations;
mod state;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use state::SesState;

pub struct SesService {
    store: AccountRegionStore<SesState>,
}

impl SesService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<SesState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for SesService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for SesService {
    fn service_name(&self) -> &str {
        "email"
    }

    fn signing_name(&self) -> &str {
        "ses"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/email/outbound-emails",
                operation: "SendEmail",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/email/identities",
                operation: "CreateEmailIdentity",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/identities",
                operation: "ListEmailIdentities",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/identities/{EmailIdentity}",
                operation: "GetEmailIdentity",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v2/email/identities/{EmailIdentity}",
                operation: "DeleteEmailIdentity",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/email/templates",
                operation: "CreateEmailTemplate",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/templates",
                operation: "ListEmailTemplates",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/templates/{TemplateName}",
                operation: "GetEmailTemplate",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v2/email/templates/{TemplateName}",
                operation: "DeleteEmailTemplate",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/account",
                operation: "GetAccount",
                required_query_param: None,
            },
        ]
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "SES request");
        let state = self.get_state(ctx);

        match operation {
            "SendEmail" => operations::emails::send_email(&state, &input, ctx),
            "CreateEmailIdentity" => {
                operations::identities::create_email_identity(&state, &input, ctx)
            }
            "DeleteEmailIdentity" => {
                operations::identities::delete_email_identity(&state, &input, ctx)
            }
            "GetEmailIdentity" => operations::identities::get_email_identity(&state, &input, ctx),
            "ListEmailIdentities" => {
                operations::identities::list_email_identities(&state, &input, ctx)
            }
            "CreateEmailTemplate" => {
                operations::templates::create_email_template(&state, &input, ctx)
            }
            "DeleteEmailTemplate" => {
                operations::templates::delete_email_template(&state, &input, ctx)
            }
            "GetEmailTemplate" => operations::templates::get_email_template(&state, &input, ctx),
            "ListEmailTemplates" => {
                operations::templates::list_email_templates(&state, &input, ctx)
            }
            "GetAccount" => operations::account::get_account(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
