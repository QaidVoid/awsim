use crate::chk;
use crate::runner::common::*;

pub async fn test_ec2(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_ec2::Client::new(&config);
    let mut results = Vec::new();

    // RunInstances
    let run_r = client
        .run_instances()
        .image_id("ami-00000000conformance")
        .instance_type(aws_sdk_ec2::types::InstanceType::T2Micro)
        .min_count(1)
        .max_count(1)
        .send()
        .await;
    let instance_id = run_r
        .as_ref()
        .ok()
        .and_then(|r| r.instances.as_ref())
        .and_then(|i| i.first())
        .and_then(|i| i.instance_id.clone());
    results.push(chk!("RunInstances", run_r, verbose));

    // DescribeInstances
    results.push(chk!(
        "DescribeInstances",
        client.describe_instances().send().await,
        verbose
    ));

    if let Some(ref iid) = instance_id {
        // CreateTags
        results.push(chk!(
            "CreateTags",
            client
                .create_tags()
                .resources(iid)
                .tags(
                    aws_sdk_ec2::types::Tag::builder()
                        .key("env")
                        .value("conformance")
                        .build(),
                )
                .send()
                .await,
            verbose
        ));

        // DescribeTags
        results.push(chk!(
            "DescribeTags",
            client
                .describe_tags()
                .filters(
                    aws_sdk_ec2::types::Filter::builder()
                        .name("resource-id")
                        .values(iid)
                        .build(),
                )
                .send()
                .await,
            verbose
        ));

        // TerminateInstances
        results.push(chk!(
            "TerminateInstances",
            client.terminate_instances().instance_ids(iid).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("CreateTags".to_string()));
        results.push(OpResult::Skipped("DescribeTags".to_string()));
        results.push(OpResult::Skipped("TerminateInstances".to_string()));
    }

    results
}
