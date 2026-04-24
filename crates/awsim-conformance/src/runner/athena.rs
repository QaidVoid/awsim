use crate::chk;
use crate::runner::common::*;

pub async fn test_athena(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_athena::Client::new(&config);
    let mut results = Vec::new();

    // CreateWorkGroup
    results.push(chk!(
        "CreateWorkGroup",
        client
            .create_work_group()
            .name("conformance-workgroup")
            .send()
            .await,
        verbose
    ));

    // ListWorkGroups
    results.push(chk!(
        "ListWorkGroups",
        client.list_work_groups().send().await,
        verbose
    ));

    // GetWorkGroup
    results.push(chk!(
        "GetWorkGroup",
        client
            .get_work_group()
            .work_group("conformance-workgroup")
            .send()
            .await,
        verbose
    ));

    // StartQueryExecution
    let start_qe_r = client
        .start_query_execution()
        .query_string("SELECT 1")
        .work_group("conformance-workgroup")
        .query_execution_context(
            aws_sdk_athena::types::QueryExecutionContext::builder()
                .database("default")
                .build(),
        )
        .result_configuration(
            aws_sdk_athena::types::ResultConfiguration::builder()
                .output_location("s3://conformance-bucket/athena-results/")
                .build(),
        )
        .send()
        .await;
    let query_execution_id = start_qe_r
        .as_ref()
        .ok()
        .and_then(|r| r.query_execution_id.clone());
    results.push(chk!("StartQueryExecution", start_qe_r, verbose));

    // GetQueryExecution
    if let Some(ref qid) = query_execution_id {
        results.push(chk!(
            "GetQueryExecution",
            client
                .get_query_execution()
                .query_execution_id(qid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetQueryExecution".to_string()));
    }

    // ListQueryExecutions
    results.push(chk!(
        "ListQueryExecutions",
        client.list_query_executions().send().await,
        verbose
    ));

    // CreateNamedQuery
    let create_nq_r = client
        .create_named_query()
        .name("conformance-query")
        .database("default")
        .query_string("SELECT 1")
        .send()
        .await;
    let named_query_id = create_nq_r
        .as_ref()
        .ok()
        .and_then(|r| r.named_query_id.clone());
    results.push(chk!("CreateNamedQuery", create_nq_r, verbose));

    // ListNamedQueries
    results.push(chk!(
        "ListNamedQueries",
        client.list_named_queries().send().await,
        verbose
    ));

    // DeleteNamedQuery
    if let Some(ref nqid) = named_query_id {
        results.push(chk!(
            "DeleteNamedQuery",
            client
                .delete_named_query()
                .named_query_id(nqid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteNamedQuery".to_string()));
    }

    // ListDataCatalogs
    results.push(chk!(
        "ListDataCatalogs",
        client.list_data_catalogs().send().await,
        verbose
    ));

    // ListEngineVersions
    results.push(chk!(
        "ListEngineVersions",
        client.list_engine_versions().send().await,
        verbose
    ));

    // TagResource (Athena)
    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource_arn("arn:aws:athena:us-east-1:000000000000:workgroup/conformance-workgroup")
            .tags(
                aws_sdk_athena::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTagsForResource (Athena)
    results.push(chk!(
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_arn("arn:aws:athena:us-east-1:000000000000:workgroup/conformance-workgroup")
            .send()
            .await,
        verbose
    ));

    // UpdateWorkGroup
    results.push(chk!(
        "UpdateWorkGroup",
        client
            .update_work_group()
            .work_group("conformance-workgroup")
            .description("updated")
            .send()
            .await,
        verbose
    ));

    // CreateDataCatalog
    results.push(chk!(
        "CreateDataCatalog",
        client
            .create_data_catalog()
            .name("conformance-catalog")
            .r#type(aws_sdk_athena::types::DataCatalogType::Hive)
            .description("conformance")
            .send()
            .await,
        verbose
    ));

    // GetDataCatalog
    results.push(chk!(
        "GetDataCatalog",
        client
            .get_data_catalog()
            .name("conformance-catalog")
            .send()
            .await,
        verbose
    ));

    // GetNamedQuery
    if let Some(ref nqid) = named_query_id {
        results.push(chk!(
            "GetNamedQuery",
            client
                .get_named_query()
                .named_query_id(nqid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetNamedQuery".to_string()));
    }

    // ListApplicationDPUSizes
    results.push(chk!(
        "ListApplicationDPUSizes",
        client.list_application_dpu_sizes().send().await,
        verbose
    ));

    // GetQueryRuntimeStatistics
    if let Some(ref qid) = query_execution_id {
        results.push(chk!(
            "GetQueryRuntimeStatistics",
            client
                .get_query_runtime_statistics()
                .query_execution_id(qid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetQueryRuntimeStatistics".to_string()));
    }

    // BatchGetPreparedStatement
    results.push(chk!(
        "BatchGetPreparedStatement",
        client
            .batch_get_prepared_statement()
            .work_group("conformance-workgroup")
            .prepared_statement_names("conformance-stmt")
            .send()
            .await,
        verbose
    ));

    // UntagResource
    results.push(chk!(
        "UntagResource",
        client
            .untag_resource()
            .resource_arn("arn:aws:athena:us-east-1:000000000000:workgroup/conformance-workgroup")
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // DeleteWorkGroup (cleanup)
    results.push(chk!(
        "DeleteWorkGroup",
        client
            .delete_work_group()
            .work_group("conformance-workgroup")
            .recursive_delete_option(true)
            .send()
            .await,
        verbose
    ));

    results
}
