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
