//! Route53 speaks restXml, where a list nests each element in its own
//! singular tag: `<HostedZones><HostedZone>...</HostedZone></HostedZones>`.
//!
//! Emitting the members directly under the plural tag produced XML that
//! botocore read as one empty struct per child element, so
//! `aws route53 list-hosted-zones` returned `[{}, {}, {}, {}, {}]`.

use std::sync::Arc;

use awsim_core::{RequestContext, ServiceHandler};
use awsim_route53::Route53Service;
use serde_json::{Value, json};

fn ctx() -> RequestContext {
    RequestContext::new_with_account("route53", "us-east-1", "000000000000")
}

async fn with_zone() -> (Arc<Route53Service>, String) {
    let svc = Arc::new(Route53Service::new());
    let created = svc
        .handle(
            "CreateHostedZone",
            json!({ "Name": "example.com", "CallerReference": "ref-1" }),
            &ctx(),
        )
        .await
        .expect("CreateHostedZone");
    let id = created["HostedZone"]["Id"]
        .as_str()
        .expect("Id")
        .to_string();
    (svc, id)
}

fn singular<'a>(response: &'a Value, plural: &str, tag: &str) -> &'a Vec<Value> {
    response[plural][tag]
        .as_array()
        .unwrap_or_else(|| panic!("{plural} should nest a {tag} list: {response}"))
}

#[tokio::test]
async fn hosted_zones_nest_each_zone_in_its_own_tag() {
    let (svc, _) = with_zone().await;

    for op in ["ListHostedZones", "ListHostedZonesByName"] {
        let out = svc
            .handle(op, json!({ "DNSName": "example.com" }), &ctx())
            .await
            .unwrap_or_else(|e| panic!("{op}: {e:?}"));
        let zones = singular(&out, "HostedZones", "HostedZone");
        assert_eq!(zones.len(), 1, "{op}: {out}");
        assert_eq!(zones[0]["Name"], "example.com.", "{op}");
    }
}

#[tokio::test]
async fn create_hosted_zone_nests_name_servers() {
    let (svc, _) = with_zone().await;
    let created = svc
        .handle(
            "CreateHostedZone",
            json!({ "Name": "other.com", "CallerReference": "ref-2" }),
            &ctx(),
        )
        .await
        .expect("CreateHostedZone");

    let servers = created["DelegationSet"]["NameServers"]["NameServer"]
        .as_array()
        .unwrap_or_else(|| panic!("NameServers should nest NameServer: {created}"));
    assert_eq!(servers.len(), 4);
}

#[tokio::test]
async fn record_sets_nest_each_set_in_its_own_tag() {
    let (svc, zone_id) = with_zone().await;

    let out = svc
        .handle(
            "ListResourceRecordSets",
            json!({ "Id": zone_id.clone() }),
            &ctx(),
        )
        .await
        .expect("ListResourceRecordSets");
    let sets = singular(&out, "ResourceRecordSets", "ResourceRecordSet");
    assert!(
        sets.iter().any(|s| s["Type"] == "NS"),
        "a new zone carries its NS record: {out}"
    );
}

#[tokio::test]
async fn health_checks_nest_each_check_in_its_own_tag() {
    let svc = Arc::new(Route53Service::new());
    let out = svc
        .handle("ListHealthChecks", json!({}), &ctx())
        .await
        .expect("ListHealthChecks");
    // An empty collection still has to carry the nesting, otherwise a
    // client sees a missing member rather than an empty list.
    assert!(
        out["HealthChecks"].get("HealthCheck").is_some(),
        "HealthChecks should nest HealthCheck even when empty: {out}"
    );
}

/// A one-element XML list parses as an object, not an array, because the
/// parser has no schema to tell them apart. Reading it as an array only
/// meant a single-record change answered 200 and changed nothing.
#[tokio::test]
async fn a_single_change_is_applied() {
    let (svc, zone_id) = with_zone().await;

    svc.handle(
        "ChangeResourceRecordSets",
        json!({
            "Id": zone_id.clone(),
            "ChangeBatch": { "Changes": { "Change": {
                "Action": "CREATE",
                "ResourceRecordSet": {
                    "Name": "www.example.com",
                    "Type": "A",
                    "TTL": 300,
                    "ResourceRecords": { "ResourceRecord": { "Value": "10.0.0.1" } },
                },
            }}},
        }),
        &ctx(),
    )
    .await
    .expect("ChangeResourceRecordSets");

    let out = svc
        .handle("ListResourceRecordSets", json!({ "Id": zone_id }), &ctx())
        .await
        .expect("ListResourceRecordSets");
    let sets = singular(&out, "ResourceRecordSets", "ResourceRecordSet");
    let created = sets
        .iter()
        .find(|s| s["Name"] == "www.example.com")
        .unwrap_or_else(|| panic!("the single change should have been applied: {out}"));
    assert_eq!(created["Type"], "A");
    assert_eq!(
        created["ResourceRecords"]["ResourceRecord"][0]["Value"],
        "10.0.0.1"
    );
}

/// The array form still has to work, so a multi-change batch is not
/// regressed by accepting the single form.
#[tokio::test]
async fn a_multi_change_batch_is_applied() {
    let (svc, zone_id) = with_zone().await;

    svc.handle(
        "ChangeResourceRecordSets",
        json!({
            "Id": zone_id.clone(),
            "ChangeBatch": { "Changes": { "Change": [
                {
                    "Action": "CREATE",
                    "ResourceRecordSet": {
                        "Name": "a.example.com", "Type": "A", "TTL": 60,
                        "ResourceRecords": { "ResourceRecord": [
                            { "Value": "10.0.0.2" }, { "Value": "10.0.0.3" },
                        ]},
                    },
                },
                {
                    "Action": "CREATE",
                    "ResourceRecordSet": {
                        "Name": "b.example.com", "Type": "CNAME", "TTL": 60,
                        "ResourceRecords": { "ResourceRecord": { "Value": "example.net" } },
                    },
                },
            ]}},
        }),
        &ctx(),
    )
    .await
    .expect("ChangeResourceRecordSets");

    let out = svc
        .handle("ListResourceRecordSets", json!({ "Id": zone_id }), &ctx())
        .await
        .expect("ListResourceRecordSets");
    let sets = singular(&out, "ResourceRecordSets", "ResourceRecordSet");
    let a = sets
        .iter()
        .find(|s| s["Name"] == "a.example.com")
        .expect("a");
    assert_eq!(
        a["ResourceRecords"]["ResourceRecord"]
            .as_array()
            .map(Vec::len),
        Some(2),
        "both values should survive: {a}"
    );
    assert!(sets.iter().any(|s| s["Name"] == "b.example.com"));
}
