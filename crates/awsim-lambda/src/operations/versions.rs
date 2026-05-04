use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::resource_not_found,
    operations::functions::persist_code,
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

    let current_bytes = {
        let f = state
            .functions
            .get(name)
            .ok_or_else(|| resource_not_found("function", name))?;
        f.code
            .as_ref()
            .map(|c| c.read_all())
            .transpose()
            .map_err(|e| AwsError::internal(format!("read function code: {e}")))?
    };

    let mut f = state
        .functions
        .get_mut(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    // Allocate the next version number from the running maximum. Using
    // `len() + 1` would re-issue a previously-deleted version number,
    // which AWS never does — once a version exists, its identity is
    // permanent even after deletion.
    let next_version: u64 = f
        .versions
        .iter()
        .filter_map(|v| v.version.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        + 1;
    let version_number = next_version.to_string();
    let now = now_iso8601();

    let version_code = persist_code(state, name, &version_number, current_bytes)?;

    let ver = FunctionVersion {
        version: version_number.clone(),
        description: description.clone(),
        code_sha256: f.code_sha256.clone(),
        code_size: f.code_size,
        code: version_code,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::functions::create_function;
    use crate::state::LambdaState;

    fn ctx() -> RequestContext {
        RequestContext::new("lambda", "us-east-1")
    }

    fn empty_zip_b64() -> String {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD as BASE64;
        let bytes: [u8; 22] = [
            0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        BASE64.encode(bytes)
    }

    fn create_test_fn(state: &LambdaState) {
        create_function(
            state,
            &json!({
                "FunctionName": "f",
                "Role": "arn:aws:iam::000000000000:role/test",
                "Code": { "ZipFile": empty_zip_b64() },
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn publish_version_starts_at_one() {
        let state = LambdaState::default();
        create_test_fn(&state);
        let resp = publish_version(&state, &json!({ "FunctionName": "f" }), &ctx()).unwrap();
        assert_eq!(resp["Version"], json!("1"));
    }

    #[test]
    fn publish_version_after_delete_does_not_reuse_number() {
        let state = LambdaState::default();
        create_test_fn(&state);
        publish_version(&state, &json!({ "FunctionName": "f" }), &ctx()).unwrap();
        publish_version(&state, &json!({ "FunctionName": "f" }), &ctx()).unwrap();
        // Simulate version 1 being deleted: drop the first entry from
        // the in-memory list. The next publish must allocate version 3,
        // not reuse 1 or 2.
        {
            let mut f = state.functions.get_mut("f").unwrap();
            f.versions.retain(|v| v.version != "1");
        }
        let resp = publish_version(&state, &json!({ "FunctionName": "f" }), &ctx()).unwrap();
        assert_eq!(resp["Version"], json!("3"));
    }
}
