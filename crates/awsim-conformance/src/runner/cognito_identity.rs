use crate::chk;
use crate::runner::common::*;

pub async fn test_cognito_identity(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_cognitoidentity::Client::new(&config);
    let mut results = Vec::new();

    // CreateIdentityPool
    let create_r = client
        .create_identity_pool()
        .identity_pool_name("conformance-identity-pool")
        .allow_unauthenticated_identities(false)
        .send()
        .await;
    let pool_id = create_r
        .as_ref()
        .ok()
        .map(|r| r.identity_pool_id.clone());
    results.push(chk!("CreateIdentityPool", create_r, verbose));

    // ListIdentityPools
    results.push(chk!(
        "ListIdentityPools",
        client.list_identity_pools().max_results(10).send().await,
        verbose
    ));

    if let Some(ref pid) = pool_id {
        // DescribeIdentityPool
        results.push(chk!(
            "DescribeIdentityPool",
            client
                .describe_identity_pool()
                .identity_pool_id(pid)
                .send()
                .await,
            verbose
        ));

        // UpdateIdentityPool
        results.push(chk!(
            "UpdateIdentityPool",
            client
                .update_identity_pool()
                .identity_pool_id(pid)
                .identity_pool_name("conformance-identity-pool-updated")
                .allow_unauthenticated_identities(false)
                .send()
                .await,
            verbose
        ));

        // GetId
        let get_id_r = client
            .get_id()
            .account_id("000000000000")
            .identity_pool_id(pid)
            .send()
            .await;
        let identity_id = get_id_r
            .as_ref()
            .ok()
            .and_then(|r| r.identity_id.clone());
        results.push(chk!("GetId", get_id_r, verbose));

        // GetCredentialsForIdentity
        if let Some(ref iid) = identity_id {
            results.push(chk!(
                "GetCredentialsForIdentity",
                client
                    .get_credentials_for_identity()
                    .identity_id(iid)
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("GetCredentialsForIdentity".to_string()));
        }

        // SetIdentityPoolRoles
        results.push(chk!(
            "SetIdentityPoolRoles",
            client
                .set_identity_pool_roles()
                .identity_pool_id(pid)
                .roles(
                    "authenticated",
                    "arn:aws:iam::000000000000:role/conformance-cognito-role",
                )
                .send()
                .await,
            verbose
        ));

        // GetIdentityPoolRoles
        results.push(chk!(
            "GetIdentityPoolRoles",
            client
                .get_identity_pool_roles()
                .identity_pool_id(pid)
                .send()
                .await,
            verbose
        ));

        // ListIdentities
        results.push(chk!(
            "ListIdentities",
            client
                .list_identities()
                .identity_pool_id(pid)
                .max_results(10)
                .send()
                .await,
            verbose
        ));

        // SetPrincipalTagAttributeMap
        results.push(chk!(
            "SetPrincipalTagAttributeMap",
            client
                .set_principal_tag_attribute_map()
                .identity_pool_id(pid)
                .identity_provider_name("graph.facebook.com")
                .use_defaults(true)
                .send()
                .await,
            verbose
        ));

        // GetPrincipalTagAttributeMap
        results.push(chk!(
            "GetPrincipalTagAttributeMap",
            client
                .get_principal_tag_attribute_map()
                .identity_pool_id(pid)
                .identity_provider_name("graph.facebook.com")
                .send()
                .await,
            verbose
        ));

        // DeleteIdentityPool
        results.push(chk!(
            "DeleteIdentityPool",
            client
                .delete_identity_pool()
                .identity_pool_id(pid)
                .send()
                .await,
            verbose
        ));
    } else {
        for op in &[
            "DescribeIdentityPool",
            "UpdateIdentityPool",
            "GetId",
            "GetCredentialsForIdentity",
            "SetIdentityPoolRoles",
            "GetIdentityPoolRoles",
            "DeleteIdentityPool",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    results
}
