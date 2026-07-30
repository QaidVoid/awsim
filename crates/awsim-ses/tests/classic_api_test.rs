//! SES classic (v1) operations.
//!
//! `aws ses ...` speaks the original query API. AWSim implemented SES v2
//! but almost none of v1, so identity management, the entry point for
//! every SES workflow, answered `UnknownOperationException` and nothing
//! downstream could be reached.
//!
//! These exercise the classic surface end to end against the same state
//! the v2 handlers use, and pin the query-protocol shapes: a list nests
//! under `member`, a map is a list of `entry` key/value pairs.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_ses::SesService;
use serde_json::{Value, json};

fn ctx() -> RequestContext {
    RequestContext::new("ses", "us-east-1")
}

fn members(response: &Value, field: &str) -> Vec<Value> {
    response[field]["member"]
        .as_array()
        .unwrap_or_else(|| panic!("{field} should be a member list: {response}"))
        .clone()
}

fn entry<'a>(response: &'a Value, field: &str, key: &str) -> Option<&'a Value> {
    response[field]["entry"]
        .as_array()?
        .iter()
        .find(|e| e["key"] == key)
        .map(|e| &e["value"])
}

async fn service_with_identities() -> SesService {
    let svc = SesService::new();
    svc.handle(
        "VerifyEmailIdentity",
        json!({ "EmailAddress": "dev@example.com" }),
        &ctx(),
    )
    .await
    .expect("VerifyEmailIdentity");
    svc.handle(
        "VerifyDomainIdentity",
        json!({ "Domain": "example.com" }),
        &ctx(),
    )
    .await
    .expect("VerifyDomainIdentity");
    svc
}

#[tokio::test]
async fn verify_then_list_identities() {
    let svc = service_with_identities().await;

    let all = svc
        .handle("ListIdentities", json!({}), &ctx())
        .await
        .expect("ListIdentities");
    assert_eq!(
        members(&all, "Identities"),
        vec![json!("dev@example.com"), json!("example.com")]
    );

    let domains = svc
        .handle(
            "ListIdentities",
            json!({ "IdentityType": "DOMAIN" }),
            &ctx(),
        )
        .await
        .expect("ListIdentities");
    assert_eq!(members(&domains, "Identities"), vec![json!("example.com")]);
}

/// The verification token goes into a DNS record, so provisioning twice
/// must not hand back a different value.
#[tokio::test]
async fn domain_verification_token_is_stable() {
    let svc = SesService::new();
    let mut tokens = Vec::new();
    for _ in 0..2 {
        let out = svc
            .handle(
                "VerifyDomainIdentity",
                json!({ "Domain": "stable.example" }),
                &ctx(),
            )
            .await
            .expect("VerifyDomainIdentity");
        tokens.push(out["VerificationToken"].as_str().unwrap().to_string());
    }
    assert_eq!(tokens[0], tokens[1]);
    assert!(!tokens[0].is_empty());
}

#[tokio::test]
async fn verify_rejects_the_wrong_identity_kind() {
    let svc = SesService::new();
    let err = svc
        .handle(
            "VerifyEmailIdentity",
            json!({ "EmailAddress": "example.com" }),
            &ctx(),
        )
        .await
        .expect_err("a domain is not an email address");
    assert_eq!(err.code, "InvalidParameterValue", "{err:?}");

    let err = svc
        .handle(
            "VerifyDomainIdentity",
            json!({ "Domain": "dev@example.com" }),
            &ctx(),
        )
        .await
        .expect_err("an email address is not a domain");
    assert_eq!(err.code, "InvalidParameterValue", "{err:?}");
}

#[tokio::test]
async fn verification_attributes_come_back_as_entry_pairs() {
    let svc = service_with_identities().await;

    let out = svc
        .handle(
            "GetIdentityVerificationAttributes",
            json!({ "Identities": ["dev@example.com", "example.com", "absent.example"] }),
            &ctx(),
        )
        .await
        .expect("GetIdentityVerificationAttributes");

    let email = entry(&out, "VerificationAttributes", "dev@example.com")
        .unwrap_or_else(|| panic!("missing email identity: {out}"));
    assert_eq!(email["VerificationStatus"], "Success");
    assert!(
        email.get("VerificationToken").is_none(),
        "only a domain carries a token: {email}"
    );

    let domain = entry(&out, "VerificationAttributes", "example.com").expect("domain");
    assert!(domain["VerificationToken"].as_str().is_some());

    assert!(
        entry(&out, "VerificationAttributes", "absent.example").is_none(),
        "an unknown identity is omitted rather than invented: {out}"
    );
}

