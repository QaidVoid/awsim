mod operations;
mod sqlite_store;
mod state;

pub use sqlite_store::{SentEmailRow, SqliteStore};
pub use state::SentEmail;

use std::path::Path;
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
    sqlite_store: Arc<SqliteStore>,
    /// Holds the per-process tempdir when running without
    /// `--data-dir` so the `.db` files are removed on graceful
    /// shutdown via Drop.
    _tempdir: Option<tempfile::TempDir>,
}

impl SesService {
    /// Ephemeral in-process store. Files live in a `TempDir`
    /// cleaned up on Drop.
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("awsim-ses-")
            .tempdir()
            .expect("creating ephemeral SES tempdir should not fail");
        let path = dir.path().join("ses.db");
        let sqlite_store = Arc::new(
            SqliteStore::open(&path).expect("opening ephemeral SES sqlite store should not fail"),
        );
        Self {
            store: AccountRegionStore::new(),
            sqlite_store,
            _tempdir: Some(dir),
        }
    }

    /// Persistent store rooted at `{dir}/ses.db`.
    pub fn with_data_dir(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir)
            .unwrap_or_else(|e| panic!("creating SES data dir {} failed: {e}", dir.display()));
        let path = dir.join("ses.db");
        let sqlite_store = Arc::new(SqliteStore::open(&path).unwrap_or_else(|e| {
            panic!(
                "opening persistent SES sqlite store at {} failed: {e}",
                path.display()
            )
        }));
        Self {
            store: AccountRegionStore::new(),
            sqlite_store,
            _tempdir: None,
        }
    }

    /// Tempdir path for the awsim binary's shutdown cleanup.
    pub fn tempdir_path(&self) -> Option<&Path> {
        self._tempdir.as_ref().map(|d| d.path())
    }

    /// Internal Arc to the sqlite store — exposed so the awsim
    /// binary's `/_awsim/storage/sqlite` endpoint can surface row
    /// counts + file size, and so the retention sweep can run.
    pub fn sqlite_store_handle(&self) -> Option<Arc<SqliteStore>> {
        Some(Arc::clone(&self.sqlite_store))
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<SesState> {
        let state = self.store.get(&ctx.account_id, &ctx.region);
        state.set_sqlite(Arc::clone(&self.sqlite_store));
        state
    }

    /// AWS lets a sender call SendEmail with a `SourceArn` that lives
    /// in another account, but only when the target identity's
    /// resource policy grants `ses:SendEmail` to the caller. Mirror
    /// that: parse the ARN, locate the foreign-account identity, walk
    /// its identity policies, and reject with `AccessDenied` when no
    /// statement allows the action. Same-account ARNs and missing
    /// `SourceArn` pass through.
    fn enforce_cross_account_source_arn(
        &self,
        input: &Value,
        ctx: &RequestContext,
    ) -> Result<(), AwsError> {
        let Some(source_arn) = input.get("SourceArn").and_then(Value::as_str) else {
            return Ok(());
        };
        if source_arn.is_empty() {
            return Ok(());
        }
        let Some(parsed) = parse_ses_identity_arn(source_arn) else {
            return Err(AwsError::bad_request(
                "InvalidParameter",
                format!("SourceArn `{source_arn}` is not a valid SES identity ARN."),
            ));
        };
        if parsed.account == ctx.account_id {
            return Ok(());
        }
        let foreign = self.store.get(&parsed.account, &parsed.region);
        let policies = foreign
            .identity_policies
            .get(&parsed.identity)
            .map(|m| m.clone())
            .unwrap_or_default();
        if policies.is_empty() {
            return Err(AwsError::access_denied_for(
                "ses:SendEmail",
                &ctx.account_id,
                source_arn,
            ));
        }
        let allowed = policies
            .values()
            .any(|doc| identity_policy_allows_send(doc));
        if !allowed {
            return Err(AwsError::access_denied_for(
                "ses:SendEmail",
                &ctx.account_id,
                source_arn,
            ));
        }
        Ok(())
    }

    /// Snapshot every sent email across all accounts/regions, newest
    /// first. Reads straight from SQLite — survives restarts.
    pub fn list_sent_emails(&self) -> Vec<(String, String, SentEmail)> {
        match self.sqlite_store.list_all() {
            Ok(rows) => rows
                .into_iter()
                .map(|r| (r.account, r.region, r.email))
                .collect(),
            Err(e) => {
                tracing::warn!(error = %e.message, "SES list_sent_emails failed");
                Vec::new()
            }
        }
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
                path_pattern: "/v2/email/outbound-bulk-emails",
                operation: "SendBulkEmail",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/email/outbound-custom-verification-emails",
                operation: "SendCustomVerificationEmail",
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
                method: "PUT",
                path_pattern: "/v2/email/identities/{EmailIdentity}/dkim",
                operation: "PutEmailIdentityDkimAttributes",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v2/email/identities/{EmailIdentity}/dkim/signing",
                operation: "PutEmailIdentityDkimSigningAttributes",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v2/email/identities/{EmailIdentity}/configuration-set",
                operation: "PutEmailIdentityConfigurationSetAttributes",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v2/email/identities/{EmailIdentity}/mail-from",
                operation: "PutEmailIdentityMailFromAttributes",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v2/email/identities/{EmailIdentity}/feedback",
                operation: "PutEmailIdentityFeedbackAttributes",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/email/identities/{EmailIdentity}/policies/{PolicyName}",
                operation: "CreateEmailIdentityPolicy",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v2/email/identities/{EmailIdentity}/policies/{PolicyName}",
                operation: "DeleteEmailIdentityPolicy",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v2/email/identities/{EmailIdentity}/policies/{PolicyName}",
                operation: "UpdateEmailIdentityPolicy",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/identities/{EmailIdentity}/policies",
                operation: "GetEmailIdentityPolicies",
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
                method: "PUT",
                path_pattern: "/v2/email/templates/{TemplateName}",
                operation: "UpdateEmailTemplate",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/account",
                operation: "GetAccount",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v2/email/account/sending",
                operation: "PutAccountSendingAttributes",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v2/email/account/suppression",
                operation: "PutAccountSuppressionAttributes",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v2/email/account/dedicated-ips/warmup",
                operation: "PutAccountDedicatedIpWarmupAttributes",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v2/email/account/vdm",
                operation: "PutAccountVdmAttributes",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/email/configuration-sets",
                operation: "CreateConfigurationSet",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/configuration-sets",
                operation: "ListConfigurationSets",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/configuration-sets/{ConfigurationSetName}",
                operation: "GetConfigurationSet",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v2/email/configuration-sets/{ConfigurationSetName}",
                operation: "DeleteConfigurationSet",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v2/email/configuration-sets/{ConfigurationSetName}/reputation-options",
                operation: "PutConfigurationSetReputationOptions",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v2/email/configuration-sets/{ConfigurationSetName}/delivery-options",
                operation: "PutConfigurationSetDeliveryOptions",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v2/email/configuration-sets/{ConfigurationSetName}/vdm-options",
                operation: "PutConfigurationSetVdmOptions",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/email/configuration-sets/{ConfigurationSetName}/event-destinations",
                operation: "CreateConfigurationSetEventDestination",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/configuration-sets/{ConfigurationSetName}/event-destinations",
                operation: "GetConfigurationSetEventDestinations",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v2/email/configuration-sets/{ConfigurationSetName}/event-destinations/{EventDestinationName}",
                operation: "DeleteConfigurationSetEventDestination",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/email/dedicated-ip-pools",
                operation: "CreateDedicatedIpPool",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/dedicated-ip-pools",
                operation: "ListDedicatedIpPools",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/dedicated-ip-pools/{PoolName}",
                operation: "GetDedicatedIpPool",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v2/email/dedicated-ip-pools/{PoolName}",
                operation: "DeleteDedicatedIpPool",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/dedicated-ips/{PoolName}",
                operation: "GetDedicatedIps",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v2/email/suppression/addresses",
                operation: "PutSuppressedDestination",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/suppression/addresses",
                operation: "ListSuppressedDestinations",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/suppression/addresses/{EmailAddress}",
                operation: "GetSuppressedDestination",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v2/email/suppression/addresses/{EmailAddress}",
                operation: "DeleteSuppressedDestination",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/email/contact-lists",
                operation: "CreateContactList",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/contact-lists",
                operation: "ListContactLists",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/contact-lists/{ContactListName}",
                operation: "GetContactList",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v2/email/contact-lists/{ContactListName}",
                operation: "UpdateContactList",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v2/email/contact-lists/{ContactListName}",
                operation: "DeleteContactList",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/email/contact-lists/{ContactListName}/contacts",
                operation: "CreateContact",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/contact-lists/{ContactListName}/contacts",
                operation: "ListContacts",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/contact-lists/{ContactListName}/contacts/{EmailAddress}",
                operation: "GetContact",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v2/email/contact-lists/{ContactListName}/contacts/{EmailAddress}",
                operation: "UpdateContact",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v2/email/contact-lists/{ContactListName}/contacts/{EmailAddress}",
                operation: "DeleteContact",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/email/custom-verification-email-templates",
                operation: "CreateCustomVerificationEmailTemplate",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/custom-verification-email-templates",
                operation: "ListCustomVerificationEmailTemplates",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/custom-verification-email-templates/{TemplateName}",
                operation: "GetCustomVerificationEmailTemplate",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v2/email/custom-verification-email-templates/{TemplateName}",
                operation: "DeleteCustomVerificationEmailTemplate",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v2/email/tags",
                operation: "TagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v2/email/tags",
                operation: "UntagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/tags",
                operation: "ListTagsForResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/deliverability-dashboard",
                operation: "GetDeliverabilityDashboardOptions",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v2/email/deliverability-dashboard",
                operation: "PutDeliverabilityDashboardOption",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v2/email/deliverability-dashboard/blacklist-report",
                operation: "GetBlacklistReports",
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

        // Cross-account SourceArn check applies to every send shape.
        // The lookup against another account's identity policy only
        // happens once per request, so doing it at dispatch keeps the
        // per-handler signatures clean.
        if matches!(
            operation,
            "SendEmail"
                | "SendTemplatedEmail"
                | "SendRawEmail"
                | "SendBulkEmail"
                | "SendBulkTemplatedEmail"
        ) {
            self.enforce_cross_account_source_arn(&input, ctx)?;
        }

        match operation {
            "SendEmail" => operations::emails::send_email(&state, &input, ctx),
            "SendTemplatedEmail" => operations::emails::send_templated_email(&state, &input, ctx),
            "SendRawEmail" => operations::emails::send_raw_email(&state, &input, ctx),
            "SendBulkEmail" => operations::more::send_bulk_email(&state, &input, ctx),
            "SendBulkTemplatedEmail" => {
                operations::emails::send_bulk_templated_email(&state, &input, ctx)
            }
            "VerifyDomainDkim" => operations::more::verify_domain_dkim(&state, &input, ctx),
            "GetIdentityDkimAttributes" => {
                operations::more::get_identity_dkim_attributes(&state, &input, ctx)
            }
            "SetIdentityDkimEnabled" => {
                operations::more::set_identity_dkim_enabled(&state, &input, ctx)
            }
            "SetIdentityDkimVerification" => {
                operations::more::set_identity_dkim_verification(&state, &input, ctx)
            }
            "SendCustomVerificationEmail" => {
                operations::more::send_custom_verification_email(&state, &input, ctx)
            }
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
            "PutEmailIdentityDkimAttributes" => {
                operations::more::put_email_identity_dkim_attributes(&state, &input, ctx)
            }
            "PutEmailIdentityDkimSigningAttributes" => {
                operations::more::put_email_identity_dkim_signing_attributes(&state, &input, ctx)
            }
            "PutEmailIdentityConfigurationSetAttributes" => {
                operations::more::put_email_identity_configuration_set_attributes(
                    &state, &input, ctx,
                )
            }
            "PutEmailIdentityMailFromAttributes" => {
                operations::more::put_email_identity_mail_from_attributes(&state, &input, ctx)
            }
            "PutEmailIdentityFeedbackAttributes" => {
                operations::more::put_email_identity_feedback_attributes(&state, &input, ctx)
            }
            "CreateEmailIdentityPolicy" => {
                operations::more::create_email_identity_policy(&state, &input, ctx)
            }
            "DeleteEmailIdentityPolicy" => {
                operations::more::delete_email_identity_policy(&state, &input, ctx)
            }
            "GetEmailIdentityPolicies" => {
                operations::more::get_email_identity_policies(&state, &input, ctx)
            }
            "UpdateEmailIdentityPolicy" => {
                operations::more::update_email_identity_policy(&state, &input, ctx)
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
            "UpdateEmailTemplate" => operations::more::update_email_template(&state, &input, ctx),
            "GetAccount" => operations::account::get_account(&state, &input, ctx),
            "PutAccountSendingAttributes" => {
                operations::more::put_account_sending_attributes(&state, &input, ctx)
            }
            "PutAccountSuppressionAttributes" => {
                operations::more::put_account_suppression_attributes(&state, &input, ctx)
            }
            "PutAccountDedicatedIpWarmupAttributes" => {
                operations::more::put_account_dedicated_ip_warmup_attributes(&state, &input, ctx)
            }
            "PutAccountVdmAttributes" => {
                operations::more::put_account_vdm_attributes(&state, &input, ctx)
            }
            "CreateConfigurationSet" => {
                operations::more::create_configuration_set(&state, &input, ctx)
            }
            "DeleteConfigurationSet" => {
                operations::more::delete_configuration_set(&state, &input, ctx)
            }
            "GetConfigurationSet" => operations::more::get_configuration_set(&state, &input, ctx),
            "ListConfigurationSets" => {
                operations::more::list_configuration_sets(&state, &input, ctx)
            }
            "PutConfigurationSetReputationOptions" => {
                operations::more::put_configuration_set_reputation_options(&state, &input, ctx)
            }
            "PutConfigurationSetDeliveryOptions" => {
                operations::more::put_configuration_set_delivery_options(&state, &input, ctx)
            }
            "PutConfigurationSetVdmOptions" => {
                operations::more::put_configuration_set_vdm_options(&state, &input, ctx)
            }
            "CreateConfigurationSetEventDestination" => {
                operations::more::create_configuration_set_event_destination(&state, &input, ctx)
            }
            "DeleteConfigurationSetEventDestination" => {
                operations::more::delete_configuration_set_event_destination(&state, &input, ctx)
            }
            "GetConfigurationSetEventDestinations" => {
                operations::more::get_configuration_set_event_destinations(&state, &input, ctx)
            }
            "CreateDedicatedIpPool" => {
                operations::more::create_dedicated_ip_pool(&state, &input, ctx)
            }
            "DeleteDedicatedIpPool" => {
                operations::more::delete_dedicated_ip_pool(&state, &input, ctx)
            }
            "GetDedicatedIpPool" => operations::more::get_dedicated_ip_pool(&state, &input, ctx),
            "ListDedicatedIpPools" => {
                operations::more::list_dedicated_ip_pools(&state, &input, ctx)
            }
            "GetDedicatedIps" => operations::more::get_dedicated_ips(&state, &input, ctx),
            "PutSuppressedDestination" => {
                operations::more::put_suppressed_destination(&state, &input, ctx)
            }
            "DeleteSuppressedDestination" => {
                operations::more::delete_suppressed_destination(&state, &input, ctx)
            }
            "GetSuppressedDestination" => {
                operations::more::get_suppressed_destination(&state, &input, ctx)
            }
            "ListSuppressedDestinations" => {
                operations::more::list_suppressed_destinations(&state, &input, ctx)
            }
            "CreateContactList" => operations::more::create_contact_list(&state, &input, ctx),
            "DeleteContactList" => operations::more::delete_contact_list(&state, &input, ctx),
            "GetContactList" => operations::more::get_contact_list(&state, &input, ctx),
            "ListContactLists" => operations::more::list_contact_lists(&state, &input, ctx),
            "UpdateContactList" => operations::more::update_contact_list(&state, &input, ctx),
            "CreateContact" => operations::more::create_contact(&state, &input, ctx),
            "DeleteContact" => operations::more::delete_contact(&state, &input, ctx),
            "GetContact" => operations::more::get_contact(&state, &input, ctx),
            "ListContacts" => operations::more::list_contacts(&state, &input, ctx),
            "UpdateContact" => operations::more::update_contact(&state, &input, ctx),
            "CreateCustomVerificationEmailTemplate" => {
                operations::more::create_custom_verification_email_template(&state, &input, ctx)
            }
            "DeleteCustomVerificationEmailTemplate" => {
                operations::more::delete_custom_verification_email_template(&state, &input, ctx)
            }
            "GetCustomVerificationEmailTemplate" => {
                operations::more::get_custom_verification_email_template(&state, &input, ctx)
            }
            "ListCustomVerificationEmailTemplates" => {
                operations::more::list_custom_verification_email_templates(&state, &input, ctx)
            }
            "TagResource" => operations::more::tag_resource(&state, &input, ctx),
            "UntagResource" => operations::more::untag_resource(&state, &input, ctx),
            "ListTagsForResource" => operations::more::list_tags_for_resource(&state, &input, ctx),
            "GetDeliverabilityDashboardOptions" => {
                operations::more::get_deliverability_dashboard_options(&state, &input, ctx)
            }
            "PutDeliverabilityDashboardOption" => {
                operations::more::put_deliverability_dashboard_option(&state, &input, ctx)
            }
            "GetBlacklistReports" => operations::more::get_blacklist_reports(&state, &input, ctx),
            "CreateReceiptRuleSet" => {
                operations::receipt::create_receipt_rule_set(&state, &input, ctx)
            }
            "DeleteReceiptRuleSet" => {
                operations::receipt::delete_receipt_rule_set(&state, &input, ctx)
            }
            "DescribeReceiptRuleSet" => {
                operations::receipt::describe_receipt_rule_set(&state, &input, ctx)
            }
            "ListReceiptRuleSets" => {
                operations::receipt::list_receipt_rule_sets(&state, &input, ctx)
            }
            "SetActiveReceiptRuleSet" => {
                operations::receipt::set_active_receipt_rule_set(&state, &input, ctx)
            }
            "DescribeActiveReceiptRuleSet" => {
                operations::receipt::describe_active_receipt_rule_set(&state, &input, ctx)
            }
            "CreateReceiptRule" => operations::receipt::create_receipt_rule(&state, &input, ctx),
            "UpdateReceiptRule" => operations::receipt::update_receipt_rule(&state, &input, ctx),
            "DeleteReceiptRule" => operations::receipt::delete_receipt_rule(&state, &input, ctx),
            "DescribeReceiptRule" => {
                operations::receipt::describe_receipt_rule(&state, &input, ctx)
            }
            "ReorderReceiptRuleSet" => {
                operations::receipt::reorder_receipt_rule_set(&state, &input, ctx)
            }
            "DeliverReceiptMessage" => {
                operations::receipt::deliver_receipt_message(&state, &input, ctx)
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}

struct ParsedSesArn {
    account: String,
    region: String,
    identity: String,
}

fn parse_ses_identity_arn(arn: &str) -> Option<ParsedSesArn> {
    // Format: arn:{partition}:ses:{region}:{account}:identity/{identity}
    let parts: Vec<&str> = arn.splitn(6, ':').collect();
    if parts.len() != 6 || parts[0] != "arn" || parts[2] != "ses" {
        return None;
    }
    let identity = parts[5].strip_prefix("identity/")?;
    Some(ParsedSesArn {
        account: parts[4].to_string(),
        region: parts[3].to_string(),
        identity: identity.to_string(),
    })
}

fn identity_policy_allows_send(policy_json: &str) -> bool {
    let Ok(doc): Result<Value, _> = serde_json::from_str(policy_json) else {
        return false;
    };
    let statements = match doc.get("Statement") {
        Some(Value::Array(arr)) => arr.clone(),
        Some(stmt @ Value::Object(_)) => vec![stmt.clone()],
        _ => return false,
    };
    for stmt in statements {
        if stmt.get("Effect").and_then(Value::as_str) != Some("Allow") {
            continue;
        }
        let actions: Vec<String> = match stmt.get("Action") {
            Some(Value::String(s)) => vec![s.clone()],
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            _ => continue,
        };
        for action in actions {
            if action_matches_send(&action) {
                return true;
            }
        }
    }
    false
}

fn action_matches_send(action: &str) -> bool {
    if action == "*" || action == "ses:*" {
        return true;
    }
    matches!(
        action,
        "ses:SendEmail" | "ses:SendRawEmail" | "ses:SendBulkTemplatedEmail"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> RequestContext {
        RequestContext::new("ses", "us-east-1")
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        fn noop_clone(_: *const ()) -> RawWaker {
            noop_raw_waker()
        }
        fn noop(_: *const ()) {}
        fn noop_raw_waker() -> RawWaker {
            static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(f);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => {}
            }
        }
    }

    #[test]
    fn put_then_get_account_round_trips_vdm_and_suppression() {
        let svc = SesService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "PutAccountSuppressionAttributes",
            json!({ "SuppressedReasons": ["BOUNCE"] }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "PutAccountVdmAttributes",
            json!({
                "VdmAttributes": {
                    "VdmEnabled": "ENABLED",
                    "DashboardAttributes": { "EngagementMetrics": "ENABLED" }
                }
            }),
            &ctx,
        ))
        .unwrap();

        let got = block_on(svc.handle("GetAccount", json!({}), &ctx)).unwrap();
        assert_eq!(
            got["SuppressionAttributes"]["SuppressedReasons"][0],
            "BOUNCE"
        );
        assert_eq!(got["VdmAttributes"]["VdmEnabled"], "ENABLED");
    }

    #[test]
    fn put_account_suppression_attributes_rejects_unknown_reason() {
        let svc = SesService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "PutAccountSuppressionAttributes",
            json!({ "SuppressedReasons": ["SPAM"] }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
    }

    #[test]
    fn put_account_vdm_attributes_rejects_invalid_enabled() {
        let svc = SesService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "PutAccountVdmAttributes",
            json!({ "VdmAttributes": { "VdmEnabled": "MAYBE" } }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
    }

    #[test]
    fn put_suppressed_destination_round_trips_and_validates_reason() {
        let svc = SesService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "PutSuppressedDestination",
            json!({ "EmailAddress": "x@example.com", "Reason": "COMPLAINT" }),
            &ctx,
        ))
        .unwrap();
        let got = block_on(svc.handle(
            "GetSuppressedDestination",
            json!({ "EmailAddress": "x@example.com" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(got["SuppressedDestination"]["Reason"], "COMPLAINT");

        let err = block_on(svc.handle(
            "PutSuppressedDestination",
            json!({ "EmailAddress": "y@example.com", "Reason": "MANUAL" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
    }

    #[test]
    fn parse_ses_identity_arn_handles_valid_shape() {
        let parsed =
            parse_ses_identity_arn("arn:aws:ses:us-east-1:222222222222:identity/example.com")
                .unwrap();
        assert_eq!(parsed.account, "222222222222");
        assert_eq!(parsed.region, "us-east-1");
        assert_eq!(parsed.identity, "example.com");
    }

    #[test]
    fn parse_ses_identity_arn_rejects_malformed() {
        assert!(parse_ses_identity_arn("not-an-arn").is_none());
        assert!(parse_ses_identity_arn("arn:aws:ses:us-east-1:222:other/x").is_none());
    }

    #[test]
    fn identity_policy_allows_send_when_statement_grants_send() {
        let doc = r#"{
            "Version":"2012-10-17",
            "Statement":[{
                "Effect":"Allow",
                "Principal":{"AWS":"*"},
                "Action":"ses:SendEmail",
                "Resource":"*"
            }]
        }"#;
        assert!(identity_policy_allows_send(doc));
    }

    #[test]
    fn identity_policy_allows_send_respects_wildcards() {
        let doc = r#"{"Statement":[{"Effect":"Allow","Action":"ses:*"}]}"#;
        assert!(identity_policy_allows_send(doc));
        let doc = r#"{"Statement":[{"Effect":"Allow","Action":"*"}]}"#;
        assert!(identity_policy_allows_send(doc));
    }

    #[test]
    fn identity_policy_denies_when_no_matching_action() {
        let doc = r#"{"Statement":[{"Effect":"Allow","Action":"ses:GetIdentity"}]}"#;
        assert!(!identity_policy_allows_send(doc));
    }

    #[test]
    fn cross_account_source_arn_without_policy_is_rejected() {
        let svc = SesService::new();
        let cross_ctx = RequestContext::new("ses", "us-east-1");
        let err = svc
            .enforce_cross_account_source_arn(
                &json!({
                    "SourceArn": "arn:aws:ses:us-east-1:222222222222:identity/foreign.com"
                }),
                &cross_ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "AccessDenied");
    }

    #[test]
    fn cross_account_source_arn_with_grant_is_allowed() {
        let svc = SesService::new();
        let foreign = svc.store.get("222222222222", "us-east-1");
        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "grant".to_string(),
            r#"{"Statement":[{"Effect":"Allow","Action":"ses:SendEmail"}]}"#.to_string(),
        );
        foreign
            .identity_policies
            .insert("foreign.com".to_string(), policies);
        svc.enforce_cross_account_source_arn(
            &json!({
                "SourceArn": "arn:aws:ses:us-east-1:222222222222:identity/foreign.com"
            }),
            &RequestContext::new("ses", "us-east-1"),
        )
        .unwrap();
    }

    #[test]
    fn cross_account_source_arn_with_unrelated_policy_is_rejected() {
        let svc = SesService::new();
        let foreign = svc.store.get("222222222222", "us-east-1");
        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "metrics".to_string(),
            r#"{"Statement":[{"Effect":"Allow","Action":"ses:GetIdentity"}]}"#.to_string(),
        );
        foreign
            .identity_policies
            .insert("foreign.com".to_string(), policies);
        let err = svc
            .enforce_cross_account_source_arn(
                &json!({
                    "SourceArn": "arn:aws:ses:us-east-1:222222222222:identity/foreign.com"
                }),
                &RequestContext::new("ses", "us-east-1"),
            )
            .unwrap_err();
        assert_eq!(err.code, "AccessDenied");
    }

    #[test]
    fn same_account_source_arn_passes_through() {
        let svc = SesService::new();
        let local_ctx = RequestContext::new("ses", "us-east-1");
        svc.enforce_cross_account_source_arn(
            &json!({
                "SourceArn": format!("arn:aws:ses:us-east-1:{}:identity/example.com", local_ctx.account_id)
            }),
            &local_ctx,
        )
        .unwrap();
    }

    #[test]
    fn missing_source_arn_passes_through() {
        let svc = SesService::new();
        svc.enforce_cross_account_source_arn(
            &json!({ "FromEmailAddress": "a@b.com" }),
            &RequestContext::new("ses", "us-east-1"),
        )
        .unwrap();
    }
}
