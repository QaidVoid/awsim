use crate::chk;
use crate::runner::common::*;

pub async fn test_cloudformation(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_cloudformation::Client::new(&config);
    let mut results = Vec::new();

    let template = r#"{"AWSTemplateFormatVersion":"2010-09-09","Description":"Conformance test stack","Resources":{"ConformanceBucket":{"Type":"AWS::S3::Bucket","Properties":{"BucketName":"conformance-cfn-bucket"}}}}"#;

    // CreateStack
    results.push(chk!(
        "CreateStack",
        client
            .create_stack()
            .stack_name("conformance-stack")
            .template_body(template)
            .send()
            .await,
        verbose
    ));

    // DescribeStacks
    results.push(chk!(
        "DescribeStacks",
        client
            .describe_stacks()
            .stack_name("conformance-stack")
            .send()
            .await,
        verbose
    ));

    // DescribeStackResources
    results.push(chk!(
        "DescribeStackResources",
        client
            .describe_stack_resources()
            .stack_name("conformance-stack")
            .send()
            .await,
        verbose
    ));

    // ListStacks
    results.push(chk!(
        "ListStacks",
        client.list_stacks().send().await,
        verbose
    ));

    // GetTemplate
    results.push(chk!(
        "GetTemplate",
        client
            .get_template()
            .stack_name("conformance-stack")
            .send()
            .await,
        verbose
    ));

    // GetTemplateSummary
    results.push(chk!(
        "GetTemplateSummary",
        client
            .get_template_summary()
            .template_body(template)
            .send()
            .await,
        verbose
    ));

    // DeleteStack
    results.push(chk!(
        "DeleteStack",
        client
            .delete_stack()
            .stack_name("conformance-stack")
            .send()
            .await,
        verbose
    ));

    results
}