/// Re-verifying must not reset settings a caller already applied.
#[tokio::test]
async fn verifying_an_existing_identity_keeps_its_settings() {
    let svc = service_with_identities().await;
    svc.handle(
        "SetIdentityFeedbackForwardingEnabled",
        json!({ "Identity": "example.com", "ForwardingEnabled": false }),
        &ctx(),
    )
    .await
    .expect("SetIdentityFeedbackForwardingEnabled");

    svc.handle(
        "VerifyDomainIdentity",
        json!({ "Domain": "example.com" }),
        &ctx(),
    )
    .await
    .expect("VerifyDomainIdentity");

    let out = svc
        .handle(
            "GetIdentityNotificationAttributes",
            json!({ "Identities": ["example.com"] }),
            &ctx(),
        )
        .await
        .expect("GetIdentityNotificationAttributes");
    assert_eq!(
        entry(&out, "NotificationAttributes", "example.com").unwrap()["ForwardingEnabled"],
        json!(false)
    );
}

#[tokio::test]
async fn notification_topics_round_trip() {
    let svc = service_with_identities().await;

    svc.handle(
        "SetIdentityNotificationTopic",
        json!({
            "Identity": "example.com",
            "NotificationType": "Bounce",
            "SnsTopic": "arn:aws:sns:us-east-1:000000000000:bounces",
        }),
        &ctx(),
    )
    .await
    .expect("SetIdentityNotificationTopic");
    svc.handle(
        "SetIdentityHeadersInNotificationsEnabled",
        json!({ "Identity": "example.com", "NotificationType": "Bounce", "Enabled": true }),
        &ctx(),
    )
    .await
    .expect("SetIdentityHeadersInNotificationsEnabled");

    let out = svc
        .handle(
            "GetIdentityNotificationAttributes",
            json!({ "Identities": ["example.com"] }),
            &ctx(),
        )
        .await
        .expect("GetIdentityNotificationAttributes");
    let attrs = entry(&out, "NotificationAttributes", "example.com").expect("attributes");
    assert_eq!(
        attrs["BounceTopic"],
        "arn:aws:sns:us-east-1:000000000000:bounces"
    );
    assert_eq!(attrs["HeadersInBounceNotificationsEnabled"], json!(true));
    assert_eq!(attrs["ComplaintTopic"], "");

    // Omitting SnsTopic is how AWS turns the notification back off.
    svc.handle(
        "SetIdentityNotificationTopic",
        json!({ "Identity": "example.com", "NotificationType": "Bounce" }),
        &ctx(),
    )
    .await
    .expect("SetIdentityNotificationTopic");
    let cleared = svc
        .handle(
            "GetIdentityNotificationAttributes",
            json!({ "Identities": ["example.com"] }),
            &ctx(),
        )
        .await
        .expect("GetIdentityNotificationAttributes");
    assert_eq!(
        entry(&cleared, "NotificationAttributes", "example.com").unwrap()["BounceTopic"],
        ""
    );
}

#[tokio::test]
async fn unknown_notification_type_is_rejected() {
    let svc = service_with_identities().await;
    let err = svc
        .handle(
            "SetIdentityNotificationTopic",
            json!({ "Identity": "example.com", "NotificationType": "Bogus", "SnsTopic": "x" }),
            &ctx(),
        )
        .await
        .expect_err("only Bounce, Complaint, and Delivery exist");
    assert_eq!(err.code, "InvalidParameterValue", "{err:?}");
}

