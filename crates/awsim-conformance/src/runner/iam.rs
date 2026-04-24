use crate::chk;
use crate::runner::common::*;

pub async fn test_iam(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_iam::Client::new(&config);
    let mut results = Vec::new();

    // CreateUser
    results.push(chk!(
        "CreateUser",
        client
            .create_user()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // GetUser
    results.push(chk!(
        "GetUser",
        client.get_user().user_name("conformance-user").send().await,
        verbose
    ));

    // ListUsers
    results.push(chk!("ListUsers", client.list_users().send().await, verbose));

    // CreateAccessKey
    results.push(chk!(
        "CreateAccessKey",
        client
            .create_access_key()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // ListAccessKeys
    results.push(chk!(
        "ListAccessKeys",
        client
            .list_access_keys()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // CreateGroup
    results.push(chk!(
        "CreateGroup",
        client
            .create_group()
            .group_name("conformance-group")
            .send()
            .await,
        verbose
    ));

    // ListGroups
    results.push(chk!(
        "ListGroups",
        client.list_groups().send().await,
        verbose
    ));

    // AddUserToGroup
    results.push(chk!(
        "AddUserToGroup",
        client
            .add_user_to_group()
            .group_name("conformance-group")
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // GetGroup
    results.push(chk!(
        "GetGroup",
        client
            .get_group()
            .group_name("conformance-group")
            .send()
            .await,
        verbose
    ));

    // CreateRole
    let trust_policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"lambda.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#;
    results.push(chk!(
        "CreateRole",
        client
            .create_role()
            .role_name("conformance-role")
            .assume_role_policy_document(trust_policy)
            .send()
            .await,
        verbose
    ));

    // GetRole
    results.push(chk!(
        "GetRole",
        client.get_role().role_name("conformance-role").send().await,
        verbose
    ));

    // ListRoles
    results.push(chk!("ListRoles", client.list_roles().send().await, verbose));

    // CreatePolicy
    let policy_doc = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:*","Resource":"*"}]}"#;
    let create_policy_r = client
        .create_policy()
        .policy_name("conformance-policy")
        .policy_document(policy_doc)
        .send()
        .await;
    let policy_arn = create_policy_r
        .as_ref()
        .ok()
        .and_then(|r| r.policy.as_ref())
        .and_then(|p| p.arn.clone());
    results.push(chk!("CreatePolicy", create_policy_r, verbose));

    // ListPolicies
    results.push(chk!(
        "ListPolicies",
        client.list_policies().send().await,
        verbose
    ));

    // AttachRolePolicy
    if let Some(ref arn) = policy_arn {
        results.push(chk!(
            "AttachRolePolicy",
            client
                .attach_role_policy()
                .role_name("conformance-role")
                .policy_arn(arn)
                .send()
                .await,
            verbose
        ));

        // DetachRolePolicy
        results.push(chk!(
            "DetachRolePolicy",
            client
                .detach_role_policy()
                .role_name("conformance-role")
                .policy_arn(arn)
                .send()
                .await,
            verbose
        ));

        // DeletePolicy
        results.push(chk!(
            "DeletePolicy",
            client.delete_policy().policy_arn(arn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("AttachRolePolicy".to_string()));
        results.push(OpResult::Skipped("DetachRolePolicy".to_string()));
        results.push(OpResult::Skipped("DeletePolicy".to_string()));
    }

    // RemoveUserFromGroup (cleanup)
    results.push(chk!(
        "RemoveUserFromGroup",
        client
            .remove_user_from_group()
            .group_name("conformance-group")
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // DeleteGroup
    results.push(chk!(
        "DeleteGroup",
        client
            .delete_group()
            .group_name("conformance-group")
            .send()
            .await,
        verbose
    ));

    // DeleteRole
    results.push(chk!(
        "DeleteRole",
        client
            .delete_role()
            .role_name("conformance-role")
            .send()
            .await,
        verbose
    ));

    // CreateUser again for supplemental tests
    let _ = client
        .create_user()
        .user_name("conformance-user2")
        .send()
        .await;

    // CreateAccessKey (for conformance-user2)
    let ak_r = client
        .create_access_key()
        .user_name("conformance-user2")
        .send()
        .await;
    let access_key_id = ak_r
        .as_ref()
        .ok()
        .and_then(|r| r.access_key.as_ref())
        .map(|ak| ak.access_key_id.clone());

    // DeleteAccessKey
    if let Some(ref akid) = access_key_id {
        results.push(chk!(
            "DeleteAccessKey",
            client
                .delete_access_key()
                .user_name("conformance-user2")
                .access_key_id(akid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteAccessKey".to_string()));
    }

    // AttachUserPolicy / DetachUserPolicy
    let policy_doc2 = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"sqs:*","Resource":"*"}]}"#;
    let up_r = client
        .create_policy()
        .policy_name("conformance-user-policy")
        .policy_document(policy_doc2)
        .send()
        .await;
    let user_policy_arn = up_r
        .as_ref()
        .ok()
        .and_then(|r| r.policy.as_ref())
        .and_then(|p| p.arn.clone());

    if let Some(ref uarn) = user_policy_arn {
        results.push(chk!(
            "AttachUserPolicy",
            client
                .attach_user_policy()
                .user_name("conformance-user2")
                .policy_arn(uarn)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ListAttachedUserPolicies",
            client
                .list_attached_user_policies()
                .user_name("conformance-user2")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "DetachUserPolicy",
            client
                .detach_user_policy()
                .user_name("conformance-user2")
                .policy_arn(uarn)
                .send()
                .await,
            verbose
        ));

        // CreatePolicyVersion
        results.push(chk!(
            "CreatePolicyVersion",
            client
                .create_policy_version()
                .policy_arn(uarn)
                .policy_document(policy_doc2)
                .send()
                .await,
            verbose
        ));

        // ListPolicyVersions
        results.push(chk!(
            "ListPolicyVersions",
            client.list_policy_versions().policy_arn(uarn).send().await,
            verbose
        ));

        // GetPolicyVersion
        results.push(chk!(
            "GetPolicyVersion",
            client
                .get_policy_version()
                .policy_arn(uarn)
                .version_id("v1")
                .send()
                .await,
            verbose
        ));

        let _ = client.delete_policy().policy_arn(uarn).send().await;
    } else {
        for op in &[
            "AttachUserPolicy",
            "ListAttachedUserPolicies",
            "DetachUserPolicy",
            "CreatePolicyVersion",
            "ListPolicyVersions",
            "GetPolicyVersion",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    // PutUserPolicy / GetUserPolicy / ListUserPolicies / DeleteUserPolicy
    results.push(chk!(
        "PutUserPolicy",
        client
            .put_user_policy()
            .user_name("conformance-user2")
            .policy_name("inline-policy")
            .policy_document(
                r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"*"}]}"#,
            )
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "GetUserPolicy",
        client
            .get_user_policy()
            .user_name("conformance-user2")
            .policy_name("inline-policy")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "ListUserPolicies",
        client
            .list_user_policies()
            .user_name("conformance-user2")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "DeleteUserPolicy",
        client
            .delete_user_policy()
            .user_name("conformance-user2")
            .policy_name("inline-policy")
            .send()
            .await,
        verbose
    ));

    // ListAttachedRolePolicies (use conformance-role which may not exist anymore; it was deleted above)
    // Create a temporary role for this
    let tr_doc = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"ec2.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#;
    let _ = client
        .create_role()
        .role_name("conformance-role2")
        .assume_role_policy_document(tr_doc)
        .send()
        .await;

    results.push(chk!(
        "ListAttachedRolePolicies",
        client
            .list_attached_role_policies()
            .role_name("conformance-role2")
            .send()
            .await,
        verbose
    ));

    let _ = client
        .delete_role()
        .role_name("conformance-role2")
        .send()
        .await;

    // CreateInstanceProfile / GetInstanceProfile / DeleteInstanceProfile
    results.push(chk!(
        "CreateInstanceProfile",
        client
            .create_instance_profile()
            .instance_profile_name("conformance-profile")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "GetInstanceProfile",
        client
            .get_instance_profile()
            .instance_profile_name("conformance-profile")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "DeleteInstanceProfile",
        client
            .delete_instance_profile()
            .instance_profile_name("conformance-profile")
            .send()
            .await,
        verbose
    ));

    // TagUser / ListUserTags / UntagUser
    results.push(chk!(
        "TagUser",
        client
            .tag_user()
            .user_name("conformance-user2")
            .tags(
                aws_sdk_iam::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "ListUserTags",
        client
            .list_user_tags()
            .user_name("conformance-user2")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "UntagUser",
        client
            .untag_user()
            .user_name("conformance-user2")
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // GetAccountSummary
    results.push(chk!(
        "GetAccountSummary",
        client.get_account_summary().send().await,
        verbose
    ));

    // ListAccountAliases
    results.push(chk!(
        "ListAccountAliases",
        client.list_account_aliases().send().await,
        verbose
    ));

    // ListInstanceProfiles
    results.push(chk!(
        "ListInstanceProfiles",
        client.list_instance_profiles().send().await,
        verbose
    ));

    // CreateInstanceProfile + ListInstanceProfilesForRole
    let _ = client
        .create_instance_profile()
        .instance_profile_name("conformance-profile2")
        .send()
        .await;

    // Create a role to associate
    let tr_doc2 = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"ec2.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#;
    let _ = client
        .create_role()
        .role_name("conformance-role3")
        .assume_role_policy_document(tr_doc2)
        .send()
        .await;

    let _ = client
        .add_role_to_instance_profile()
        .instance_profile_name("conformance-profile2")
        .role_name("conformance-role3")
        .send()
        .await;

    results.push(chk!(
        "ListInstanceProfilesForRole",
        client
            .list_instance_profiles_for_role()
            .role_name("conformance-role3")
            .send()
            .await,
        verbose
    ));

    // Cleanup
    let _ = client
        .remove_role_from_instance_profile()
        .instance_profile_name("conformance-profile2")
        .role_name("conformance-role3")
        .send()
        .await;
    let _ = client
        .delete_instance_profile()
        .instance_profile_name("conformance-profile2")
        .send()
        .await;
    let _ = client
        .delete_role()
        .role_name("conformance-role3")
        .send()
        .await;

    // CreateLoginProfile / GetLoginProfile / UpdateLoginProfile / DeleteLoginProfile
    // Use conformance-user which still exists at this point.
    results.push(chk!(
        "CreateLoginProfile",
        client
            .create_login_profile()
            .user_name("conformance-user")
            .password("Pass@word1!")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "GetLoginProfile",
        client
            .get_login_profile()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "UpdateLoginProfile",
        client
            .update_login_profile()
            .user_name("conformance-user")
            .password("NewPass@word2!")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "DeleteLoginProfile",
        client
            .delete_login_profile()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // ListSigningCertificates
    results.push(chk!(
        "ListSigningCertificates",
        client
            .list_signing_certificates()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // GetAccountPasswordPolicy
    results.push(chk!(
        "GetAccountPasswordPolicy",
        client.get_account_password_policy().send().await,
        verbose
    ));

    // SimulateCustomPolicy
    results.push(chk!(
        "SimulateCustomPolicy",
        client
            .simulate_custom_policy()
            .policy_input_list(
                r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:*","Resource":"*"}]}"#,
            )
            .action_names("s3:GetObject")
            .resource_arns("*")
            .send()
            .await,
        verbose
    ));

    // SimulatePrincipalPolicy
    results.push(chk!(
        "SimulatePrincipalPolicy",
        client
            .simulate_principal_policy()
            .policy_source_arn("arn:aws:iam::000000000000:user/conformance-user".to_string())
            .action_names("s3:GetObject")
            .resource_arns("*")
            .send()
            .await,
        verbose
    ));

    // GetContextKeysForCustomPolicy
    results.push(chk!(
        "GetContextKeysForCustomPolicy",
        client
            .get_context_keys_for_custom_policy()
            .policy_input_list(
                r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:*","Resource":"*"}]}"#,
            )
            .send()
            .await,
        verbose
    ));

    // GetContextKeysForPrincipalPolicy
    results.push(chk!(
        "GetContextKeysForPrincipalPolicy",
        client
            .get_context_keys_for_principal_policy()
            .policy_source_arn("arn:aws:iam::000000000000:user/conformance-user".to_string())
            .send()
            .await,
        verbose
    ));

    // ListGroupsForUser
    results.push(chk!(
        "ListGroupsForUser",
        client
            .list_groups_for_user()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    // ChangePassword
    results.push(chk!(
        "ChangePassword",
        client
            .change_password()
            .old_password("OldPass@word1!")
            .new_password("NewPass@word2!")
            .send()
            .await,
        verbose
    ));

    // CreateVirtualMFADevice + GetMFADevice + DeleteVirtualMFADevice
    let mfa_r = client
        .create_virtual_mfa_device()
        .virtual_mfa_device_name("conformance-mfa")
        .send()
        .await;
    let mfa_serial = mfa_r
        .as_ref()
        .ok()
        .and_then(|r| r.virtual_mfa_device.as_ref())
        .map(|d| d.serial_number.clone());
    results.push(chk!("CreateVirtualMFADevice", mfa_r, verbose));

    if let Some(serial) = mfa_serial.as_ref() {
        results.push(chk!(
            "GetMFADevice",
            client.get_mfa_device().serial_number(serial).send().await,
            verbose
        ));
        let _ = client
            .delete_virtual_mfa_device()
            .serial_number(serial)
            .send()
            .await;
    } else {
        results.push(OpResult::Skipped("GetMFADevice".to_string()));
    }

    // CreateServiceSpecificCredential / List / Delete
    let ssc_r = client
        .create_service_specific_credential()
        .user_name("conformance-user")
        .service_name("codecommit.amazonaws.com")
        .send()
        .await;
    let ssc_id = ssc_r
        .as_ref()
        .ok()
        .and_then(|r| r.service_specific_credential.as_ref())
        .map(|c| c.service_specific_credential_id.clone());
    results.push(chk!("CreateServiceSpecificCredential", ssc_r, verbose));

    results.push(chk!(
        "ListServiceSpecificCredentials",
        client
            .list_service_specific_credentials()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    if let Some(id) = ssc_id {
        let _ = client
            .delete_service_specific_credential()
            .service_specific_credential_id(id)
            .send()
            .await;
    }

    // UploadServerCertificate + UpdateServerCertificate + DeleteServerCertificate
    let cert_body = "-----BEGIN CERTIFICATE-----\nMIIDummy\n-----END CERTIFICATE-----";
    let private_key = "-----BEGIN PRIVATE KEY-----\nDUMMY\n-----END PRIVATE KEY-----";
    results.push(chk!(
        "UploadServerCertificate",
        client
            .upload_server_certificate()
            .server_certificate_name("conformance-cert")
            .certificate_body(cert_body)
            .private_key(private_key)
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "UpdateServerCertificate",
        client
            .update_server_certificate()
            .server_certificate_name("conformance-cert")
            .new_server_certificate_name("conformance-cert-renamed")
            .send()
            .await,
        verbose
    ));

    let _ = client
        .delete_server_certificate()
        .server_certificate_name("conformance-cert-renamed")
        .send()
        .await;

    // PutUserPermissionsBoundary / DeleteUserPermissionsBoundary
    let boundary_doc =
        r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"*","Resource":"*"}]}"#;
    let boundary_r = client
        .create_policy()
        .policy_name("conformance-boundary")
        .policy_document(boundary_doc)
        .send()
        .await;
    let boundary_arn = boundary_r
        .as_ref()
        .ok()
        .and_then(|r| r.policy.as_ref())
        .and_then(|p| p.arn.clone());

    if let Some(ref barn) = boundary_arn {
        results.push(chk!(
            "PutUserPermissionsBoundary",
            client
                .put_user_permissions_boundary()
                .user_name("conformance-user2")
                .permissions_boundary(barn)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "DeleteUserPermissionsBoundary",
            client
                .delete_user_permissions_boundary()
                .user_name("conformance-user2")
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("PutUserPermissionsBoundary".to_string()));
        results.push(OpResult::Skipped(
            "DeleteUserPermissionsBoundary".to_string(),
        ));
    }

    // PutRolePermissionsBoundary / DeleteRolePermissionsBoundary
    let role_doc = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"ec2.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#;
    let _ = client
        .create_role()
        .role_name("conformance-boundary-role")
        .assume_role_policy_document(role_doc)
        .send()
        .await;

    if let Some(ref barn) = boundary_arn {
        results.push(chk!(
            "PutRolePermissionsBoundary",
            client
                .put_role_permissions_boundary()
                .role_name("conformance-boundary-role")
                .permissions_boundary(barn)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "DeleteRolePermissionsBoundary",
            client
                .delete_role_permissions_boundary()
                .role_name("conformance-boundary-role")
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("PutRolePermissionsBoundary".to_string()));
        results.push(OpResult::Skipped(
            "DeleteRolePermissionsBoundary".to_string(),
        ));
    }

    let _ = client
        .delete_role()
        .role_name("conformance-boundary-role")
        .send()
        .await;
    if let Some(ref barn) = boundary_arn {
        let _ = client.delete_policy().policy_arn(barn).send().await;
    }

    // GetAccessKeyLastUsed / UpdateAccessKey
    let ak2_r = client
        .create_access_key()
        .user_name("conformance-user2")
        .send()
        .await;
    let ak2_id = ak2_r
        .as_ref()
        .ok()
        .and_then(|r| r.access_key.as_ref())
        .map(|k| k.access_key_id.clone());

    if let Some(ref akid) = ak2_id {
        results.push(chk!(
            "GetAccessKeyLastUsed",
            client
                .get_access_key_last_used()
                .access_key_id(akid)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "UpdateAccessKey",
            client
                .update_access_key()
                .user_name("conformance-user2")
                .access_key_id(akid)
                .status(aws_sdk_iam::types::StatusType::Inactive)
                .send()
                .await,
            verbose
        ));

        let _ = client
            .delete_access_key()
            .user_name("conformance-user2")
            .access_key_id(akid)
            .send()
            .await;
    } else {
        results.push(OpResult::Skipped("GetAccessKeyLastUsed".to_string()));
        results.push(OpResult::Skipped("UpdateAccessKey".to_string()));
    }

    // UpdateGroup
    results.push(chk!(
        "UpdateGroup",
        client
            .update_group()
            .group_name("conformance-group")
            .new_path("/conformance/")
            .send()
            .await,
        verbose
    ));

    // UploadSigningCertificate / UpdateSigningCertificate
    let sign_cert_body = "-----BEGIN CERTIFICATE-----\nMIICONFORMANCE\n-----END CERTIFICATE-----";
    let usc_r = client
        .upload_signing_certificate()
        .user_name("conformance-user")
        .certificate_body(sign_cert_body)
        .send()
        .await;
    let signing_cert_id = usc_r
        .as_ref()
        .ok()
        .and_then(|r| r.certificate.as_ref())
        .map(|c| c.certificate_id.clone());
    results.push(chk!("UploadSigningCertificate", usc_r, verbose));

    if let Some(ref cid) = signing_cert_id {
        results.push(chk!(
            "UpdateSigningCertificate",
            client
                .update_signing_certificate()
                .user_name("conformance-user")
                .certificate_id(cid)
                .status(aws_sdk_iam::types::StatusType::Inactive)
                .send()
                .await,
            verbose
        ));

        let _ = client
            .delete_signing_certificate()
            .user_name("conformance-user")
            .certificate_id(cid)
            .send()
            .await;
    } else {
        results.push(OpResult::Skipped("UpdateSigningCertificate".to_string()));
    }

    // UpdateServiceSpecificCredential / ResetServiceSpecificCredential
    let ssc2_r = client
        .create_service_specific_credential()
        .user_name("conformance-user")
        .service_name("codecommit.amazonaws.com")
        .send()
        .await;
    let ssc2_id = ssc2_r
        .as_ref()
        .ok()
        .and_then(|r| r.service_specific_credential.as_ref())
        .map(|c| c.service_specific_credential_id.clone());

    if let Some(ref id) = ssc2_id {
        results.push(chk!(
            "UpdateServiceSpecificCredential",
            client
                .update_service_specific_credential()
                .user_name("conformance-user")
                .service_specific_credential_id(id)
                .status(aws_sdk_iam::types::StatusType::Inactive)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ResetServiceSpecificCredential",
            client
                .reset_service_specific_credential()
                .user_name("conformance-user")
                .service_specific_credential_id(id)
                .send()
                .await,
            verbose
        ));

        let _ = client
            .delete_service_specific_credential()
            .user_name("conformance-user")
            .service_specific_credential_id(id)
            .send()
            .await;
    } else {
        results.push(OpResult::Skipped(
            "UpdateServiceSpecificCredential".to_string(),
        ));
        results.push(OpResult::Skipped(
            "ResetServiceSpecificCredential".to_string(),
        ));
    }

    // GenerateCredentialReport / GetCredentialReport
    results.push(chk!(
        "GenerateCredentialReport",
        client.generate_credential_report().send().await,
        verbose
    ));

    results.push(chk!(
        "GetCredentialReport",
        client.get_credential_report().send().await,
        verbose
    ));

    // GetServiceLastAccessedDetails
    let glsad_r = client
        .generate_service_last_accessed_details()
        .arn("arn:aws:iam::000000000000:user/conformance-user".to_string())
        .send()
        .await;
    let job_id = glsad_r.as_ref().ok().and_then(|r| r.job_id.clone());

    if let Some(jid) = job_id {
        results.push(chk!(
            "GetServiceLastAccessedDetails",
            client
                .get_service_last_accessed_details()
                .job_id(jid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped(
            "GetServiceLastAccessedDetails".to_string(),
        ));
    }

    // ListSAMLProviders / ListVirtualMFADevices / ListServerCertificates / ListMFADevices / ListOpenIDConnectProviders
    results.push(chk!(
        "ListSAMLProviders",
        client.list_saml_providers().send().await,
        verbose
    ));

    results.push(chk!(
        "ListVirtualMFADevices",
        client.list_virtual_mfa_devices().send().await,
        verbose
    ));

    results.push(chk!(
        "ListServerCertificates",
        client.list_server_certificates().send().await,
        verbose
    ));

    results.push(chk!(
        "ListMFADevices",
        client
            .list_mfa_devices()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "ListOpenIDConnectProviders",
        client.list_open_id_connect_providers().send().await,
        verbose
    ));

    // ListPoliciesGrantingServiceAccess / GetAccountAuthorizationDetails
    results.push(chk!(
        "ListPoliciesGrantingServiceAccess",
        client
            .list_policies_granting_service_access()
            .arn("arn:aws:iam::000000000000:user/conformance-user".to_string())
            .service_namespaces("s3")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "GetAccountAuthorizationDetails",
        client.get_account_authorization_details().send().await,
        verbose
    ));

    // CreateAccountAlias / DeleteAccountAlias
    results.push(chk!(
        "CreateAccountAlias",
        client
            .create_account_alias()
            .account_alias("conformance-alias")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "DeleteAccountAlias",
        client
            .delete_account_alias()
            .account_alias("conformance-alias")
            .send()
            .await,
        verbose
    ));

    // UpdateRole / UpdateRoleDescription
    let _ = client
        .create_role()
        .role_name("conformance-update-role")
        .assume_role_policy_document(
            r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"ec2.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#,
        )
        .send()
        .await;

    results.push(chk!(
        "UpdateRole",
        client
            .update_role()
            .role_name("conformance-update-role")
            .description("updated description")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "UpdateRoleDescription",
        client
            .update_role_description()
            .role_name("conformance-update-role")
            .description("another description")
            .send()
            .await,
        verbose
    ));

    let _ = client
        .delete_role()
        .role_name("conformance-update-role")
        .send()
        .await;

    // DeleteUser (cleanup user2)
    let _ = client
        .delete_user()
        .user_name("conformance-user2")
        .send()
        .await;

    // DeleteUser
    results.push(chk!(
        "DeleteUser",
        client
            .delete_user()
            .user_name("conformance-user")
            .send()
            .await,
        verbose
    ));

    results
}
