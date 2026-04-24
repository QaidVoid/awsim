use crate::chk;
use crate::runner::common::*;

pub async fn test_route53(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_route53::Client::new(&config);
    let mut results = Vec::new();

    // CreateHostedZone
    let create_zone_r = client
        .create_hosted_zone()
        .name("conformance.example.com.")
        .caller_reference(uuid::Uuid::new_v4().to_string())
        .send()
        .await;
    let zone_id = create_zone_r
        .as_ref()
        .ok()
        .and_then(|r| r.hosted_zone.as_ref())
        .map(|z| z.id.clone());
    results.push(chk!("CreateHostedZone", create_zone_r, verbose));

    // ListHostedZones
    results.push(chk!(
        "ListHostedZones",
        client.list_hosted_zones().send().await,
        verbose
    ));

    // GetHostedZoneCount
    results.push(chk!(
        "GetHostedZoneCount",
        client.get_hosted_zone_count().send().await,
        verbose
    ));

    if let Some(ref zid) = zone_id {
        // GetHostedZone
        results.push(chk!(
            "GetHostedZone",
            client.get_hosted_zone().id(zid).send().await,
            verbose
        ));

        // ChangeResourceRecordSets (add an A record)
        results.push(chk!(
            "ChangeResourceRecordSets",
            client
                .change_resource_record_sets()
                .hosted_zone_id(zid)
                .change_batch(
                    aws_sdk_route53::types::ChangeBatch::builder()
                        .changes(
                            aws_sdk_route53::types::Change::builder()
                                .action(aws_sdk_route53::types::ChangeAction::Create)
                                .resource_record_set(
                                    aws_sdk_route53::types::ResourceRecordSet::builder()
                                        .name("www.conformance.example.com.")
                                        .r#type(aws_sdk_route53::types::RrType::A)
                                        .ttl(300)
                                        .resource_records(
                                            aws_sdk_route53::types::ResourceRecord::builder()
                                                .value("1.2.3.4")
                                                .build()
                                                .unwrap(),
                                        )
                                        .build()
                                        .unwrap(),
                                )
                                .build()
                                .unwrap(),
                        )
                        .build()
                        .unwrap(),
                )
                .send()
                .await,
            verbose
        ));

        // ListResourceRecordSets
        results.push(chk!(
            "ListResourceRecordSets",
            client
                .list_resource_record_sets()
                .hosted_zone_id(zid)
                .send()
                .await,
            verbose
        ));

        // DeleteHostedZone
        results.push(chk!(
            "DeleteHostedZone",
            client.delete_hosted_zone().id(zid).send().await,
            verbose
        ));
    } else {
        for op in &[
            "GetHostedZone",
            "ChangeResourceRecordSets",
            "ListResourceRecordSets",
            "DeleteHostedZone",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    // CreateHealthCheck
    let create_hc_r = client
        .create_health_check()
        .caller_reference(uuid::Uuid::new_v4().to_string())
        .health_check_config(
            aws_sdk_route53::types::HealthCheckConfig::builder()
                .ip_address("1.2.3.4")
                .port(80)
                .r#type(aws_sdk_route53::types::HealthCheckType::Http)
                .resource_path("/")
                .request_interval(30)
                .failure_threshold(3)
                .build()
                .unwrap(),
        )
        .send()
        .await;
    let health_check_id = create_hc_r
        .as_ref()
        .ok()
        .and_then(|r| r.health_check.as_ref())
        .map(|h| h.id.clone());
    results.push(chk!("CreateHealthCheck", create_hc_r, verbose));

    // ListHealthChecks
    results.push(chk!(
        "ListHealthChecks",
        client.list_health_checks().send().await,
        verbose
    ));

    // GetHealthCheck
    if let Some(ref hcid) = health_check_id {
        results.push(chk!(
            "GetHealthCheck",
            client.get_health_check().health_check_id(hcid).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetHealthCheck".to_string()));
    }

    // ListGeoLocations
    results.push(chk!(
        "ListGeoLocations",
        client.list_geo_locations().send().await,
        verbose
    ));

    // ListReusableDelegationSets
    results.push(chk!(
        "ListReusableDelegationSets",
        client.list_reusable_delegation_sets().send().await,
        verbose
    ));

    // DeleteHealthCheck
    if let Some(ref hcid) = health_check_id {
        results.push(chk!(
            "DeleteHealthCheck",
            client
                .delete_health_check()
                .health_check_id(hcid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteHealthCheck".to_string()));
    }

    results
}
