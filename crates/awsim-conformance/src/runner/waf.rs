use crate::chk;
use crate::runner::common::*;

pub async fn test_waf(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_wafv2::Client::new(&config);
    let mut results = Vec::new();

    let scope = aws_sdk_wafv2::types::Scope::Regional;

    // CreateWebACL
    let create_acl_r = client
        .create_web_acl()
        .name("conformance-web-acl")
        .scope(scope.clone())
        .default_action(
            aws_sdk_wafv2::types::DefaultAction::builder()
                .allow(aws_sdk_wafv2::types::AllowAction::builder().build())
                .build(),
        )
        .visibility_config(
            aws_sdk_wafv2::types::VisibilityConfig::builder()
                .cloud_watch_metrics_enabled(false)
                .metric_name("conformance-web-acl")
                .sampled_requests_enabled(false)
                .build()
                .unwrap(),
        )
        .send()
        .await;
    let (acl_id, acl_lock_token) = create_acl_r
        .as_ref()
        .ok()
        .and_then(|r| r.summary.as_ref())
        .map(|s| (s.id.clone(), s.lock_token.clone()))
        .unwrap_or((None, None));
    results.push(chk!("CreateWebACL", create_acl_r, verbose));

    // ListWebACLs
    results.push(chk!(
        "ListWebACLs",
        client.list_web_acls().scope(scope.clone()).send().await,
        verbose
    ));

    if let (Some(id), Some(token)) = (&acl_id, &acl_lock_token) {
        // GetWebACL
        results.push(chk!(
            "GetWebACL",
            client
                .get_web_acl()
                .name("conformance-web-acl")
                .scope(scope.clone())
                .id(id)
                .send()
                .await,
            verbose
        ));

        // DeleteWebACL
        results.push(chk!(
            "DeleteWebACL",
            client
                .delete_web_acl()
                .name("conformance-web-acl")
                .scope(scope.clone())
                .id(id)
                .lock_token(token)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetWebACL".to_string()));
        results.push(OpResult::Skipped("DeleteWebACL".to_string()));
    }

    // CreateIPSet
    let create_ip_r = client
        .create_ip_set()
        .name("conformance-ip-set")
        .scope(scope.clone())
        .ip_address_version(aws_sdk_wafv2::types::IpAddressVersion::Ipv4)
        .addresses("1.2.3.4/32")
        .send()
        .await;
    let (ip_set_id, ip_lock_token) = create_ip_r
        .as_ref()
        .ok()
        .and_then(|r| r.summary.as_ref())
        .map(|s| (s.id.clone(), s.lock_token.clone()))
        .unwrap_or((None, None));
    results.push(chk!("CreateIPSet", create_ip_r, verbose));

    // ListIPSets
    results.push(chk!(
        "ListIPSets",
        client.list_ip_sets().scope(scope.clone()).send().await,
        verbose
    ));

    // GetIPSet
    if let (Some(id), Some(_token)) = (&ip_set_id, &ip_lock_token) {
        results.push(chk!(
            "GetIPSet",
            client
                .get_ip_set()
                .name("conformance-ip-set")
                .scope(scope.clone())
                .id(id)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetIPSet".to_string()));
    }

    // CheckCapacity
    results.push(chk!(
        "CheckCapacity",
        client.check_capacity().scope(scope.clone()).send().await,
        verbose
    ));

    // ListAvailableManagedRuleGroups
    results.push(chk!(
        "ListAvailableManagedRuleGroups",
        client
            .list_available_managed_rule_groups()
            .scope(scope.clone())
            .send()
            .await,
        verbose
    ));

    // PutLoggingConfiguration
    let logging_resource_arn =
        "arn:aws:wafv2:us-east-1:000000000000:regional/webacl/conformance-logging/abc";
    let put_log_r = client
        .put_logging_configuration()
        .logging_configuration(
            aws_sdk_wafv2::types::LoggingConfiguration::builder()
                .resource_arn(logging_resource_arn)
                .log_destination_configs(
                    "arn:aws:logs:us-east-1:000000000000:log-group:aws-waf-logs-conformance",
                )
                .build()
                .unwrap(),
        )
        .send()
        .await;
    results.push(chk!("PutLoggingConfiguration", put_log_r, verbose));

    // GetLoggingConfiguration
    results.push(chk!(
        "GetLoggingConfiguration",
        client
            .get_logging_configuration()
            .resource_arn(logging_resource_arn)
            .send()
            .await,
        verbose
    ));

    // ListLoggingConfigurations
    results.push(chk!(
        "ListLoggingConfigurations",
        client
            .list_logging_configurations()
            .scope(scope.clone())
            .send()
            .await,
        verbose
    ));

    // DeleteLoggingConfiguration
    results.push(chk!(
        "DeleteLoggingConfiguration",
        client
            .delete_logging_configuration()
            .resource_arn(logging_resource_arn)
            .send()
            .await,
        verbose
    ));

    // DeleteIPSet
    if let (Some(id), Some(token)) = (&ip_set_id, &ip_lock_token) {
        results.push(chk!(
            "DeleteIPSet",
            client
                .delete_ip_set()
                .name("conformance-ip-set")
                .scope(scope.clone())
                .id(id)
                .lock_token(token)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteIPSet".to_string()));
    }

    results
}