#[tokio::test]
async fn mail_from_domain_round_trips() {
    let svc = service_with_identities().await;

    svc.handle(
        "SetIdentityMailFromDomain",
        json!({
            "Identity": "example.com",
            "MailFromDomain": "mail.example.com",
            "BehaviorOnMXFailure": "RejectMessage",
        }),
        &ctx(),
    )
    .await
    .expect("SetIdentityMailFromDomain");

    let out = svc
        .handle(
            "GetIdentityMailFromDomainAttributes",
            json!({ "Identities": ["example.com", "dev@example.com"] }),
            &ctx(),
        )
        .await
        .expect("GetIdentityMailFromDomainAttributes");
    let attrs = entry(&out, "MailFromDomainAttributes", "example.com").expect("attributes");
    assert_eq!(attrs["MailFromDomain"], "mail.example.com");
    assert_eq!(attrs["BehaviorOnMXFailure"], "RejectMessage");
    assert!(
        entry(&out, "MailFromDomainAttributes", "dev@example.com").is_none(),
        "an identity with no MAIL FROM set is omitted: {out}"
    );
}

#[tokio::test]
async fn identity_policies_round_trip() {
    let svc = service_with_identities().await;
    let policy = r#"{"Version":"2012-10-17","Statement":[]}"#;

    svc.handle(
        "PutIdentityPolicy",
        json!({ "Identity": "example.com", "PolicyName": "p1", "Policy": policy }),
        &ctx(),
    )
    .await
    .expect("PutIdentityPolicy");

    let names = svc
        .handle(
            "ListIdentityPolicies",
            json!({ "Identity": "example.com" }),
            &ctx(),
        )
        .await
        .expect("ListIdentityPolicies");
    assert_eq!(members(&names, "PolicyNames"), vec![json!("p1")]);

    let policies = svc
        .handle(
            "GetIdentityPolicies",
            json!({ "Identity": "example.com", "PolicyNames": ["p1"] }),
            &ctx(),
        )
        .await
        .expect("GetIdentityPolicies");
    assert_eq!(entry(&policies, "Policies", "p1").unwrap(), policy);

    svc.handle(
        "DeleteIdentityPolicy",
        json!({ "Identity": "example.com", "PolicyName": "p1" }),
        &ctx(),
    )
    .await
    .expect("DeleteIdentityPolicy");
    let after = svc
        .handle(
            "ListIdentityPolicies",
            json!({ "Identity": "example.com" }),
            &ctx(),
        )
        .await
        .expect("ListIdentityPolicies");
    assert!(members(&after, "PolicyNames").is_empty());
}

#[tokio::test]
async fn a_malformed_identity_policy_is_rejected() {
    let svc = service_with_identities().await;
    let err = svc
        .handle(
            "PutIdentityPolicy",
            json!({ "Identity": "example.com", "PolicyName": "p", "Policy": "not json" }),
            &ctx(),
        )
        .await
        .expect_err("a policy has to be JSON");
    assert_eq!(err.code, "InvalidPolicy", "{err:?}");
}

