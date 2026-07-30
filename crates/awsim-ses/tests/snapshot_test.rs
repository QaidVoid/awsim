//! SES configuration has to survive a snapshot/restore cycle.
//!
//! SES was one of the services that reported no snapshot support at all,
//! so a save/load round trip silently dropped every verified identity,
//! template, configuration set, and receipt rule. Rebuilding that by
//! hand is the whole reason snapshots exist.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_ses::SesService;
use serde_json::{Value, json};

fn ctx() -> RequestContext {
    RequestContext::new_with_account("ses", "us-east-1", "000000000000")
}

/// A second account/region, to prove the snapshot is not flattened into
/// one bucket on the way back in.
fn other_ctx() -> RequestContext {
    RequestContext::new_with_account("ses", "eu-west-1", "111111111111")
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

/// Populate one account with a value from every corner of the state.
async fn seed(svc: &SesService, ctx: &RequestContext) {
    svc.handle(
        "VerifyDomainIdentity",
        json!({ "Domain": "example.com" }),
        ctx,
    )
    .await
    .expect("VerifyDomainIdentity");
    svc.handle(
        "SetIdentityNotificationTopic",
        json!({
            "Identity": "example.com",
            "NotificationType": "Bounce",
            "SnsTopic": "arn:aws:sns:us-east-1:000000000000:bounces",
        }),
        ctx,
    )
    .await
    .expect("SetIdentityNotificationTopic");
    svc.handle(
        "SetIdentityMailFromDomain",
        json!({ "Identity": "example.com", "MailFromDomain": "mail.example.com" }),
        ctx,
    )
    .await
    .expect("SetIdentityMailFromDomain");
    svc.handle(
        "PutIdentityPolicy",
        json!({ "Identity": "example.com", "PolicyName": "p1", "Policy": "{}" }),
        ctx,
    )
    .await
    .expect("PutIdentityPolicy");

    svc.handle(
        "CreateTemplate",
        json!({ "Template": { "TemplateName": "welcome", "SubjectPart": "Hi {{name}}" }}),
        ctx,
    )
    .await
    .expect("CreateTemplate");

    svc.handle(
        "CreateConfigurationSet",
        json!({ "ConfigurationSet": { "Name": "cs" }}),
        ctx,
    )
    .await
    .expect("CreateConfigurationSet");
    svc.handle(
        "CreateConfigurationSetEventDestination",
        json!({
            "ConfigurationSetName": "cs",
            "EventDestination": {
                "Name": "d1",
                "MatchingEventTypes": ["send"],
                "SNSDestination": { "TopicARN": "arn:aws:sns:us-east-1:000000000000:events" },
            },
        }),
        ctx,
    )
    .await
    .expect("CreateConfigurationSetEventDestination");
    svc.handle(
        "CreateConfigurationSetTrackingOptions",
        json!({
            "ConfigurationSetName": "cs",
            "TrackingOptions": { "CustomRedirectDomain": "click.example.com" },
        }),
        ctx,
    )
    .await
    .expect("CreateConfigurationSetTrackingOptions");

    svc.handle("CreateReceiptRuleSet", json!({ "RuleSetName": "rs" }), ctx)
        .await
        .expect("CreateReceiptRuleSet");
    svc.handle(
        "CreateReceiptRule",
        json!({ "RuleSetName": "rs", "Rule": { "Name": "r1" } }),
        ctx,
    )
    .await
    .expect("CreateReceiptRule");
    svc.handle(
        "CreateReceiptFilter",
        json!({ "Filter": {
            "Name": "blocklist",
            "IpFilter": { "Policy": "Block", "Cidr": "10.0.0.0/24" },
        }}),
        ctx,
    )
    .await
    .expect("CreateReceiptFilter");

    svc.handle(
        "UpdateAccountSendingEnabled",
        json!({ "Enabled": false }),
        ctx,
    )
    .await
    .expect("UpdateAccountSendingEnabled");
}

async fn restored_from(source: &SesService) -> SesService {
    let bytes = source.snapshot().expect("SES should support snapshots");
    let target = SesService::new();
    target.restore(&bytes).expect("restore should succeed");
    target
}

#[tokio::test]
async fn identities_and_their_settings_survive() {
    let source = SesService::new();
    seed(&source, &ctx()).await;
    let restored = restored_from(&source).await;

    let listed = restored
        .handle("ListIdentities", json!({}), &ctx())
        .await
        .expect("ListIdentities");
    assert_eq!(members(&listed, "Identities"), vec![json!("example.com")]);

    let notifications = restored
        .handle(
            "GetIdentityNotificationAttributes",
            json!({ "Identities": ["example.com"] }),
            &ctx(),
        )
        .await
        .expect("GetIdentityNotificationAttributes");
    assert_eq!(
        entry(&notifications, "NotificationAttributes", "example.com").unwrap()["BounceTopic"],
        "arn:aws:sns:us-east-1:000000000000:bounces"
    );

    let mail_from = restored
        .handle(
            "GetIdentityMailFromDomainAttributes",
            json!({ "Identities": ["example.com"] }),
            &ctx(),
        )
        .await
        .expect("GetIdentityMailFromDomainAttributes");
    assert_eq!(
        entry(&mail_from, "MailFromDomainAttributes", "example.com").unwrap()["MailFromDomain"],
        "mail.example.com"
    );

    let policies = restored
        .handle(
            "ListIdentityPolicies",
            json!({ "Identity": "example.com" }),
            &ctx(),
        )
        .await
        .expect("ListIdentityPolicies");
    assert_eq!(members(&policies, "PolicyNames"), vec![json!("p1")]);
}

#[tokio::test]
async fn templates_survive() {
    let source = SesService::new();
    seed(&source, &ctx()).await;
    let restored = restored_from(&source).await;

    let got = restored
        .handle("GetTemplate", json!({ "TemplateName": "welcome" }), &ctx())
        .await
        .expect("GetTemplate");
    assert_eq!(got["Template"]["SubjectPart"], "Hi {{name}}");
}

#[tokio::test]
async fn configuration_sets_survive_with_their_destinations() {
    let source = SesService::new();
    seed(&source, &ctx()).await;
    let restored = restored_from(&source).await;

    let described = restored
        .handle(
            "DescribeConfigurationSet",
            json!({ "ConfigurationSetName": "cs" }),
            &ctx(),
        )
        .await
        .expect("DescribeConfigurationSet");
    let destinations = members(&described, "EventDestinations");
    assert_eq!(destinations.len(), 1, "{described}");
    assert_eq!(
        destinations[0]["SNSDestination"]["TopicARN"],
        "arn:aws:sns:us-east-1:000000000000:events"
    );
    assert_eq!(
        described["TrackingOptions"]["CustomRedirectDomain"],
        "click.example.com"
    );
}

#[tokio::test]
async fn receipt_rules_and_filters_survive() {
    let source = SesService::new();
    seed(&source, &ctx()).await;
    let restored = restored_from(&source).await;

    let rules = restored
        .handle(
            "DescribeReceiptRuleSet",
            json!({ "RuleSetName": "rs" }),
            &ctx(),
        )
        .await
        .expect("DescribeReceiptRuleSet");
    assert_eq!(members(&rules, "Rules")[0]["Name"], "r1");

    let filters = restored
        .handle("ListReceiptFilters", json!({}), &ctx())
        .await
        .expect("ListReceiptFilters");
    assert_eq!(members(&filters, "Filters")[0]["Name"], "blocklist");
}

/// The account switch lives behind a mutex rather than in a map, which
/// is exactly the kind of field a hand-written mirror type forgets.
#[tokio::test]
async fn the_account_sending_switch_survives() {
    let source = SesService::new();
    seed(&source, &ctx()).await;
    let restored = restored_from(&source).await;

    let enabled = restored
        .handle("GetAccountSendingEnabled", json!({}), &ctx())
        .await
        .expect("GetAccountSendingEnabled");
    assert_eq!(enabled["Enabled"], json!(false));
}

#[tokio::test]
async fn each_account_and_region_stays_separate() {
    let source = SesService::new();
    seed(&source, &ctx()).await;
    source
        .handle(
            "VerifyDomainIdentity",
            json!({ "Domain": "other.example" }),
            &other_ctx(),
        )
        .await
        .expect("VerifyDomainIdentity");

    let restored = restored_from(&source).await;

    let first = restored
        .handle("ListIdentities", json!({}), &ctx())
        .await
        .expect("ListIdentities");
    assert_eq!(members(&first, "Identities"), vec![json!("example.com")]);

    let second = restored
        .handle("ListIdentities", json!({}), &other_ctx())
        .await
        .expect("ListIdentities");
    assert_eq!(members(&second, "Identities"), vec![json!("other.example")]);
}

/// Restoring replaces rather than merges, so state written after the
/// snapshot must not linger.
#[tokio::test]
async fn restore_replaces_existing_state() {
    let source = SesService::new();
    seed(&source, &ctx()).await;
    let bytes = source.snapshot().expect("snapshot");

    let target = SesService::new();
    target
        .handle(
            "VerifyDomainIdentity",
            json!({ "Domain": "stale.example" }),
            &ctx(),
        )
        .await
        .expect("VerifyDomainIdentity");
    target.restore(&bytes).expect("restore");

    let listed = target
        .handle("ListIdentities", json!({}), &ctx())
        .await
        .expect("ListIdentities");
    assert_eq!(
        members(&listed, "Identities"),
        vec![json!("example.com")],
        "the pre-restore identity should be gone"
    );
}

/// A snapshot taken before a field existed still has to load. Without
/// this, adding one map to the state would break every saved snapshot.
#[tokio::test]
async fn a_snapshot_missing_newer_fields_still_loads() {
    let sparse = br#"[{"account_id":"000000000000","region":"us-east-1","state":{}}]"#;
    let svc = SesService::new();
    svc.restore(sparse)
        .expect("a snapshot with only the known fields should load");

    let listed = svc
        .handle("ListIdentities", json!({}), &ctx())
        .await
        .expect("ListIdentities");
    assert!(members(&listed, "Identities").is_empty());
}

/// Sending still works against restored state, which is the point of
/// keeping the identities rather than just listing them back.
#[tokio::test]
async fn a_restored_identity_can_still_send() {
    let source = SesService::new();
    seed(&source, &ctx()).await;
    let restored = restored_from(&source).await;

    let out = restored
        .handle(
            "SendEmail",
            json!({
                "Source": "dev@example.com",
                "Destination": { "ToAddresses": ["a@b.c"] },
                "Message": { "Subject": { "Data": "Hi" }, "Body": { "Text": { "Data": "yo" } } },
            }),
            &ctx(),
        )
        .await
        .expect("SendEmail");
    assert!(out["MessageId"].as_str().is_some(), "{out}");
}
