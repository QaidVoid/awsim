use crate::chk;
use crate::runner::common::*;

pub async fn test_elb(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_elasticloadbalancingv2::Client::new(&config);
    let mut results = Vec::new();

    // CreateLoadBalancer
    let create_lb_r = client
        .create_load_balancer()
        .name("conformance-lb")
        .r#type(aws_sdk_elasticloadbalancingv2::types::LoadBalancerTypeEnum::Application)
        .scheme(aws_sdk_elasticloadbalancingv2::types::LoadBalancerSchemeEnum::InternetFacing)
        .send()
        .await;
    let lb_arn = create_lb_r
        .as_ref()
        .ok()
        .and_then(|r| r.load_balancers.as_ref())
        .and_then(|lbs| lbs.first())
        .and_then(|lb| lb.load_balancer_arn.clone());
    results.push(chk!("CreateLoadBalancer", create_lb_r, verbose));

    // DescribeLoadBalancers
    results.push(chk!(
        "DescribeLoadBalancers",
        client.describe_load_balancers().send().await,
        verbose
    ));

    // CreateTargetGroup
    let create_tg_r = client
        .create_target_group()
        .name("conformance-tg")
        .protocol(aws_sdk_elasticloadbalancingv2::types::ProtocolEnum::Http)
        .port(80)
        .target_type(aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum::Instance)
        .vpc_id("vpc-00000000")
        .send()
        .await;
    let tg_arn = create_tg_r
        .as_ref()
        .ok()
        .and_then(|r| r.target_groups.as_ref())
        .and_then(|tgs| tgs.first())
        .and_then(|tg| tg.target_group_arn.clone());
    results.push(chk!("CreateTargetGroup", create_tg_r, verbose));

    // DescribeTargetGroups
    results.push(chk!(
        "DescribeTargetGroups",
        client.describe_target_groups().send().await,
        verbose
    ));

    // DescribeLoadBalancerAttributes
    if let Some(ref arn) = lb_arn {
        results.push(chk!(
            "DescribeLoadBalancerAttributes",
            client
                .describe_load_balancer_attributes()
                .load_balancer_arn(arn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped(
            "DescribeLoadBalancerAttributes".to_string(),
        ));
    }

    // DescribeTargetGroupAttributes
    if let Some(ref arn) = tg_arn {
        results.push(chk!(
            "DescribeTargetGroupAttributes",
            client
                .describe_target_group_attributes()
                .target_group_arn(arn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped(
            "DescribeTargetGroupAttributes".to_string(),
        ));
    }

    // CreateListener (requires lb + tg arns)
    let listener_arn = if let (Some(l_arn), Some(t_arn)) = (&lb_arn, &tg_arn) {
        let create_listener_r = client
            .create_listener()
            .load_balancer_arn(l_arn)
            .protocol(aws_sdk_elasticloadbalancingv2::types::ProtocolEnum::Http)
            .port(80)
            .default_actions(
                aws_sdk_elasticloadbalancingv2::types::Action::builder()
                    .r#type(aws_sdk_elasticloadbalancingv2::types::ActionTypeEnum::Forward)
                    .target_group_arn(t_arn)
                    .build(),
            )
            .send()
            .await;
        let arn = create_listener_r
            .as_ref()
            .ok()
            .and_then(|r| r.listeners.as_ref())
            .and_then(|ls| ls.first())
            .and_then(|l| l.listener_arn.clone());
        results.push(chk!("CreateListener", create_listener_r, verbose));
        arn
    } else {
        results.push(OpResult::Skipped("CreateListener".to_string()));
        None
    };

    // DescribeListeners
    results.push(chk!(
        "DescribeListeners",
        client.describe_listeners().send().await,
        verbose
    ));

    // ModifyLoadBalancerAttributes
    if let Some(ref arn) = lb_arn {
        results.push(chk!(
            "ModifyLoadBalancerAttributes",
            client
                .modify_load_balancer_attributes()
                .load_balancer_arn(arn)
                .attributes(
                    aws_sdk_elasticloadbalancingv2::types::LoadBalancerAttribute::builder()
                        .key("idle_timeout.timeout_seconds")
                        .value("120")
                        .build(),
                )
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped(
            "ModifyLoadBalancerAttributes".to_string(),
        ));
    }

    // SetSecurityGroups
    if let Some(ref arn) = lb_arn {
        results.push(chk!(
            "SetSecurityGroups",
            client
                .set_security_groups()
                .load_balancer_arn(arn)
                .security_groups("sg-00000000")
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("SetSecurityGroups".to_string()));
    }

    // SetSubnets
    if let Some(ref arn) = lb_arn {
        results.push(chk!(
            "SetSubnets",
            client
                .set_subnets()
                .load_balancer_arn(arn)
                .subnets("subnet-00000000")
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("SetSubnets".to_string()));
    }

    // DescribeTargetHealth
    if let Some(ref arn) = tg_arn {
        results.push(chk!(
            "DescribeTargetHealth",
            client
                .describe_target_health()
                .target_group_arn(arn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DescribeTargetHealth".to_string()));
    }

    // AddTags
    if let Some(ref arn) = lb_arn {
        results.push(chk!(
            "AddTags",
            client
                .add_tags()
                .resource_arns(arn)
                .tags(
                    aws_sdk_elasticloadbalancingv2::types::Tag::builder()
                        .key("env")
                        .value("conformance")
                        .build(),
                )
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("AddTags".to_string()));
    }

    // DescribeTags
    if let Some(ref arn) = lb_arn {
        results.push(chk!(
            "DescribeTags",
            client.describe_tags().resource_arns(arn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DescribeTags".to_string()));
    }

    // RemoveTags
    if let Some(ref arn) = lb_arn {
        results.push(chk!(
            "RemoveTags",
            client
                .remove_tags()
                .resource_arns(arn)
                .tag_keys("env")
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("RemoveTags".to_string()));
    }

    // DescribeListenerCertificates
    if let Some(ref l_arn) = listener_arn {
        results.push(chk!(
            "DescribeListenerCertificates",
            client
                .describe_listener_certificates()
                .listener_arn(l_arn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped(
            "DescribeListenerCertificates".to_string(),
        ));
    }

    // AddListenerCertificates
    if let Some(ref l_arn) = listener_arn {
        results.push(chk!(
            "AddListenerCertificates",
            client
                .add_listener_certificates()
                .listener_arn(l_arn)
                .certificates(
                    aws_sdk_elasticloadbalancingv2::types::Certificate::builder()
                        .certificate_arn(
                            "arn:aws:acm:us-east-1:000000000000:certificate/conformance",
                        )
                        .is_default(true)
                        .build(),
                )
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("AddListenerCertificates".to_string()));
    }

    // RemoveListenerCertificates
    if let Some(ref l_arn) = listener_arn {
        results.push(chk!(
            "RemoveListenerCertificates",
            client
                .remove_listener_certificates()
                .listener_arn(l_arn)
                .certificates(
                    aws_sdk_elasticloadbalancingv2::types::Certificate::builder()
                        .certificate_arn(
                            "arn:aws:acm:us-east-1:000000000000:certificate/conformance",
                        )
                        .build(),
                )
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("RemoveListenerCertificates".to_string()));
    }

    // CreateRule
    let rule_arn = if let (Some(l_arn), Some(t_arn)) = (&listener_arn, &tg_arn) {
        let create_rule_r = client
            .create_rule()
            .listener_arn(l_arn)
            .priority(10)
            .conditions(
                aws_sdk_elasticloadbalancingv2::types::RuleCondition::builder()
                    .field("path-pattern")
                    .values("/api/*")
                    .build(),
            )
            .actions(
                aws_sdk_elasticloadbalancingv2::types::Action::builder()
                    .r#type(aws_sdk_elasticloadbalancingv2::types::ActionTypeEnum::Forward)
                    .target_group_arn(t_arn)
                    .build(),
            )
            .send()
            .await;
        let arn = create_rule_r
            .as_ref()
            .ok()
            .and_then(|r| r.rules.as_ref())
            .and_then(|rs| rs.first())
            .and_then(|rule| rule.rule_arn.clone());
        results.push(chk!("CreateRule", create_rule_r, verbose));
        arn
    } else {
        results.push(OpResult::Skipped("CreateRule".to_string()));
        None
    };

    // DescribeRules
    if let Some(ref l_arn) = listener_arn {
        results.push(chk!(
            "DescribeRules",
            client.describe_rules().listener_arn(l_arn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DescribeRules".to_string()));
    }

    // ModifyRule
    if let (Some(r_arn), Some(t_arn)) = (&rule_arn, &tg_arn) {
        results.push(chk!(
            "ModifyRule",
            client
                .modify_rule()
                .rule_arn(r_arn)
                .actions(
                    aws_sdk_elasticloadbalancingv2::types::Action::builder()
                        .r#type(aws_sdk_elasticloadbalancingv2::types::ActionTypeEnum::Forward)
                        .target_group_arn(t_arn)
                        .build(),
                )
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("ModifyRule".to_string()));
    }

    // SetRulePriorities
    if let Some(ref r_arn) = rule_arn {
        results.push(chk!(
            "SetRulePriorities",
            client
                .set_rule_priorities()
                .rule_priorities(
                    aws_sdk_elasticloadbalancingv2::types::RulePriorityPair::builder()
                        .rule_arn(r_arn)
                        .priority(20)
                        .build(),
                )
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("SetRulePriorities".to_string()));
    }

    // DeleteRule
    if let Some(ref r_arn) = rule_arn {
        results.push(chk!(
            "DeleteRule",
            client.delete_rule().rule_arn(r_arn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteRule".to_string()));
    }

    // DeleteListener
    if let Some(ref l_arn) = listener_arn {
        results.push(chk!(
            "DeleteListener",
            client.delete_listener().listener_arn(l_arn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteListener".to_string()));
    }

    // DeleteTargetGroup
    if let Some(ref arn) = tg_arn {
        results.push(chk!(
            "DeleteTargetGroup",
            client
                .delete_target_group()
                .target_group_arn(arn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteTargetGroup".to_string()));
    }

    // DescribeAccountLimits
    results.push(chk!(
        "DescribeAccountLimits",
        client.describe_account_limits().send().await,
        verbose
    ));

    // DescribeSSLPolicies
    results.push(chk!(
        "DescribeSSLPolicies",
        client.describe_ssl_policies().send().await,
        verbose
    ));

    // DeleteLoadBalancer
    if let Some(ref arn) = lb_arn {
        results.push(chk!(
            "DeleteLoadBalancer",
            client
                .delete_load_balancer()
                .load_balancer_arn(arn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteLoadBalancer".to_string()));
    }

    results
}
