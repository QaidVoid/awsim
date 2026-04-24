use crate::chk;
use crate::runner::common::*;

pub async fn test_cloudfront(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_cloudfront::Client::new(&config);
    let mut results = Vec::new();

    // ListDistributions (empty)
    results.push(chk!(
        "ListDistributions",
        client.list_distributions().send().await,
        verbose
    ));

    // ListCachePolicies
    results.push(chk!(
        "ListCachePolicies",
        client.list_cache_policies().send().await,
        verbose
    ));

    // ListCloudFrontOriginAccessIdentities
    results.push(chk!(
        "ListCloudFrontOriginAccessIdentities",
        client.list_cloud_front_origin_access_identities().send().await,
        verbose
    ));

    // ListOriginAccessControls
    results.push(chk!(
        "ListOriginAccessControls",
        client.list_origin_access_controls().send().await,
        verbose
    ));

    // ListOriginRequestPolicies
    results.push(chk!(
        "ListOriginRequestPolicies",
        client.list_origin_request_policies().send().await,
        verbose
    ));

    // ListKeyGroups
    results.push(chk!(
        "ListKeyGroups",
        client.list_key_groups().send().await,
        verbose
    ));

    // ListPublicKeys
    results.push(chk!(
        "ListPublicKeys",
        client.list_public_keys().send().await,
        verbose
    ));

    // ListFunctions
    results.push(chk!(
        "ListFunctions",
        client.list_functions().send().await,
        verbose
    ));

    // CreateOriginRequestPolicy
    let create_orp_r = client
        .create_origin_request_policy()
        .origin_request_policy_config(
            aws_sdk_cloudfront::types::OriginRequestPolicyConfig::builder()
                .name("conformance-orp")
                .comment("conformance")
                .headers_config(
                    aws_sdk_cloudfront::types::OriginRequestPolicyHeadersConfig::builder()
                        .header_behavior(
                            aws_sdk_cloudfront::types::OriginRequestPolicyHeaderBehavior::None,
                        )
                        .build()
                        .unwrap(),
                )
                .cookies_config(
                    aws_sdk_cloudfront::types::OriginRequestPolicyCookiesConfig::builder()
                        .cookie_behavior(
                            aws_sdk_cloudfront::types::OriginRequestPolicyCookieBehavior::None,
                        )
                        .build()
                        .unwrap(),
                )
                .query_strings_config(
                    aws_sdk_cloudfront::types::OriginRequestPolicyQueryStringsConfig::builder()
                        .query_string_behavior(
                            aws_sdk_cloudfront::types::OriginRequestPolicyQueryStringBehavior::None,
                        )
                        .build()
                        .unwrap(),
                )
                .build()
                .unwrap(),
        )
        .send()
        .await;
    let orp_id = create_orp_r
        .as_ref()
        .ok()
        .and_then(|r| r.origin_request_policy.as_ref())
        .map(|p| p.id.clone());
    results.push(chk!("CreateOriginRequestPolicy", create_orp_r, verbose));

    // GetOriginRequestPolicy
    if let Some(ref id) = orp_id {
        results.push(chk!(
            "GetOriginRequestPolicy",
            client.get_origin_request_policy().id(id).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetOriginRequestPolicy".to_string()));
    }

    // CreateKeyGroup
    let create_kg_r = client
        .create_key_group()
        .key_group_config(
            aws_sdk_cloudfront::types::KeyGroupConfig::builder()
                .name("conformance-kg")
                .items("K1ABCDEFGHIJ")
                .comment("conformance")
                .build()
                .unwrap(),
        )
        .send()
        .await;
    let kg_id = create_kg_r
        .as_ref()
        .ok()
        .and_then(|r| r.key_group.as_ref())
        .map(|k| k.id.clone());
    results.push(chk!("CreateKeyGroup", create_kg_r, verbose));

    // GetKeyGroup
    if let Some(ref id) = kg_id {
        results.push(chk!(
            "GetKeyGroup",
            client.get_key_group().id(id).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetKeyGroup".to_string()));
    }

    // CreatePublicKey
    let create_pk_r = client
        .create_public_key()
        .public_key_config(
            aws_sdk_cloudfront::types::PublicKeyConfig::builder()
                .caller_reference("conformance-pk-ref")
                .name("conformance-pk")
                .encoded_key("-----BEGIN PUBLIC KEY-----\nMIIB\n-----END PUBLIC KEY-----")
                .comment("conformance")
                .build()
                .unwrap(),
        )
        .send()
        .await;
    let pk_id = create_pk_r
        .as_ref()
        .ok()
        .and_then(|r| r.public_key.as_ref())
        .map(|p| p.id.clone());
    results.push(chk!("CreatePublicKey", create_pk_r, verbose));

    // GetPublicKey
    if let Some(ref id) = pk_id {
        results.push(chk!(
            "GetPublicKey",
            client.get_public_key().id(id).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetPublicKey".to_string()));
    }

    // CreateFunction
    let create_fn_r = client
        .create_function()
        .name("conformance-fn")
        .function_config(
            aws_sdk_cloudfront::types::FunctionConfig::builder()
                .comment("conformance")
                .runtime(aws_sdk_cloudfront::types::FunctionRuntime::CloudfrontJs20)
                .build()
                .unwrap(),
        )
        .function_code(aws_sdk_cloudfront::primitives::Blob::new(
            "function handler(event){return event.request;}".as_bytes(),
        ))
        .send()
        .await;
    results.push(chk!("CreateFunction", create_fn_r, verbose));

    // PublishFunction
    results.push(chk!(
        "PublishFunction",
        client
            .publish_function()
            .name("conformance-fn")
            .if_match("etag")
            .send()
            .await,
        verbose
    ));

    // ListDistributionsByWebACLId
    results.push(chk!(
        "ListDistributionsByWebACLId",
        client
            .list_distributions_by_web_acl_id()
            .web_acl_id("conformance-acl")
            .send()
            .await,
        verbose
    ));

    // ListDistributionsByRealtimeLogConfig
    results.push(chk!(
        "ListDistributionsByRealtimeLogConfig",
        client
            .list_distributions_by_realtime_log_config()
            .realtime_log_config_name("conformance-rt")
            .send()
            .await,
        verbose
    ));

    // ListFieldLevelEncryptionConfigs
    results.push(chk!(
        "ListFieldLevelEncryptionConfigs",
        client.list_field_level_encryption_configs().send().await,
        verbose
    ));

    // ListRealtimeLogConfigs
    results.push(chk!(
        "ListRealtimeLogConfigs",
        client.list_realtime_log_configs().send().await,
        verbose
    ));

    results
}