#[tokio::test]
async fn templates_round_trip_and_render() {
    let svc = SesService::new();
    svc.handle(
        "CreateTemplate",
        json!({ "Template": {
            "TemplateName": "welcome",
            "SubjectPart": "Hi {{name}}",
            "HtmlPart": "<p>Welcome {{name}}</p>",
            "TextPart": "Welcome {{name}}",
        }}),
        &ctx(),
    )
    .await
    .expect("CreateTemplate");

    let got = svc
        .handle("GetTemplate", json!({ "TemplateName": "welcome" }), &ctx())
        .await
        .expect("GetTemplate");
    assert_eq!(got["Template"]["SubjectPart"], "Hi {{name}}");
    assert_eq!(got["Template"]["HtmlPart"], "<p>Welcome {{name}}</p>");

    let rendered = svc
        .handle(
            "TestRenderTemplate",
            json!({ "TemplateName": "welcome", "TemplateData": r#"{"name":"Ada"}"# }),
            &ctx(),
        )
        .await
        .expect("TestRenderTemplate");
    let body = rendered["RenderedTemplate"].as_str().expect("rendered");
    assert!(body.contains("Hi Ada"), "{body}");
    assert!(body.contains("<p>Welcome Ada</p>"), "{body}");

    let listed = svc
        .handle("ListTemplates", json!({}), &ctx())
        .await
        .expect("ListTemplates");
    assert_eq!(members(&listed, "TemplatesMetadata")[0]["Name"], "welcome");
}

#[tokio::test]
async fn creating_a_template_twice_conflicts_but_update_succeeds() {
    let svc = SesService::new();
    let template = json!({ "Template": { "TemplateName": "t", "SubjectPart": "one" }});
    svc.handle("CreateTemplate", template.clone(), &ctx())
        .await
        .expect("CreateTemplate");

    let err = svc
        .handle("CreateTemplate", template, &ctx())
        .await
        .expect_err("a second create conflicts");
    assert_eq!(err.code, "AlreadyExists", "{err:?}");

    svc.handle(
        "UpdateTemplate",
        json!({ "Template": { "TemplateName": "t", "SubjectPart": "two" }}),
        &ctx(),
    )
    .await
    .expect("UpdateTemplate");
    let got = svc
        .handle("GetTemplate", json!({ "TemplateName": "t" }), &ctx())
        .await
        .expect("GetTemplate");
    assert_eq!(got["Template"]["SubjectPart"], "two");
}

#[tokio::test]
async fn updating_a_missing_template_is_rejected() {
    let svc = SesService::new();
    let err = svc
        .handle(
            "UpdateTemplate",
            json!({ "Template": { "TemplateName": "ghost" }}),
            &ctx(),
        )
        .await
        .expect_err("there is nothing to update");
    assert_eq!(err.code, "TemplateDoesNotExist", "{err:?}");
}

#[tokio::test]
async fn account_sending_switch_round_trips() {
    let svc = SesService::new();
    let initial = svc
        .handle("GetAccountSendingEnabled", json!({}), &ctx())
        .await
        .expect("GetAccountSendingEnabled");
    assert_eq!(
        initial["Enabled"],
        json!(true),
        "an account that has never been touched can send"
    );

    svc.handle(
        "UpdateAccountSendingEnabled",
        json!({ "Enabled": false }),
        &ctx(),
    )
    .await
    .expect("UpdateAccountSendingEnabled");
    let after = svc
        .handle("GetAccountSendingEnabled", json!({}), &ctx())
        .await
        .expect("GetAccountSendingEnabled");
    assert_eq!(after["Enabled"], json!(false));
}

#[tokio::test]
async fn send_quota_and_statistics_are_answerable() {
    let svc = SesService::new();
    let quota = svc
        .handle("GetSendQuota", json!({}), &ctx())
        .await
        .expect("GetSendQuota");
    assert!(quota["Max24HourSend"].as_f64().unwrap() > 0.0);
    assert!(quota["MaxSendRate"].as_f64().unwrap() > 0.0);

    let stats = svc
        .handle("GetSendStatistics", json!({}), &ctx())
        .await
        .expect("GetSendStatistics");
    let points = members(&stats, "SendDataPoints");
    assert_eq!(points.len(), 1);
    assert_eq!(points[0]["Bounces"], json!(0));
}

#[tokio::test]
async fn receipt_filters_round_trip() {
    let svc = SesService::new();
    svc.handle(
        "CreateReceiptFilter",
        json!({ "Filter": {
            "Name": "blocklist",
            "IpFilter": { "Policy": "Block", "Cidr": "10.0.0.0/24" },
        }}),
        &ctx(),
    )
    .await
    .expect("CreateReceiptFilter");

    let listed = svc
        .handle("ListReceiptFilters", json!({}), &ctx())
        .await
        .expect("ListReceiptFilters");
    let filters = members(&listed, "Filters");
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0]["IpFilter"]["Policy"], "Block");

    svc.handle(
        "DeleteReceiptFilter",
        json!({ "FilterName": "blocklist" }),
        &ctx(),
    )
    .await
    .expect("DeleteReceiptFilter");
    let after = svc
        .handle("ListReceiptFilters", json!({}), &ctx())
        .await
        .expect("ListReceiptFilters");
    assert!(members(&after, "Filters").is_empty());
}

