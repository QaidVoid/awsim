use crate::chk;
use crate::runner::common::*;

pub async fn test_glue(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_glue::Client::new(&config);
    let mut results = Vec::new();

    // CreateDatabase
    results.push(chk!(
        "CreateDatabase",
        client
            .create_database()
            .database_input(
                aws_sdk_glue::types::DatabaseInput::builder()
                    .name("conformance_db")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetDatabase
    results.push(chk!(
        "GetDatabase",
        client.get_database().name("conformance_db").send().await,
        verbose
    ));

    // GetDatabases
    results.push(chk!(
        "GetDatabases",
        client.get_databases().send().await,
        verbose
    ));

    // CreateTable
    results.push(chk!(
        "CreateTable",
        client
            .create_table()
            .database_name("conformance_db")
            .table_input(
                aws_sdk_glue::types::TableInput::builder()
                    .name("conformance_table")
                    .storage_descriptor(
                        aws_sdk_glue::types::StorageDescriptor::builder()
                            .location("s3://conformance-bucket/data/")
                            .input_format(
                                "org.apache.hadoop.mapred.TextInputFormat",
                            )
                            .output_format(
                                "org.apache.hadoop.hive.ql.io.HiveIgnoreKeyTextOutputFormat",
                            )
                            .serde_info(
                                aws_sdk_glue::types::SerDeInfo::builder()
                                    .serialization_library(
                                        "org.apache.hadoop.hive.serde2.lazy.LazySimpleSerDe",
                                    )
                                    .build(),
                            )
                            .build(),
                    )
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetTable
    results.push(chk!(
        "GetTable",
        client
            .get_table()
            .database_name("conformance_db")
            .name("conformance_table")
            .send()
            .await,
        verbose
    ));

    // GetTables
    results.push(chk!(
        "GetTables",
        client
            .get_tables()
            .database_name("conformance_db")
            .send()
            .await,
        verbose
    ));

    // CreateCrawler
    results.push(chk!(
        "CreateCrawler",
        client
            .create_crawler()
            .name("conformance-crawler")
            .role("arn:aws:iam::000000000000:role/glue-crawler-role")
            .targets(
                aws_sdk_glue::types::CrawlerTargets::builder()
                    .s3_targets(
                        aws_sdk_glue::types::S3Target::builder()
                            .path("s3://conformance-bucket/")
                            .build(),
                    )
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetCrawler
    results.push(chk!(
        "GetCrawler",
        client
            .get_crawler()
            .name("conformance-crawler")
            .send()
            .await,
        verbose
    ));

    // GetCrawlers
    results.push(chk!(
        "GetCrawlers",
        client.get_crawlers().send().await,
        verbose
    ));

    // CreateJob
    results.push(chk!(
        "CreateJob",
        client
            .create_job()
            .name("conformance-job")
            .role("arn:aws:iam::000000000000:role/glue-job-role")
            .command(
                aws_sdk_glue::types::JobCommand::builder()
                    .name("glueetl")
                    .script_location("s3://conformance-bucket/scripts/etl.py")
                    .python_version("3")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetJob
    results.push(chk!(
        "GetJob",
        client.get_job().job_name("conformance-job").send().await,
        verbose
    ));

    // GetJobs
    results.push(chk!(
        "GetJobs",
        client.get_jobs().send().await,
        verbose
    ));

    // CreateTrigger
    results.push(chk!(
        "CreateTrigger",
        client
            .create_trigger()
            .name("conformance-trigger")
            .r#type(aws_sdk_glue::types::TriggerType::OnDemand)
            .actions(
                aws_sdk_glue::types::Action::builder()
                    .job_name("conformance-job")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // GetTriggers
    results.push(chk!(
        "GetTriggers",
        client.get_triggers().send().await,
        verbose
    ));

    // CreateWorkflow
    results.push(chk!(
        "CreateWorkflow",
        client
            .create_workflow()
            .name("conformance-workflow")
            .send()
            .await,
        verbose
    ));

    // GetWorkflow
    results.push(chk!(
        "GetWorkflow",
        client
            .get_workflow()
            .name("conformance-workflow")
            .send()
            .await,
        verbose
    ));

    // DeleteWorkflow
    results.push(chk!(
        "DeleteWorkflow",
        client
            .delete_workflow()
            .name("conformance-workflow")
            .send()
            .await,
        verbose
    ));

    // GetTrigger
    results.push(chk!(
        "GetTrigger",
        client
            .get_trigger()
            .name("conformance-trigger")
            .send()
            .await,
        verbose
    ));

    // ListWorkflows
    let _ = client
        .create_workflow()
        .name("conformance-workflow2")
        .send()
        .await;
    results.push(chk!(
        "ListWorkflows",
        client.list_workflows().send().await,
        verbose
    ));
    let _ = client
        .delete_workflow()
        .name("conformance-workflow2")
        .send()
        .await;

    // UpdateJob
    results.push(chk!(
        "UpdateJob",
        client
            .update_job()
            .job_name("conformance-job")
            .job_update(
                aws_sdk_glue::types::JobUpdate::builder()
                    .role("arn:aws:iam::000000000000:role/glue-job-role-updated")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // CreateConnection / GetConnection / UpdateConnection
    results.push(chk!(
        "CreateConnection",
        client
            .create_connection()
            .connection_input(
                aws_sdk_glue::types::ConnectionInput::builder()
                    .name("conformance-connection")
                    .connection_type(aws_sdk_glue::types::ConnectionType::Jdbc)
                    .connection_properties(
                        aws_sdk_glue::types::ConnectionPropertyKey::JdbcConnectionUrl,
                        "jdbc:postgresql://host:5432/db",
                    )
                    .connection_properties(
                        aws_sdk_glue::types::ConnectionPropertyKey::UserName,
                        "admin",
                    )
                    .connection_properties(
                        aws_sdk_glue::types::ConnectionPropertyKey::Password,
                        "secret",
                    )
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "GetConnection",
        client
            .get_connection()
            .name("conformance-connection")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "UpdateConnection",
        client
            .update_connection()
            .name("conformance-connection")
            .connection_input(
                aws_sdk_glue::types::ConnectionInput::builder()
                    .name("conformance-connection")
                    .connection_type(aws_sdk_glue::types::ConnectionType::Jdbc)
                    .description("updated")
                    .connection_properties(
                        aws_sdk_glue::types::ConnectionPropertyKey::JdbcConnectionUrl,
                        "jdbc:postgresql://host2:5432/db",
                    )
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    let _ = client
        .delete_connection()
        .connection_name("conformance-connection")
        .send()
        .await;

    // GetTableVersion
    results.push(chk!(
        "GetTableVersion",
        client
            .get_table_version()
            .database_name("conformance_db")
            .table_name("conformance_table")
            .version_id("1")
            .send()
            .await,
        verbose
    ));

    // CreatePartition (so we have something for GetPartition / BatchGetPartition)
    let _ = client
        .create_partition()
        .database_name("conformance_db")
        .table_name("conformance_table")
        .partition_input(
            aws_sdk_glue::types::PartitionInput::builder()
                .values("2024")
                .values("01")
                .build(),
        )
        .send()
        .await;

    results.push(chk!(
        "GetPartition",
        client
            .get_partition()
            .database_name("conformance_db")
            .table_name("conformance_table")
            .partition_values("2024")
            .partition_values("01")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "BatchGetPartition",
        client
            .batch_get_partition()
            .database_name("conformance_db")
            .table_name("conformance_table")
            .partitions_to_get(
                aws_sdk_glue::types::PartitionValueList::builder()
                    .values("2024")
                    .values("01")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // BatchDeleteTable (create extra table to delete)
    let _ = client
        .create_table()
        .database_name("conformance_db")
        .table_input(
            aws_sdk_glue::types::TableInput::builder()
                .name("conformance_table_extra")
                .build()
                .unwrap(),
        )
        .send()
        .await;

    results.push(chk!(
        "BatchDeleteTable",
        client
            .batch_delete_table()
            .database_name("conformance_db")
            .tables_to_delete("conformance_table_extra")
            .send()
            .await,
        verbose
    ));

    // DeleteTrigger
    results.push(chk!(
        "DeleteTrigger",
        client
            .delete_trigger()
            .name("conformance-trigger")
            .send()
            .await,
        verbose
    ));

    // DeleteTable (cleanup)
    results.push(chk!(
        "DeleteTable",
        client
            .delete_table()
            .database_name("conformance_db")
            .name("conformance_table")
            .send()
            .await,
        verbose
    ));

    // DeleteCrawler (cleanup)
    results.push(chk!(
        "DeleteCrawler",
        client
            .delete_crawler()
            .name("conformance-crawler")
            .send()
            .await,
        verbose
    ));

    // DeleteJob (cleanup)
    results.push(chk!(
        "DeleteJob",
        client.delete_job().job_name("conformance-job").send().await,
        verbose
    ));

    // DeleteDatabase (cleanup)
    results.push(chk!(
        "DeleteDatabase",
        client.delete_database().name("conformance_db").send().await,
        verbose
    ));

    results
}
