use crate::chk;
use crate::runner::common::*;

pub async fn test_organizations(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_organizations::Client::new(&config);
    let mut results = Vec::new();

    results.push(chk!(
        "CreateOrganization",
        client.create_organization().feature_set(aws_sdk_organizations::types::OrganizationFeatureSet::All).send().await,
        verbose
    ));
    results.push(chk!(
        "DescribeOrganization",
        client.describe_organization().send().await,
        verbose
    ));
    results.push(chk!(
        "CreateAccount",
        client.create_account().email("conf@example.com").account_name("conf-acct").send().await,
        verbose
    ));
    results.push(chk!(
        "ListAccounts",
        client.list_accounts().send().await,
        verbose
    ));
    results.push(chk!(
        "ListRoots",
        client.list_roots().send().await,
        verbose
    ));
    results.push(chk!(
        "CreateOrganizationalUnit",
        client.create_organizational_unit().parent_id("r-0000").name("conf-ou").send().await,
        verbose
    ));
    results.push(chk!(
        "ListOrganizationalUnitsForParent",
        client.list_organizational_units_for_parent().parent_id("r-0000").send().await,
        verbose
    ));
    results.push(chk!(
        "CreatePolicy",
        client
            .create_policy()
            .name("conf-policy")
            .description("conformance")
            .content("{\"Version\":\"2012-10-17\",\"Statement\":[]}")
            .r#type(aws_sdk_organizations::types::PolicyType::ServiceControlPolicy)
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "ListPolicies",
        client.list_policies().filter(aws_sdk_organizations::types::PolicyType::ServiceControlPolicy).send().await,
        verbose
    ));
    results.push(chk!(
        "ListChildren",
        client
            .list_children()
            .parent_id("r-0000")
            .child_type(aws_sdk_organizations::types::ChildType::OrganizationalUnit)
            .send()
            .await,
        verbose
    ));

    results
}