#[tokio::test]
async fn an_invalid_filter_policy_is_rejected() {
    let svc = SesService::new();
    let err = svc
        .handle(
            "CreateReceiptFilter",
            json!({ "Filter": {
                "Name": "f",
                "IpFilter": { "Policy": "Maybe", "Cidr": "10.0.0.0/24" },
            }}),
            &ctx(),
        )
        .await
        .expect_err("only Allow and Block exist");
    assert_eq!(err.code, "InvalidParameterValue", "{err:?}");
}

/// The two APIs disagree on this response, so the classic path has to
/// answer structures while v2 answers bare names.
#[tokio::test]
async fn classic_list_configuration_sets_returns_structures() {
    let svc = SesService::new();
    svc.handle(
        "CreateConfigurationSet",
        json!({ "ConfigurationSet": { "Name": "cs1" }}),
        &ctx(),
    )
    .await
    .expect("CreateConfigurationSet");

    let classic = svc
        .handle("ListConfigurationSets", json!({}), &ctx())
        .await
        .expect("ListConfigurationSets");
    assert_eq!(members(&classic, "ConfigurationSets")[0]["Name"], "cs1");

    let mut v2_ctx = ctx();
    v2_ctx.uri = "/v2/email/configuration-sets".to_string();
    let v2 = svc
        .handle("ListConfigurationSets", json!({}), &v2_ctx)
        .await
        .expect("ListConfigurationSets");
    assert_eq!(members(&v2, "ConfigurationSets"), vec![json!("cs1")]);
}

#[tokio::test]
async fn describe_configuration_set_reports_reputation_options() {
    let svc = SesService::new();
    svc.handle(
        "CreateConfigurationSet",
        json!({ "ConfigurationSet": { "Name": "cs1" }}),
        &ctx(),
    )
    .await
    .expect("CreateConfigurationSet");

    svc.handle(
        "UpdateConfigurationSetSendingEnabled",
        json!({ "ConfigurationSetName": "cs1", "Enabled": false }),
        &ctx(),
    )
    .await
    .expect("UpdateConfigurationSetSendingEnabled");

    let out = svc
        .handle(
            "DescribeConfigurationSet",
            json!({ "ConfigurationSetName": "cs1" }),
            &ctx(),
        )
        .await
        .expect("DescribeConfigurationSet");
    assert_eq!(out["ConfigurationSet"]["Name"], "cs1");
    assert_eq!(out["ReputationOptions"]["SendingEnabled"], json!(false));
}

#[tokio::test]
async fn describing_a_missing_configuration_set_is_rejected() {
    let svc = SesService::new();
    let err = svc
        .handle(
            "DescribeConfigurationSet",
            json!({ "ConfigurationSetName": "ghost" }),
            &ctx(),
        )
        .await
        .expect_err("there is nothing to describe");
    assert_eq!(err.code, "ConfigurationSetDoesNotExist", "{err:?}");
}

/// Deleting is idempotent on AWS, and it must also clear the policies
/// attached to the identity rather than orphaning them.
#[tokio::test]
async fn deleting_an_identity_clears_its_policies() {
    let svc = service_with_identities().await;
    svc.handle(
        "PutIdentityPolicy",
        json!({ "Identity": "example.com", "PolicyName": "p", "Policy": "{}" }),
        &ctx(),
    )
    .await
    .expect("PutIdentityPolicy");

    for _ in 0..2 {
        svc.handle(
            "DeleteIdentity",
            json!({ "Identity": "example.com" }),
            &ctx(),
        )
        .await
        .expect("DeleteIdentity is idempotent");
    }

    let listed = svc
        .handle("ListIdentities", json!({}), &ctx())
        .await
        .expect("ListIdentities");
    assert_eq!(
        members(&listed, "Identities"),
        vec![json!("dev@example.com")]
    );

    let policies = svc
        .handle(
            "ListIdentityPolicies",
            json!({ "Identity": "example.com" }),
            &ctx(),
        )
        .await
        .expect("ListIdentityPolicies");
    assert!(members(&policies, "PolicyNames").is_empty());
}

