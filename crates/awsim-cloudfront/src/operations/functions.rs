use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::{
    ids::{new_etag, now_iso8601},
    state::{CloudFrontFunction, CloudFrontState},
};

fn not_found(name: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchFunctionExists",
        format!("The specified function does not exist: {name}"),
    )
}

fn function_to_value(f: &CloudFrontFunction) -> Value {
    json!({
        "Name": f.name,
        "Status": "DEPLOYED",
        "FunctionConfig": {
            "Comment": f.comment,
            "Runtime": f.runtime,
        },
        "FunctionMetadata": {
            "FunctionARN": format!("arn:aws:cloudfront::123456789012:function/{}", f.name),
            "Stage": f.stage,
            "CreatedTime": f.created_at,
            "LastModifiedTime": f.created_at,
        }
    })
}

pub fn create_function(state: &CloudFrontState, input: &Value) -> Result<Value, AwsError> {
    let name = input
        .get("Name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("InvalidArgument", "Name is required"))?
        .to_string();

    let cfg = input.get("FunctionConfig").unwrap_or(input);
    let comment = cfg.get("Comment").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let runtime = cfg
        .get("Runtime")
        .and_then(|v| v.as_str())
        .unwrap_or("cloudfront-js-2.0")
        .to_string();
    let function_code = input
        .get("FunctionCode")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let etag = new_etag();
    let f = CloudFrontFunction {
        name: name.clone(),
        stage: "DEVELOPMENT".to_string(),
        comment,
        runtime,
        function_code,
        created_at: now_iso8601(),
        etag: etag.clone(),
    };

    let result = function_to_value(&f);
    state.functions.insert(name, f);

    Ok(json!({
        "FunctionSummary": result,
        "Location": "",
        "ETag": etag,
    }))
}

pub fn describe_function(state: &CloudFrontState, name: &str) -> Result<Value, AwsError> {
    let f = state.functions.get(name).ok_or_else(|| not_found(name))?;
    let etag = f.etag.clone();
    let result = function_to_value(&f);
    Ok(json!({ "FunctionSummary": result, "ETag": etag }))
}

pub fn delete_function(state: &CloudFrontState, name: &str) -> Result<Value, AwsError> {
    if state.functions.remove(name).is_none() {
        return Err(not_found(name));
    }
    Ok(json!({}))
}

pub fn list_functions(state: &CloudFrontState) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .functions
        .iter()
        .map(|e| function_to_value(e.value()))
        .collect();
    let qty = items.len();
    Ok(json!({
        "FunctionList": {
            "MaxItems": 100,
            "Quantity": qty,
            "Items": { "FunctionSummary": items }
        }
    }))
}

pub fn publish_function(state: &CloudFrontState, name: &str) -> Result<Value, AwsError> {
    let mut f = state.functions.get_mut(name).ok_or_else(|| not_found(name))?;
    f.stage = "LIVE".to_string();
    let result = function_to_value(&f);
    Ok(json!({ "FunctionSummary": result }))
}
