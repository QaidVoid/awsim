//! API Gateway v1 puts every paginated collection under `item`.
//!
//! The SDK models rename that member to `items`, so returning `items`
//! looked correct but parsed as an empty result: `aws apigateway
//! get-rest-apis` printed nothing with an API present.

use awsim_apigateway::ApiGatewayV1Service;
use awsim_core::{RequestContext, ServiceHandler};
use serde_json::{Value, json};

fn ctx() -> RequestContext {
    RequestContext::new("apigateway", "us-east-1")
}

fn collection<'a>(response: &'a Value, op: &str) -> &'a Vec<Value> {
    response["item"]
        .as_array()
        .unwrap_or_else(|| panic!("{op} should return an `item` collection: {response}"))
}

async fn api_with_stage() -> (ApiGatewayV1Service, String) {
    let svc = ApiGatewayV1Service::new();
    let api = svc
        .handle("CreateRestApi", json!({ "name": "shape-api" }), &ctx())
        .await
        .expect("CreateRestApi");
    let id = api["id"].as_str().expect("id").to_string();
    (svc, id)
}

#[tokio::test]
async fn rest_apis_come_back_under_item() {
    let (svc, _) = api_with_stage().await;
    let out = svc
        .handle("GetRestApis", json!({}), &ctx())
        .await
        .expect("GetRestApis");
    let apis = collection(&out, "GetRestApis");
    assert_eq!(apis.len(), 1);
    assert_eq!(apis[0]["name"], "shape-api");
    assert!(
        out.get("items").is_none(),
        "`items` is the model name, not the wire name: {out}"
    );
}

#[tokio::test]
async fn every_v1_collection_uses_the_same_key() {
    let (svc, id) = api_with_stage().await;

    let cases: Vec<(&str, Value)> = vec![
        ("GetResources", json!({ "restapi_id": id })),
        ("GetDeployments", json!({ "restapi_id": id })),
        ("GetAuthorizers", json!({ "restapi_id": id })),
        ("GetStages", json!({ "restapi_id": id })),
        ("GetApiKeys", json!({})),
        ("GetUsagePlans", json!({})),
    ];

    for (op, input) in cases {
        let out = svc
            .handle(op, input, &ctx())
            .await
            .unwrap_or_else(|e| panic!("{op}: {e:?}"));
        collection(&out, op);
    }
}