async fn service_with_rules() -> SesService {
    let svc = SesService::new();
    svc.handle(
        "CreateReceiptRuleSet",
        json!({ "RuleSetName": "rs" }),
        &ctx(),
    )
    .await
    .expect("CreateReceiptRuleSet");
    for name in ["a", "b", "c"] {
        svc.handle(
            "CreateReceiptRule",
            json!({ "RuleSetName": "rs", "Rule": { "Name": name } }),
            &ctx(),
        )
        .await
        .unwrap_or_else(|e| panic!("CreateReceiptRule {name}: {e:?}"));
    }
    svc
}

async fn rule_names(svc: &SesService, set: &str) -> Vec<String> {
    let out = svc
        .handle(
            "DescribeReceiptRuleSet",
            json!({ "RuleSetName": set }),
            &ctx(),
        )
        .await
        .expect("DescribeReceiptRuleSet");
    members(&out, "Rules")
        .iter()
        .map(|r| r["Name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn cloning_a_rule_set_copies_its_rules() {
    let svc = service_with_rules().await;
    svc.handle(
        "CloneReceiptRuleSet",
        json!({ "RuleSetName": "copy", "OriginalRuleSetName": "rs" }),
        &ctx(),
    )
    .await
    .expect("CloneReceiptRuleSet");

    assert_eq!(rule_names(&svc, "copy").await, ["a", "b", "c"]);

    let err = svc
        .handle(
            "CloneReceiptRuleSet",
            json!({ "RuleSetName": "copy", "OriginalRuleSetName": "rs" }),
            &ctx(),
        )
        .await
        .expect_err("cloning onto an existing name conflicts");
    assert_eq!(err.code, "AlreadyExists", "{err:?}");
}

#[tokio::test]
async fn a_rule_can_be_moved_within_its_set() {
    let svc = service_with_rules().await;

    svc.handle(
        "SetReceiptRulePosition",
        json!({ "RuleSetName": "rs", "RuleName": "c", "After": "a" }),
        &ctx(),
    )
    .await
    .expect("SetReceiptRulePosition");
    assert_eq!(rule_names(&svc, "rs").await, ["a", "c", "b"]);

    // Omitting `After` moves the rule to the front.
    svc.handle(
        "SetReceiptRulePosition",
        json!({ "RuleSetName": "rs", "RuleName": "b" }),
        &ctx(),
    )
    .await
    .expect("SetReceiptRulePosition");
    assert_eq!(rule_names(&svc, "rs").await, ["b", "a", "c"]);
}

/// A bad anchor must leave the order alone rather than dropping the rule
/// it already removed.
#[tokio::test]
async fn moving_after_an_unknown_rule_is_rejected_without_losing_it() {
    let svc = service_with_rules().await;
    let err = svc
        .handle(
            "SetReceiptRulePosition",
            json!({ "RuleSetName": "rs", "RuleName": "c", "After": "ghost" }),
            &ctx(),
        )
        .await
        .expect_err("the anchor does not exist");
    assert_eq!(err.code, "RuleDoesNotExist", "{err:?}");
    assert_eq!(rule_names(&svc, "rs").await, ["a", "b", "c"]);
}

#[tokio::test]
async fn an_event_destination_update_replaces_rather_than_duplicates() {
    let svc = SesService::new();
    svc.handle(
        "CreateConfigurationSet",
        json!({ "ConfigurationSet": { "Name": "cs" }}),
        &ctx(),
    )
    .await
    .expect("CreateConfigurationSet");

    let destination = |arn: &str| {
        json!({
            "ConfigurationSetName": "cs",
            "EventDestination": {
                "Name": "d1",
                "Enabled": true,
                "MatchingEventTypes": ["send"],
                "SNSDestination": { "TopicARN": arn },
            },
        })
    };
    svc.handle(
        "CreateConfigurationSetEventDestination",
        destination("arn:aws:sns:us-east-1:000000000000:one"),
        &ctx(),
    )
    .await
    .expect("CreateConfigurationSetEventDestination");
    svc.handle(
        "UpdateConfigurationSetEventDestination",
        destination("arn:aws:sns:us-east-1:000000000000:two"),
        &ctx(),
    )
    .await
    .expect("UpdateConfigurationSetEventDestination");

    let out = svc
        .handle(
            "DescribeConfigurationSet",
            json!({ "ConfigurationSetName": "cs" }),
            &ctx(),
        )
        .await
        .expect("DescribeConfigurationSet");
    let destinations = members(&out, "EventDestinations");
    assert_eq!(destinations.len(), 1, "update must not append: {out}");
    assert_eq!(
        destinations[0]["SNSDestination"]["TopicARN"], "arn:aws:sns:us-east-1:000000000000:two",
        "the classic SNSDestination spelling has to be read: {out}"
    );
}

#[tokio::test]
async fn a_custom_verification_template_update_keeps_omitted_fields() {
    let svc = SesService::new();
    svc.handle(
        "CreateCustomVerificationEmailTemplate",
        json!({
            "TemplateName": "cvt",
            "FromEmailAddress": "dev@example.com",
            "TemplateSubject": "Verify",
            "TemplateContent": "<p>click</p>",
            "SuccessRedirectionURL": "https://ok.example",
            "FailureRedirectionURL": "https://no.example",
        }),
        &ctx(),
    )
    .await
    .expect("CreateCustomVerificationEmailTemplate");

    svc.handle(
        "UpdateCustomVerificationEmailTemplate",
        json!({ "TemplateName": "cvt", "TemplateSubject": "Please verify" }),
        &ctx(),
    )
    .await
    .expect("UpdateCustomVerificationEmailTemplate");

    let out = svc
        .handle(
            "GetCustomVerificationEmailTemplate",
            json!({ "TemplateName": "cvt" }),
            &ctx(),
        )
        .await
        .expect("GetCustomVerificationEmailTemplate");
    assert_eq!(out["TemplateSubject"], "Please verify");
    assert_eq!(
        out["FromEmailAddress"], "dev@example.com",
        "an omitted field keeps its stored value: {out}"
    );
}

#[tokio::test]
async fn updating_a_missing_custom_verification_template_is_rejected() {
    let svc = SesService::new();
    let err = svc
        .handle(
            "UpdateCustomVerificationEmailTemplate",
            json!({ "TemplateName": "ghost" }),
            &ctx(),
        )
        .await
        .expect_err("there is nothing to update");
    assert_eq!(
        err.code, "CustomVerificationEmailTemplateDoesNotExist",
        "{err:?}"
    );
}

/// The classic SendEmail carries a flat `Message` and calls the sender
/// `Source`, where v2 uses `Content.Simple` and `FromEmailAddress`.
#[tokio::test]
async fn classic_send_email_shape_is_accepted() {
    let svc = service_with_identities().await;
    let out = svc
        .handle(
            "SendEmail",
            json!({
                "Source": "dev@example.com",
                "Destination": { "ToAddresses": ["a@b.c"] },
                "Message": {
                    "Subject": { "Data": "Hi there" },
                    "Body": { "Text": { "Data": "Hello world" } },
                },
            }),
            &ctx(),
        )
        .await
        .expect("SendEmail");
    assert!(out["MessageId"].as_str().is_some(), "{out}");

    let sent = svc.list_sent_emails();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].2.subject.as_deref(), Some("Hi there"));
    assert_eq!(sent[0].2.body_text.as_deref(), Some("Hello world"));
}

