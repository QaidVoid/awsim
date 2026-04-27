use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::resource_not_found,
    state::{FunctionVersion, LambdaState},
    util::{now_iso8601, opt_str, require_str},
};

pub fn publish_version(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let description = opt_str(input, "Description").unwrap_or("").to_string();

    let mut f = state
        .functions
        .get_mut(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    let version_number = (f.versions.len() + 1).to_string();
    let now = now_iso8601();

    let ver = FunctionVersion {
        version: version_number.clone(),
        description: description.clone(),
        code_sha256: f.code_sha256.clone(),
        code_size: f.code_size,
        code: f.code.clone(),
        last_modified: now.clone(),
    };

    f.versions.push(ver);

    Ok(json!({
        "FunctionName": f.name,
        "FunctionArn": format!("{}:{}", f.arn, version_number),
        "Runtime": f.runtime,
        "Role": f.role,
        "Handler": f.handler,
        "Description": description,
        "Timeout": f.timeout,
        "MemorySize": f.memory_size,
        "CodeSha256": f.code_sha256,
        "CodeSize": f.code_size,
        "Version": version_number,
        "LastModified": now,
        "State": "Active",
    }))
}

pub fn list_versions_by_function(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let f = state
        .functions
        .get(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    // Include $LATEST plus all published versions
    let mut versions: Vec<Value> = vec![json!({
        "FunctionName": f.name,
        "FunctionArn": f.arn,
        "Runtime": f.runtime,
        "Role": f.role,
        "Handler": f.handler,
        "Description": f.description,
        "Timeout": f.timeout,
        "MemorySize": f.memory_size,
        "CodeSha256": f.code_sha256,
        "CodeSize": f.code_size,
        "Version": "$LATEST",
        "LastModified": f.last_modified,
        "State": f.state,
    })];

    for ver in &f.versions {
        versions.push(json!({
            "FunctionName": f.name,
            "FunctionArn": format!("{}:{}", f.arn, ver.version),
            "Runtime": f.runtime,
            "Role": f.role,
            "Handler": f.handler,
            "Description": ver.description,
            "Timeout": f.timeout,
            "MemorySize": f.memory_size,
            "CodeSha256": ver.code_sha256,
            "CodeSize": ver.code_size,
            "Version": ver.version,
            "LastModified": ver.last_modified,
            "State": "Active",
        }));
    }

    Ok(json!({ "Versions": versions }))
}
