use crate::chk;
use crate::runner::common::*;

pub async fn test_bedrock(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_bedrock::Client::new(&config);
    let mut results = Vec::new();

    results.push(chk!(
        "ListFoundationModels",
        client.list_foundation_models().send().await,
        verbose
    ));

    results.push(chk!(
        "GetFoundationModel",
        client
            .get_foundation_model()
            .model_identifier("anthropic.claude-v2:1")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "ListGuardrails",
        client.list_guardrails().send().await,
        verbose
    ));

    results.push(chk!(
        "ListProvisionedModelThroughputs",
        client.list_provisioned_model_throughputs().send().await,
        verbose
    ));

    results.push(chk!(
        "ListCustomModels",
        client.list_custom_models().send().await,
        verbose
    ));

    results.push(chk!(
        "ListModelCustomizationJobs",
        client.list_model_customization_jobs().send().await,
        verbose
    ));

    let create_guard_r = client
        .create_guardrail()
        .name("conformance-guardrail")
        .blocked_input_messaging("Blocked input.")
        .blocked_outputs_messaging("Blocked output.")
        .send()
        .await;
    let guardrail_id = create_guard_r.as_ref().ok().map(|r| r.guardrail_id.clone());
    results.push(chk!("CreateGuardrail", create_guard_r, verbose));

    if let Some(ref gid) = guardrail_id {
        results.push(chk!(
            "GetGuardrail",
            client
                .get_guardrail()
                .guardrail_identifier(gid)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "DeleteGuardrail",
            client
                .delete_guardrail()
                .guardrail_identifier(gid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetGuardrail".to_string()));
        results.push(OpResult::Skipped("DeleteGuardrail".to_string()));
    }

    let create_pmt_r = client
        .create_provisioned_model_throughput()
        .provisioned_model_name("conformance-pmt")
        .model_id("anthropic.claude-v2:1")
        .model_units(1)
        .send()
        .await;
    let pmt_arn = create_pmt_r
        .as_ref()
        .ok()
        .map(|r| r.provisioned_model_arn.clone());
    results.push(chk!("CreateProvisionedModelThroughput", create_pmt_r, verbose));

    if let Some(ref arn) = pmt_arn {
        let pmt_id = arn.rsplit('/').next().unwrap_or(arn);
        results.push(chk!(
            "GetProvisionedModelThroughput",
            client
                .get_provisioned_model_throughput()
                .provisioned_model_id(pmt_id)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "DeleteProvisionedModelThroughput",
            client
                .delete_provisioned_model_throughput()
                .provisioned_model_id(pmt_id)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetProvisionedModelThroughput".to_string()));
        results.push(OpResult::Skipped("DeleteProvisionedModelThroughput".to_string()));
    }

    let create_job_r = client
        .create_model_customization_job()
        .job_name("conformance-job")
        .custom_model_name("conformance-custom-model")
        .role_arn("arn:aws:iam::000000000000:role/bedrock-role")
        .base_model_identifier("anthropic.claude-v2:1")
        .training_data_config(
            aws_sdk_bedrock::types::TrainingDataConfig::builder()
                .s3_uri("s3://conformance-bucket/training.jsonl")
                .build(),
        )
        .output_data_config(
            aws_sdk_bedrock::types::OutputDataConfig::builder()
                .s3_uri("s3://conformance-bucket/output/")
                .build()
                .unwrap_or_else(|_| panic!("output data config")),
        )
        .send()
        .await;
    let job_arn = create_job_r.as_ref().ok().map(|r| r.job_arn.clone());
    results.push(chk!("CreateModelCustomizationJob", create_job_r, verbose));

    if let Some(ref jarn) = job_arn {
        results.push(chk!(
            "GetModelCustomizationJob",
            client
                .get_model_customization_job()
                .job_identifier(jarn)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "StopModelCustomizationJob",
            client
                .stop_model_customization_job()
                .job_identifier(jarn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetModelCustomizationJob".to_string()));
        results.push(OpResult::Skipped("StopModelCustomizationJob".to_string()));
    }

    results.push(chk!(
        "PutModelInvocationLoggingConfiguration",
        client
            .put_model_invocation_logging_configuration()
            .logging_config(
                aws_sdk_bedrock::types::LoggingConfig::builder()
                    .text_data_delivery_enabled(true)
                    .image_data_delivery_enabled(false)
                    .embedding_data_delivery_enabled(false)
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "GetModelInvocationLoggingConfiguration",
        client.get_model_invocation_logging_configuration().send().await,
        verbose
    ));

    let tag_arn = "arn:aws:bedrock:us-east-1:000000000000:custom-model/conformance";

    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource_arn(tag_arn)
            .tags(
                aws_sdk_bedrock::types::Tag::builder()
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
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_arn(tag_arn)
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "UntagResource",
        client
            .untag_resource()
            .resource_arn(tag_arn)
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "GetCustomModel",
        client
            .get_custom_model()
            .model_identifier("nonexistent-custom-model")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "DeleteCustomModel",
        client
            .delete_custom_model()
            .model_identifier("nonexistent-custom-model")
            .send()
            .await,
        verbose
    ));

    results
}