#[tokio::test]
async fn tracking_options_round_trip_and_clear() {
    let svc = SesService::new();
    svc.handle(
        "CreateConfigurationSet",
        json!({ "ConfigurationSet": { "Name": "cs" }}),
        &ctx(),
    )
    .await
    .expect("CreateConfigurationSet");

    svc.handle(
        "CreateConfigurationSetTrackingOptions",
        json!({
            "ConfigurationSetName": "cs",
            "TrackingOptions": { "CustomRedirectDomain": "click.example.com" },
        }),
        &ctx(),
    )
    .await
    .expect("CreateConfigurationSetTrackingOptions");

    let described = svc
        .handle(
            "DescribeConfigurationSet",
            json!({ "ConfigurationSetName": "cs" }),
            &ctx(),
        )
        .await
        .expect("DescribeConfigurationSet");
    assert_eq!(
        described["TrackingOptions"]["CustomRedirectDomain"],
        "click.example.com"
    );

    // v2 passes the domain flat rather than nested.
    svc.handle(
        "PutConfigurationSetTrackingOptions",
        json!({ "ConfigurationSetName": "cs", "CustomRedirectDomain": "go.example.com" }),
        &ctx(),
    )
    .await
    .expect("PutConfigurationSetTrackingOptions");
    let updated = svc
        .handle(
            "DescribeConfigurationSet",
            json!({ "ConfigurationSetName": "cs" }),
            &ctx(),
        )
        .await
        .expect("DescribeConfigurationSet");
    assert_eq!(
        updated["TrackingOptions"]["CustomRedirectDomain"],
        "go.example.com"
    );

    svc.handle(
        "DeleteConfigurationSetTrackingOptions",
        json!({ "ConfigurationSetName": "cs" }),
        &ctx(),
    )
    .await
    .expect("DeleteConfigurationSetTrackingOptions");
    let cleared = svc
        .handle(
            "DescribeConfigurationSet",
            json!({ "ConfigurationSetName": "cs" }),
            &ctx(),
        )
        .await
        .expect("DescribeConfigurationSet");
    assert!(
        cleared.get("TrackingOptions").is_none(),
        "tracking was removed: {cleared}"
    );
}

