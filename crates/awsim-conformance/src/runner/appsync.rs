use crate::chk;
use crate::runner::common::*;

pub async fn test_appsync(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_appsync::Client::new(&config);
    let mut results = Vec::new();

    // CreateGraphqlApi
    let create_r = client
        .create_graphql_api()
        .name("conformance-api")
        .authentication_type(aws_sdk_appsync::types::AuthenticationType::ApiKey)
        .send()
        .await;
    let api_id = create_r
        .as_ref()
        .ok()
        .and_then(|r| r.graphql_api.as_ref())
        .and_then(|a| a.api_id.clone());
    results.push(chk!("CreateGraphqlApi", create_r, verbose));

    // ListGraphqlApis
    results.push(chk!(
        "ListGraphqlApis",
        client.list_graphql_apis().send().await,
        verbose
    ));

    if let Some(ref aid) = api_id {
        results.push(chk!(
            "GetGraphqlApi",
            client.get_graphql_api().api_id(aid).send().await,
            verbose
        ));

        results.push(chk!(
            "UpdateGraphqlApi",
            client
                .update_graphql_api()
                .api_id(aid)
                .name("conformance-api-renamed")
                .authentication_type(aws_sdk_appsync::types::AuthenticationType::ApiKey)
                .send()
                .await,
            verbose
        ));

        let api_arn = format!("arn:aws:appsync:us-east-1:000000000000:apis/{}", aid);

        results.push(chk!(
            "CreateApiKey",
            client
                .create_api_key()
                .api_id(aid)
                .description("conformance-key")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ListApiKeys",
            client.list_api_keys().api_id(aid).send().await,
            verbose
        ));

        results.push(chk!(
            "CreateDataSource",
            client
                .create_data_source()
                .api_id(aid)
                .name("noneds")
                .r#type(aws_sdk_appsync::types::DataSourceType::None)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "GetDataSource",
            client
                .get_data_source()
                .api_id(aid)
                .name("noneds")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ListDataSources",
            client.list_data_sources().api_id(aid).send().await,
            verbose
        ));

        results.push(chk!(
            "UpdateDataSource",
            client
                .update_data_source()
                .api_id(aid)
                .name("noneds")
                .r#type(aws_sdk_appsync::types::DataSourceType::None)
                .description("updated")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "CreateType",
            client
                .create_type()
                .api_id(aid)
                .definition("type Query { hello: String }")
                .format(aws_sdk_appsync::types::TypeDefinitionFormat::Sdl)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "UpdateType",
            client
                .update_type()
                .api_id(aid)
                .type_name("Query")
                .definition("type Query { hello: String world: Int }")
                .format(aws_sdk_appsync::types::TypeDefinitionFormat::Sdl)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "CreateResolver",
            client
                .create_resolver()
                .api_id(aid)
                .type_name("Query")
                .field_name("hello")
                .data_source_name("noneds")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "GetResolver",
            client
                .get_resolver()
                .api_id(aid)
                .type_name("Query")
                .field_name("hello")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ListResolvers",
            client
                .list_resolvers()
                .api_id(aid)
                .type_name("Query")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "UpdateResolver",
            client
                .update_resolver()
                .api_id(aid)
                .type_name("Query")
                .field_name("hello")
                .data_source_name("noneds")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "GetType",
            client
                .get_type()
                .api_id(aid)
                .type_name("Query")
                .format(aws_sdk_appsync::types::TypeDefinitionFormat::Sdl)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ListTypes",
            client
                .list_types()
                .api_id(aid)
                .format(aws_sdk_appsync::types::TypeDefinitionFormat::Sdl)
                .send()
                .await,
            verbose
        ));

        let create_fn_r = client
            .create_function()
            .api_id(aid)
            .name("conformance-fn")
            .data_source_name("noneds")
            .function_version("2018-05-29")
            .send()
            .await;
        let function_id = create_fn_r
            .as_ref()
            .ok()
            .and_then(|r| r.function_configuration.as_ref())
            .and_then(|f| f.function_id.clone());
        results.push(chk!("CreateFunction", create_fn_r, verbose));

        results.push(chk!(
            "ListFunctions",
            client.list_functions().api_id(aid).send().await,
            verbose
        ));

        if let Some(ref fid) = function_id {
            results.push(chk!(
                "GetFunction",
                client
                    .get_function()
                    .api_id(aid)
                    .function_id(fid)
                    .send()
                    .await,
                verbose
            ));

            results.push(chk!(
                "UpdateFunction",
                client
                    .update_function()
                    .api_id(aid)
                    .function_id(fid)
                    .name("conformance-fn")
                    .data_source_name("noneds")
                    .function_version("2018-05-29")
                    .send()
                    .await,
                verbose
            ));

            results.push(chk!(
                "DeleteFunction",
                client
                    .delete_function()
                    .api_id(aid)
                    .function_id(fid)
                    .send()
                    .await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("GetFunction".to_string()));
            results.push(OpResult::Skipped("UpdateFunction".to_string()));
            results.push(OpResult::Skipped("DeleteFunction".to_string()));
        }

        let api_key_id = client
            .list_api_keys()
            .api_id(aid)
            .send()
            .await
            .ok()
            .and_then(|r| r.api_keys.and_then(|k| k.into_iter().next()))
            .and_then(|k| k.id);

        if let Some(ref kid) = api_key_id {
            results.push(chk!(
                "UpdateApiKey",
                client
                    .update_api_key()
                    .api_id(aid)
                    .id(kid)
                    .description("updated-key")
                    .send()
                    .await,
                verbose
            ));

            results.push(chk!(
                "DeleteApiKey",
                client.delete_api_key().api_id(aid).id(kid).send().await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("UpdateApiKey".to_string()));
            results.push(OpResult::Skipped("DeleteApiKey".to_string()));
        }

        results.push(chk!(
            "StartSchemaCreation",
            client
                .start_schema_creation()
                .api_id(aid)
                .definition(aws_sdk_appsync::primitives::Blob::new(
                    b"type Query { hello: String }".to_vec(),
                ))
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "GetSchemaCreationStatus",
            client.get_schema_creation_status().api_id(aid).send().await,
            verbose
        ));

        results.push(chk!(
            "DeleteResolver",
            client
                .delete_resolver()
                .api_id(aid)
                .type_name("Query")
                .field_name("hello")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "DeleteType",
            client
                .delete_type()
                .api_id(aid)
                .type_name("Query")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "DeleteDataSource",
            client
                .delete_data_source()
                .api_id(aid)
                .name("noneds")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "GetIntrospectionSchema",
            client
                .get_introspection_schema()
                .api_id(aid)
                .format(aws_sdk_appsync::types::OutputType::Sdl)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "TagResource",
            client
                .tag_resource()
                .resource_arn(&api_arn)
                .tags("env", "conformance")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ListTagsForResource",
            client
                .list_tags_for_resource()
                .resource_arn(&api_arn)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "UntagResource",
            client
                .untag_resource()
                .resource_arn(&api_arn)
                .tag_keys("env")
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "ListSourceApiAssociations",
            client
                .list_source_api_associations()
                .api_id(aid)
                .send()
                .await,
            verbose
        ));

        results.push(chk!(
            "DeleteGraphqlApi",
            client.delete_graphql_api().api_id(aid).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetGraphqlApi".to_string()));
        results.push(OpResult::Skipped("CreateApiKey".to_string()));
        results.push(OpResult::Skipped("ListApiKeys".to_string()));
        results.push(OpResult::Skipped("CreateDataSource".to_string()));
        results.push(OpResult::Skipped("GetDataSource".to_string()));
        results.push(OpResult::Skipped("ListSourceApiAssociations".to_string()));
        results.push(OpResult::Skipped("DeleteGraphqlApi".to_string()));
    }

    results
}
