use crate::chk;
use crate::runner::common::*;

pub async fn test_ecs(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_ecs::Client::new(&config);
    let mut results = Vec::new();

    // CreateCluster
    let create_r = client
        .create_cluster()
        .cluster_name("conformance-cluster")
        .send()
        .await;
    results.push(chk!("CreateCluster", create_r, verbose));

    // ListClusters
    results.push(chk!(
        "ListClusters",
        client.list_clusters().send().await,
        verbose
    ));

    // DescribeClusters
    results.push(chk!(
        "DescribeClusters",
        client
            .describe_clusters()
            .clusters("conformance-cluster")
            .send()
            .await,
        verbose
    ));

    // RegisterTaskDefinition
    let td_r = client
        .register_task_definition()
        .family("conformance-task")
        .container_definitions(
            aws_sdk_ecs::types::ContainerDefinition::builder()
                .name("conformance-container")
                .image("public.ecr.aws/nginx/nginx:latest")
                .build(),
        )
        .send()
        .await;
    let task_def_arn = td_r
        .as_ref()
        .ok()
        .and_then(|r| r.task_definition.as_ref())
        .and_then(|td| td.task_definition_arn.clone());
    results.push(chk!("RegisterTaskDefinition", td_r, verbose));

    // ListTaskDefinitions
    results.push(chk!(
        "ListTaskDefinitions",
        client.list_task_definitions().send().await,
        verbose
    ));

    // ListTaskDefinitionFamilies
    results.push(chk!(
        "ListTaskDefinitionFamilies",
        client.list_task_definition_families().send().await,
        verbose
    ));

    // DescribeTaskDefinition
    results.push(chk!(
        "DescribeTaskDefinition",
        client
            .describe_task_definition()
            .task_definition("conformance-task")
            .send()
            .await,
        verbose
    ));

    // CreateService
    results.push(chk!(
        "CreateService",
        client
            .create_service()
            .cluster("conformance-cluster")
            .service_name("conformance-service")
            .task_definition("conformance-task")
            .desired_count(0)
            .send()
            .await,
        verbose
    ));

    // ListServices
    results.push(chk!(
        "ListServices",
        client
            .list_services()
            .cluster("conformance-cluster")
            .send()
            .await,
        verbose
    ));

    // DescribeServices
    results.push(chk!(
        "DescribeServices",
        client
            .describe_services()
            .cluster("conformance-cluster")
            .services("conformance-service")
            .send()
            .await,
        verbose
    ));

    // UpdateService
    results.push(chk!(
        "UpdateService",
        client
            .update_service()
            .cluster("conformance-cluster")
            .service("conformance-service")
            .desired_count(0)
            .send()
            .await,
        verbose
    ));

    // RunTask
    let run_task_r = client
        .run_task()
        .cluster("conformance-cluster")
        .task_definition("conformance-task")
        .send()
        .await;
    let task_arn = run_task_r
        .as_ref()
        .ok()
        .and_then(|r| r.tasks.as_ref())
        .and_then(|t| t.first())
        .and_then(|t| t.task_arn.clone());
    results.push(chk!("RunTask", run_task_r, verbose));

    // ListTasks
    results.push(chk!(
        "ListTasks",
        client
            .list_tasks()
            .cluster("conformance-cluster")
            .send()
            .await,
        verbose
    ));

    // DescribeTasks
    if let Some(ref tarn) = task_arn {
        results.push(chk!(
            "DescribeTasks",
            client
                .describe_tasks()
                .cluster("conformance-cluster")
                .tasks(tarn)
                .send()
                .await,
            verbose
        ));

        // StopTask
        results.push(chk!(
            "StopTask",
            client
                .stop_task()
                .cluster("conformance-cluster")
                .task(tarn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DescribeTasks".to_string()));
        results.push(OpResult::Skipped("StopTask".to_string()));
    }

    // DeregisterTaskDefinition
    if let Some(ref tdarn) = task_def_arn {
        results.push(chk!(
            "DeregisterTaskDefinition",
            client
                .deregister_task_definition()
                .task_definition(tdarn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeregisterTaskDefinition".to_string()));
    }

    // TagResource (ECS)
    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource_arn(format!(
                "arn:aws:ecs:us-east-1:000000000000:cluster/conformance-cluster"
            ))
            .tags(
                aws_sdk_ecs::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTagsForResource (ECS)
    results.push(chk!(
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_arn(format!(
                "arn:aws:ecs:us-east-1:000000000000:cluster/conformance-cluster"
            ))
            .send()
            .await,
        verbose
    ));

    // UntagResource (ECS)
    results.push(chk!(
        "UntagResource",
        client
            .untag_resource()
            .resource_arn(format!(
                "arn:aws:ecs:us-east-1:000000000000:cluster/conformance-cluster"
            ))
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // PutClusterCapacityProviders
    results.push(chk!(
        "PutClusterCapacityProviders",
        client
            .put_cluster_capacity_providers()
            .cluster("conformance-cluster")
            .default_capacity_provider_strategy(
                aws_sdk_ecs::types::CapacityProviderStrategyItem::builder()
                    .capacity_provider("FARGATE")
                    .weight(1)
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // DescribeCapacityProviders
    results.push(chk!(
        "DescribeCapacityProviders",
        client.describe_capacity_providers().send().await,
        verbose
    ));

    // PutAccountSetting
    results.push(chk!(
        "PutAccountSetting",
        client
            .put_account_setting()
            .name(aws_sdk_ecs::types::SettingName::ContainerInsights)
            .value("enabled")
            .send()
            .await,
        verbose
    ));

    // ListAccountSettings
    results.push(chk!(
        "ListAccountSettings",
        client.list_account_settings().send().await,
        verbose
    ));

    // DeleteService
    results.push(chk!(
        "DeleteService",
        client
            .delete_service()
            .cluster("conformance-cluster")
            .service("conformance-service")
            .send()
            .await,
        verbose
    ));

    // DescribeContainerInstances
    results.push(chk!(
        "DescribeContainerInstances",
        client
            .describe_container_instances()
            .cluster("conformance-cluster")
            .container_instances("ci-stub")
            .send()
            .await,
        verbose
    ));

    // PutAttributes
    results.push(chk!(
        "PutAttributes",
        client
            .put_attributes()
            .cluster("conformance-cluster")
            .attributes(
                aws_sdk_ecs::types::Attribute::builder()
                    .name("env")
                    .value("conformance")
                    .target_type(aws_sdk_ecs::types::TargetType::ContainerInstance)
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // ListAttributes
    results.push(chk!(
        "ListAttributes",
        client
            .list_attributes()
            .cluster("conformance-cluster")
            .target_type(aws_sdk_ecs::types::TargetType::ContainerInstance)
            .send()
            .await,
        verbose
    ));

    // DeleteCluster
    results.push(chk!(
        "DeleteCluster",
        client
            .delete_cluster()
            .cluster("conformance-cluster")
            .send()
            .await,
        verbose
    ));

    results
}
