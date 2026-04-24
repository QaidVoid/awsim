use crate::chk;
use crate::runner::common::*;

pub async fn test_sso_admin(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_ssoadmin::Client::new(&config);
    let mut results = Vec::new();

    results.push(chk!(
        "ListInstances",
        client.list_instances().send().await,
        verbose
    ));
    results.push(chk!(
        "CreatePermissionSet",
        client
            .create_permission_set()
            .instance_arn("arn:aws:sso:::instance/ssoins-0000000000000000")
            .name("conf-permset")
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "ListPermissionSets",
        client
            .list_permission_sets()
            .instance_arn("arn:aws:sso:::instance/ssoins-0000000000000000")
            .send()
            .await,
        verbose
    ));

    results
}
