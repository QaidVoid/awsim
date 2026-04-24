use crate::chk;
use crate::runner::common::*;

pub async fn test_cognito_idp(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_cognitoidentityprovider::Client::new(&config);
    let mut results = Vec::new();

    // CreateUserPool
    let create_r = client
        .create_user_pool()
        .pool_name("conformance-pool")
        .send()
        .await;
    let pool_id = create_r
        .as_ref()
        .ok()
        .and_then(|r| r.user_pool.as_ref())
        .and_then(|p| p.id.clone());
    results.push(chk!("CreateUserPool", create_r, verbose));

    // ListUserPools
    results.push(chk!(
        "ListUserPools",
        client.list_user_pools().max_results(10).send().await,
        verbose
    ));

    if let Some(ref pool_id) = pool_id {
        // DescribeUserPool
        results.push(chk!(
            "DescribeUserPool",
            client
                .describe_user_pool()
                .user_pool_id(pool_id)
                .send()
                .await,
            verbose
        ));

        // CreateUserPoolClient
        let client_r = client
            .create_user_pool_client()
            .user_pool_id(pool_id)
            .client_name("conformance-client")
            .send()
            .await;
        let app_client_id = client_r
            .as_ref()
            .ok()
            .and_then(|r| r.user_pool_client.as_ref())
            .and_then(|c| c.client_id.clone());
        results.push(chk!("CreateUserPoolClient", client_r, verbose));

        // ListUserPoolClients
        results.push(chk!(
            "ListUserPoolClients",
            client
                .list_user_pool_clients()
                .user_pool_id(pool_id)
                .send()
                .await,
            verbose
        ));

        // AdminCreateUser
        results.push(chk!(
            "AdminCreateUser",
            client
                .admin_create_user()
                .user_pool_id(pool_id)
                .username("conformance-user")
                .send()
                .await,
            verbose
        ));

        // ListUsers
        results.push(chk!(
            "ListUsers",
            client.list_users().user_pool_id(pool_id).send().await,
            verbose
        ));

        // AdminGetUser
        results.push(chk!(
            "AdminGetUser",
            client
                .admin_get_user()
                .user_pool_id(pool_id)
                .username("conformance-user")
                .send()
                .await,
            verbose
        ));

        // DescribeUserPoolClient
        if let Some(ref cid) = app_client_id {
            results.push(chk!(
                "DescribeUserPoolClient",
                client
                    .describe_user_pool_client()
                    .user_pool_id(pool_id)
                    .client_id(cid)
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("DescribeUserPoolClient".to_string()));
        }

        // CreateGroup (Cognito IDP)
        results.push(chk!(
            "CreateGroup",
            client
                .create_group()
                .user_pool_id(pool_id)
                .group_name("conformance-group")
                .send()
                .await,
            verbose
        ));

        // ListGroups (Cognito IDP)
        results.push(chk!(
            "ListGroups",
            client.list_groups().user_pool_id(pool_id).send().await,
            verbose
        ));

        // AdminAddUserToGroup
        results.push(chk!(
            "AdminAddUserToGroup",
            client
                .admin_add_user_to_group()
                .user_pool_id(pool_id)
                .username("conformance-user")
                .group_name("conformance-group")
                .send()
                .await,
            verbose
        ));

        // AdminListGroupsForUser
        results.push(chk!(
            "AdminListGroupsForUser",
            client
                .admin_list_groups_for_user()
                .user_pool_id(pool_id)
                .username("conformance-user")
                .send()
                .await,
            verbose
        ));

        // AdminDeleteUser (cleanup)
        results.push(chk!(
            "AdminDeleteUser",
            client
                .admin_delete_user()
                .user_pool_id(pool_id)
                .username("conformance-user")
                .send()
                .await,
            verbose
        ));

        // SignUp (needs client credentials)
        if let Some(ref cid) = app_client_id {
            results.push(chk!(
                "SignUp",
                client
                    .sign_up()
                    .client_id(cid)
                    .username("signup-user")
                    .password("Pass@word1!")
                    .send()
                    .await,
                verbose
            ));

            // ConfirmSignUp (auto-confirm in sim — may pass or need admin confirm)
            results.push(chk!(
                "ConfirmSignUp",
                client
                    .confirm_sign_up()
                    .client_id(cid)
                    .username("signup-user")
                    .confirmation_code("123456")
                    .send()
                    .await,
                verbose
            ));

            // InitiateAuth (USER_PASSWORD_AUTH)
            results.push(chk!(
                "InitiateAuth",
                client
                    .initiate_auth()
                    .client_id(cid)
                    .auth_flow(
                        aws_sdk_cognitoidentityprovider::types::AuthFlowType::UserPasswordAuth
                    )
                    .auth_parameters("USERNAME", "signup-user")
                    .auth_parameters("PASSWORD", "Pass@word1!")
                    .send()
                    .await,
                verbose
            ));

            // ForgotPassword
            results.push(chk!(
                "ForgotPassword",
                client
                    .forgot_password()
                    .client_id(cid)
                    .username("signup-user")
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("SignUp".to_string()));
            results.push(OpResult::Skipped("ConfirmSignUp".to_string()));
            results.push(OpResult::Skipped("InitiateAuth".to_string()));
            results.push(OpResult::Skipped("ForgotPassword".to_string()));
        }

        // UpdateUserPool
        results.push(chk!(
            "UpdateUserPool",
            client.update_user_pool().user_pool_id(pool_id).send().await,
            verbose
        ));

        // AdminEnableUser / AdminDisableUser
        // Re-create the user for enable/disable tests (was deleted above)
        let _ = client
            .admin_create_user()
            .user_pool_id(pool_id)
            .username("enable-test-user")
            .send()
            .await;

        results.push(chk!(
            "AdminDisableUser",
            client
                .admin_disable_user()
                .user_pool_id(pool_id)
                .username("enable-test-user")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "AdminEnableUser",
            client
                .admin_enable_user()
                .user_pool_id(pool_id)
                .username("enable-test-user")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "AdminResetUserPassword",
            client
                .admin_reset_user_password()
                .user_pool_id(pool_id)
                .username("enable-test-user")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "AdminSetUserMFAPreference",
            client
                .admin_set_user_mfa_preference()
                .user_pool_id(pool_id)
                .username("enable-test-user")
                .send()
                .await,
            verbose
        ));

        // Cleanup enable-test-user
        let _ = client
            .admin_delete_user()
            .user_pool_id(pool_id)
            .username("enable-test-user")
            .send()
            .await;

        // SetUserPoolMfaConfig / GetUserPoolMfaConfig
        results.push(chk!(
            "SetUserPoolMfaConfig",
            client
                .set_user_pool_mfa_config()
                .user_pool_id(pool_id)
                .mfa_configuration(
                    aws_sdk_cognitoidentityprovider::types::UserPoolMfaType::Optional,
                )
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "GetUserPoolMfaConfig",
            client
                .get_user_pool_mfa_config()
                .user_pool_id(pool_id)
                .send()
                .await,
            verbose
        ));

        // Group management
        results.push(chk!(
            "GetGroup",
            client
                .get_group()
                .user_pool_id(pool_id)
                .group_name("conformance-group")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "UpdateGroup",
            client
                .update_group()
                .user_pool_id(pool_id)
                .group_name("conformance-group")
                .description("updated description")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ListUsersInGroup",
            client
                .list_users_in_group()
                .user_pool_id(pool_id)
                .group_name("conformance-group")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "AdminRemoveUserFromGroup",
            client
                .admin_remove_user_from_group()
                .user_pool_id(pool_id)
                .username("signup-user")
                .group_name("conformance-group")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "DeleteGroup",
            client
                .delete_group()
                .user_pool_id(pool_id)
                .group_name("conformance-group")
                .send()
                .await,
            verbose
        ));

        // Identity Providers
        results.push(chk!(
            "CreateIdentityProvider",
            client
                .create_identity_provider()
                .user_pool_id(pool_id)
                .provider_name("conformance-oidc")
                .provider_type(
                    aws_sdk_cognitoidentityprovider::types::IdentityProviderTypeType::Oidc,
                )
                .provider_details("client_id", "test-client")
                .provider_details("client_secret", "test-secret")
                .provider_details("attributes_request_method", "GET")
                .provider_details("oidc_issuer", "https://accounts.example.com",)
                .provider_details("authorize_scopes", "openid")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ListIdentityProviders",
            client
                .list_identity_providers()
                .user_pool_id(pool_id)
                .send()
                .await,
            verbose
        ));

        if let Some(ref cid) = app_client_id {
            results.push(chk!(
                "AddUserPoolClientSecret",
                client
                    .add_user_pool_client_secret()
                    .user_pool_id(pool_id)
                    .client_id(cid)
                    .send()
                    .await,
                verbose
            ));

            results.push(chk!(
                "ListUserPoolClientSecrets",
                client
                    .list_user_pool_client_secrets()
                    .user_pool_id(pool_id)
                    .client_id(cid)
                    .send()
                    .await,
                verbose
            ));
        }

        results.push(chk!(
            "ListTerms",
            client.list_terms().user_pool_id(pool_id).send().await,
            verbose
        ));

        results.push(chk!(
            "DeleteIdentityProvider",
            client
                .delete_identity_provider()
                .user_pool_id(pool_id)
                .provider_name("conformance-oidc")
                .send()
                .await,
            verbose
        ));

        // Resource Servers
        results.push(chk!(
            "CreateResourceServer",
            client
                .create_resource_server()
                .user_pool_id(pool_id)
                .identifier("https://api.conformance.test")
                .name("conformance-resource-server")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ListResourceServers",
            client
                .list_resource_servers()
                .user_pool_id(pool_id)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "DeleteResourceServer",
            client
                .delete_resource_server()
                .user_pool_id(pool_id)
                .identifier("https://api.conformance.test")
                .send()
                .await,
            verbose
        ));

        // UpdateUserPoolClient
        if let Some(ref cid) = app_client_id {
            results.push(chk!(
                "UpdateUserPoolClient",
                client
                    .update_user_pool_client()
                    .user_pool_id(pool_id)
                    .client_id(cid)
                    .client_name("conformance-client-updated")
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("UpdateUserPoolClient".to_string()));
        }

        // Tags
        let pool_arn = format!(
            "arn:aws:cognito-idp:us-east-1:000000000000:userpool/{}",
            pool_id
        );
        results.push(chk!(
            "TagResource",
            client
                .tag_resource()
                .resource_arn(&pool_arn)
                .tags("env", "conformance")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ListTagsForResource",
            client
                .list_tags_for_resource()
                .resource_arn(&pool_arn)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "UntagResource",
            client
                .untag_resource()
                .resource_arn(&pool_arn)
                .tag_keys("env")
                .send()
                .await,
            verbose
        ));

        // DescribeUserPoolDomain (for a domain that doesn't exist — should return empty)
        results.push(chk!(
            "DescribeUserPoolDomain",
            client
                .describe_user_pool_domain()
                .domain("nonexistent-conformance-domain")
                .send()
                .await,
            verbose
        ));

        // DeleteUserPoolClient
        if let Some(ref cid) = app_client_id {
            results.push(chk!(
                "DeleteUserPoolClient",
                client
                    .delete_user_pool_client()
                    .user_pool_id(pool_id)
                    .client_id(cid)
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("DeleteUserPoolClient".to_string()));
        }

        // DeleteUserPool
        results.push(chk!(
            "DeleteUserPool",
            client.delete_user_pool().user_pool_id(pool_id).send().await,
            verbose
        ));
    } else {
        for op in &[
            "DescribeUserPool",
            "CreateUserPoolClient",
            "ListUserPoolClients",
            "DescribeUserPoolClient",
            "AdminCreateUser",
            "ListUsers",
            "AdminGetUser",
            "CreateGroup",
            "ListGroups",
            "AdminAddUserToGroup",
            "AdminListGroupsForUser",
            "AdminDeleteUser",
            "SignUp",
            "ConfirmSignUp",
            "InitiateAuth",
            "ForgotPassword",
            "UpdateUserPool",
            "AdminDisableUser",
            "AdminEnableUser",
            "AdminResetUserPassword",
            "AdminSetUserMFAPreference",
            "SetUserPoolMfaConfig",
            "GetUserPoolMfaConfig",
            "GetGroup",
            "UpdateGroup",
            "ListUsersInGroup",
            "AdminRemoveUserFromGroup",
            "DeleteGroup",
            "CreateIdentityProvider",
            "ListIdentityProviders",
            "DeleteIdentityProvider",
            "CreateResourceServer",
            "ListResourceServers",
            "DeleteResourceServer",
            "UpdateUserPoolClient",
            "TagResource",
            "ListTagsForResource",
            "UntagResource",
            "DescribeUserPoolDomain",
            "DeleteUserPoolClient",
            "DeleteUserPool",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    results
}
