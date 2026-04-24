use crate::chk;
use crate::runner::common::*;

pub async fn test_secretsmanager(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_secretsmanager::Client::new(&config);
    let mut results = Vec::new();

    // CreateSecret
    let create_r = client
        .create_secret()
        .name("conformance/secret")
        .secret_string(r#"{"password":"hunter2"}"#)
        .send()
        .await;
    let secret_id = create_r
        .as_ref()
        .ok()
        .and_then(|r| r.arn.clone());
    results.push(chk!("CreateSecret", create_r, verbose));

    // ListSecrets
    results.push(chk!(
        "ListSecrets",
        client.list_secrets().send().await,
        verbose
    ));

    if let Some(ref arn) = secret_id {
        // GetSecretValue
        results.push(chk!(
            "GetSecretValue",
            client.get_secret_value().secret_id(arn).send().await,
            verbose
        ));

        // DescribeSecret
        results.push(chk!(
            "DescribeSecret",
            client.describe_secret().secret_id(arn).send().await,
            verbose
        ));

        // PutSecretValue
        results.push(chk!(
            "PutSecretValue",
            client
                .put_secret_value()
                .secret_id(arn)
                .secret_string(r#"{"password":"updated"}"#)
                .send()
                .await,
            verbose
        ));

        // UpdateSecret
        results.push(chk!(
            "UpdateSecret",
            client
                .update_secret()
                .secret_id(arn)
                .description("updated description")
                .send()
                .await,
            verbose
        ));

        // TagResource (Secrets Manager)
        results.push(chk!(
            "TagResource",
            client
                .tag_resource()
                .secret_id(arn)
                .tags(
                    aws_sdk_secretsmanager::types::Tag::builder()
                        .key("env")
                        .value("conformance")
                        .build(),
                )
                .send()
                .await,
            verbose
        ));

        // UntagResource (Secrets Manager)
        results.push(chk!(
            "UntagResource",
            client
                .untag_resource()
                .secret_id(arn)
                .tag_keys("env")
                .send()
                .await,
            verbose
        ));

        // DeleteSecret (soft delete first, then restore)
        results.push(chk!(
            "DeleteSecret",
            client
                .delete_secret()
                .secret_id(arn)
                .recovery_window_in_days(7)
                .send()
                .await,
            verbose
        ));

        // RestoreSecret (restore the soft-deleted secret)
        results.push(chk!(
            "RestoreSecret",
            client.restore_secret().secret_id(arn).send().await,
            verbose
        ));

        // RotateSecret
        results.push(chk!(
            "RotateSecret",
            client
                .rotate_secret()
                .secret_id(arn)
                .rotation_rules(
                    aws_sdk_secretsmanager::types::RotationRulesType::builder()
                        .automatically_after_days(30)
                        .build(),
                )
                .send()
                .await,
            verbose
        ));

        // ValidateResourcePolicy
        let policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"AWS":"arn:aws:iam::000000000000:root"},"Action":"secretsmanager:GetSecretValue","Resource":"*"}]}"#;
        results.push(chk!(
            "ValidateResourcePolicy",
            client
                .validate_resource_policy()
                .secret_id(arn)
                .resource_policy(policy)
                .send()
                .await,
            verbose
        ));

        // ListSecretVersionIds
        results.push(chk!(
            "ListSecretVersionIds",
            client.list_secret_version_ids().secret_id(arn).send().await,
            verbose
        ));

        // BatchGetSecretValue
        results.push(chk!(
            "BatchGetSecretValue",
            client
                .batch_get_secret_value()
                .secret_id_list(arn)
                .send()
                .await,
            verbose
        ));

        // PutResourcePolicy
        let res_policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"AWS":"arn:aws:iam::000000000000:root"},"Action":"secretsmanager:GetSecretValue","Resource":"*"}]}"#;
        results.push(chk!(
            "PutResourcePolicy",
            client
                .put_resource_policy()
                .secret_id(arn)
                .resource_policy(res_policy)
                .send()
                .await,
            verbose
        ));

        // GetResourcePolicy
        results.push(chk!(
            "GetResourcePolicy",
            client.get_resource_policy().secret_id(arn).send().await,
            verbose
        ));

        // DeleteResourcePolicy
        results.push(chk!(
            "DeleteResourcePolicy",
            client.delete_resource_policy().secret_id(arn).send().await,
            verbose
        ));

        // Final hard delete for cleanup
        let _ = client
            .delete_secret()
            .secret_id(arn)
            .force_delete_without_recovery(true)
            .send()
            .await;
    } else {
        for op in &[
            "GetSecretValue",
            "DescribeSecret",
            "PutSecretValue",
            "UpdateSecret",
            "TagResource",
            "UntagResource",
            "DeleteSecret",
            "RestoreSecret",
            "RotateSecret",
            "ValidateResourcePolicy",
            "ListSecretVersionIds",
            "BatchGetSecretValue",
            "PutResourcePolicy",
            "GetResourcePolicy",
            "DeleteResourcePolicy",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    // GetRandomPassword (no secret needed)
    results.push(chk!(
        "GetRandomPassword",
        client.get_random_password().send().await,
        verbose
    ));

    results
}
