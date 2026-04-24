use crate::chk;
use crate::runner::common::*;
use aws_sdk_lambda::primitives::Blob;
use aws_sdk_lambda::types::{FunctionCode, Runtime};

pub async fn test_lambda(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_lambda::Client::new(&config);
    let mut results = Vec::new();

    // ListFunctions (before creation)
    results.push(chk!(
        "ListFunctions",
        client.list_functions().send().await,
        verbose
    ));

    // CreateFunction — use a minimal ZIP (we can't really invoke it).
    // The ZIP contains a single file handler.py with a dummy handler.
    let zip_bytes = minimal_lambda_zip();
    let create_r = client
        .create_function()
        .function_name("conformance-fn")
        .runtime(Runtime::Python312)
        .role("arn:aws:iam::000000000000:role/conformance-role")
        .handler("handler.handler")
        .code(
            FunctionCode::builder()
                .zip_file(Blob::new(zip_bytes))
                .build(),
        )
        .send()
        .await;
    results.push(chk!("CreateFunction", create_r, verbose));

    // GetFunction
    results.push(chk!(
        "GetFunction",
        client
            .get_function()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // GetFunctionConfiguration
    results.push(chk!(
        "GetFunctionConfiguration",
        client
            .get_function_configuration()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // UpdateFunctionConfiguration
    results.push(chk!(
        "UpdateFunctionConfiguration",
        client
            .update_function_configuration()
            .function_name("conformance-fn")
            .description("updated")
            .send()
            .await,
        verbose
    ));

    // UpdateFunctionCode
    let zip_bytes2 = minimal_lambda_zip();
    results.push(chk!(
        "UpdateFunctionCode",
        client
            .update_function_code()
            .function_name("conformance-fn")
            .zip_file(Blob::new(zip_bytes2))
            .send()
            .await,
        verbose
    ));

    // PublishVersion
    results.push(chk!(
        "PublishVersion",
        client
            .publish_version()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // ListVersionsByFunction
    results.push(chk!(
        "ListVersionsByFunction",
        client
            .list_versions_by_function()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // CreateAlias
    let alias_r = client
        .create_alias()
        .function_name("conformance-fn")
        .name("conformance-alias")
        .function_version("$LATEST")
        .send()
        .await;
    results.push(chk!("CreateAlias", alias_r, verbose));

    // GetAlias
    results.push(chk!(
        "GetAlias",
        client
            .get_alias()
            .function_name("conformance-fn")
            .name("conformance-alias")
            .send()
            .await,
        verbose
    ));

    // ListAliases
    results.push(chk!(
        "ListAliases",
        client
            .list_aliases()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // DeleteAlias
    results.push(chk!(
        "DeleteAlias",
        client
            .delete_alias()
            .function_name("conformance-fn")
            .name("conformance-alias")
            .send()
            .await,
        verbose
    ));

    // CreateEventSourceMapping
    let esm_r = client
        .create_event_source_mapping()
        .function_name("conformance-fn")
        .event_source_arn("arn:aws:kinesis:us-east-1:000000000000:stream/conformance-stream")
        .starting_position(aws_sdk_lambda::types::EventSourcePosition::TrimHorizon)
        .send()
        .await;
    let esm_uuid = esm_r.as_ref().ok().and_then(|r| r.uuid.clone());
    results.push(chk!("CreateEventSourceMapping", esm_r, verbose));

    // ListEventSourceMappings
    results.push(chk!(
        "ListEventSourceMappings",
        client
            .list_event_source_mappings()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // GetEventSourceMapping
    if let Some(ref uuid) = esm_uuid {
        results.push(chk!(
            "GetEventSourceMapping",
            client.get_event_source_mapping().uuid(uuid).send().await,
            verbose
        ));

        // DeleteEventSourceMapping
        results.push(chk!(
            "DeleteEventSourceMapping",
            client.delete_event_source_mapping().uuid(uuid).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetEventSourceMapping".to_string()));
        results.push(OpResult::Skipped("DeleteEventSourceMapping".to_string()));
    }

    // PublishLayerVersion
    let layer_zip = minimal_lambda_zip();
    let layer_r = client
        .publish_layer_version()
        .layer_name("conformance-layer")
        .content(
            aws_sdk_lambda::types::LayerVersionContentInput::builder()
                .zip_file(Blob::new(layer_zip))
                .build(),
        )
        .send()
        .await;
    results.push(chk!("PublishLayerVersion", layer_r, verbose));

    // ListLayers
    results.push(chk!(
        "ListLayers",
        client.list_layers().send().await,
        verbose
    ));

    // ListLayerVersions
    results.push(chk!(
        "ListLayerVersions",
        client
            .list_layer_versions()
            .layer_name("conformance-layer")
            .send()
            .await,
        verbose
    ));

    // DeleteLayerVersion
    results.push(chk!(
        "DeleteLayerVersion",
        client
            .delete_layer_version()
            .layer_name("conformance-layer")
            .version_number(1)
            .send()
            .await,
        verbose
    ));

    // TagResource (Lambda)
    let fn_arn = format!("arn:aws:lambda:us-east-1:000000000000:function:conformance-fn");
    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource(fn_arn.clone())
            .tags("env", "conformance")
            .send()
            .await,
        verbose
    ));

    // ListTags
    results.push(chk!(
        "ListTags",
        client.list_tags().resource(fn_arn.clone()).send().await,
        verbose
    ));

    // UntagResource (Lambda)
    results.push(chk!(
        "UntagResource",
        client
            .untag_resource()
            .resource(fn_arn.clone())
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // AddPermission
    results.push(chk!(
        "AddPermission",
        client
            .add_permission()
            .function_name("conformance-fn")
            .statement_id("conformance-stmt")
            .action("lambda:InvokeFunction")
            .principal("apigateway.amazonaws.com")
            .send()
            .await,
        verbose
    ));

    // GetPolicy
    results.push(chk!(
        "GetPolicy",
        client
            .get_policy()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // RemovePermission
    results.push(chk!(
        "RemovePermission",
        client
            .remove_permission()
            .function_name("conformance-fn")
            .statement_id("conformance-stmt")
            .send()
            .await,
        verbose
    ));

    // GetAccountSettings
    results.push(chk!(
        "GetAccountSettings",
        client.get_account_settings().send().await,
        verbose
    ));

    // Invoke
    results.push(chk!(
        "Invoke",
        client
            .invoke()
            .function_name("conformance-fn")
            .payload(Blob::new(br#"{"hello":"world"}"#.to_vec()))
            .send()
            .await,
        verbose
    ));

    // CreateFunctionUrlConfig
    results.push(chk!(
        "CreateFunctionUrlConfig",
        client
            .create_function_url_config()
            .function_name("conformance-fn")
            .auth_type(aws_sdk_lambda::types::FunctionUrlAuthType::None)
            .send()
            .await,
        verbose
    ));

    // GetFunctionUrlConfig
    results.push(chk!(
        "GetFunctionUrlConfig",
        client
            .get_function_url_config()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // ListFunctionUrlConfigs
    results.push(chk!(
        "ListFunctionUrlConfigs",
        client
            .list_function_url_configs()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    // DeleteFunctionUrlConfig
    results.push(chk!(
        "DeleteFunctionUrlConfig",
        client
            .delete_function_url_config()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "PutFunctionEventInvokeConfig",
        client
            .put_function_event_invoke_config()
            .function_name("conformance-fn")
            .maximum_retry_attempts(2)
            .maximum_event_age_in_seconds(3600)
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "GetFunctionEventInvokeConfig",
        client
            .get_function_event_invoke_config()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "UpdateFunctionEventInvokeConfig",
        client
            .update_function_event_invoke_config()
            .function_name("conformance-fn")
            .maximum_retry_attempts(1)
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "ListFunctionEventInvokeConfigs",
        client
            .list_function_event_invoke_configs()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "DeleteFunctionEventInvokeConfig",
        client
            .delete_function_event_invoke_config()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "DeleteFunction",
        client
            .delete_function()
            .function_name("conformance-fn")
            .send()
            .await,
        verbose
    ));

    results
}

/// Build a minimal valid Lambda ZIP (Python handler) in memory.
fn minimal_lambda_zip() -> Vec<u8> {
    use std::io::Write;
    let handler_code = b"def handler(event, context):\n    return {'statusCode': 200}\n";

    let mut zip_buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("handler.py", opts).unwrap();
        zip.write_all(handler_code).unwrap();
        zip.finish().unwrap();
    }
    zip_buf
}