/// The classic API spells event types lowercase (`send`) where v2 uses
/// `SEND`, so validation rejected every request an `aws ses` caller made.
#[tokio::test]
async fn event_types_are_accepted_in_either_spelling() {
    let svc = SesService::new();
    svc.handle(
        "CreateConfigurationSet",
        json!({ "ConfigurationSet": { "Name": "cs" }}),
        &ctx(),
    )
    .await
    .expect("CreateConfigurationSet");

    for (name, types) in [
        ("classic", json!(["send", "renderingFailure"])),
        ("modern", json!(["SEND", "RENDERING_FAILURE"])),
    ] {
        svc.handle(
            "CreateConfigurationSetEventDestination",
            json!({
                "ConfigurationSetName": "cs",
                "EventDestination": { "Name": name, "MatchingEventTypes": types },
            }),
            &ctx(),
        )
        .await
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));
    }

    let err = svc
        .handle(
            "CreateConfigurationSetEventDestination",
            json!({
                "ConfigurationSetName": "cs",
                "EventDestination": { "Name": "bad", "MatchingEventTypes": ["explode"] },
            }),
            &ctx(),
        )
        .await
        .expect_err("a misspelled type still has to fail");
    assert_eq!(err.code, "BadRequestException", "{err:?}");
}

/// The v2 `EmailContent` union member is `Template`. AWSim read
/// `Templated`, which is not an AWS name, so every templated send from a
/// real SDK fell through to "Content must include Simple, Raw, or
/// Template" instead of rendering.
#[tokio::test]
async fn v2_templated_send_uses_the_template_union_member() {
    let svc = service_with_identities().await;
    svc.handle(
        "CreateEmailTemplate",
        json!({
            "TemplateName": "welcome",
            "TemplateContent": {
                "Subject": "Hi {{name}}",
                "Text": "Hello {{name}}",
                "Html": "<p>Hello {{name}}</p>",
            },
        }),
        &ctx(),
    )
    .await
    .expect("CreateEmailTemplate");

    svc.handle(
        "SendEmail",
        json!({
            "FromEmailAddress": "dev@example.com",
            "Destination": { "ToAddresses": ["a@b.c"] },
            "Content": { "Template": {
                "TemplateName": "welcome",
                "TemplateData": r#"{"name":"Ada"}"#,
            }},
        }),
        &ctx(),
    )
    .await
    .expect("SendEmail");

    let sent = svc.list_sent_emails();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].2.subject.as_deref(), Some("Hi Ada"));
    assert_eq!(sent[0].2.body_text.as_deref(), Some("Hello Ada"));
    assert_eq!(sent[0].2.body_html.as_deref(), Some("<p>Hello Ada</p>"));
}
