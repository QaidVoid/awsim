use crate::chk;
use crate::runner::common::*;

pub async fn test_batch(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_batch::Client::new(&config);
    let mut results = Vec::new();

    results.push(chk!(
        "CreateComputeEnvironment",
        client
            .create_compute_environment()
            .compute_environment_name("conf-ce")
            .r#type(aws_sdk_batch::types::CeType::Managed)
            .service_role("arn:aws:iam::000000000000:role/BatchRole")
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "DescribeComputeEnvironments",
        client.describe_compute_environments().send().await,
        verbose
    ));
    results.push(chk!(
        "CreateJobQueue",
        client
            .create_job_queue()
            .job_queue_name("conf-queue")
            .priority(1)
            .compute_environment_order(
                aws_sdk_batch::types::ComputeEnvironmentOrder::builder()
                    .order(1)
                    .compute_environment("conf-ce")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "DescribeJobQueues",
        client.describe_job_queues().send().await,
        verbose
    ));
    results.push(chk!(
        "RegisterJobDefinition",
        client
            .register_job_definition()
            .job_definition_name("conf-jobdef")
            .r#type(aws_sdk_batch::types::JobDefinitionType::Container)
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "DescribeJobDefinitions",
        client.describe_job_definitions().send().await,
        verbose
    ));
    results.push(chk!(
        "ListJobs",
        client.list_jobs().job_queue("conf-queue").send().await,
        verbose
    ));

    results
}
