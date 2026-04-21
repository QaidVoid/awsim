use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::{
    ids::{new_oac_id, now_iso8601},
    state::{CloudFrontState, OriginAccessControl},
};

fn not_found(id: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchOriginAccessControl",
        format!("The specified origin access control does not exist: {id}"),
    )
}

fn oac_to_value(oac: &OriginAccessControl) -> Value {
    json!({
        "Id": oac.id,
        "OriginAccessControlConfig": {
            "Name": oac.name,
            "Description": oac.description,
            "SigningProtocol": oac.signing_protocol,
            "SigningBehavior": oac.signing_behavior,
            "OriginAccessControlOriginType": oac.origin_access_control_origin_type,
        },
        "LastModifiedTime": oac.created_at,
    })
}

pub fn create_origin_access_control(
    state: &CloudFrontState,
    input: &Value,
) -> Result<Value, AwsError> {
    let cfg = input
        .get("OriginAccessControlConfig")
        .unwrap_or(input);

    let name = cfg
        .get("Name")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();
    let description = cfg
        .get("Description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let signing_protocol = cfg
        .get("SigningProtocol")
        .and_then(|v| v.as_str())
        .unwrap_or("sigv4")
        .to_string();
    let signing_behavior = cfg
        .get("SigningBehavior")
        .and_then(|v| v.as_str())
        .unwrap_or("always")
        .to_string();
    let origin_type = cfg
        .get("OriginAccessControlOriginType")
        .and_then(|v| v.as_str())
        .unwrap_or("s3")
        .to_string();

    let id = new_oac_id();

    let oac = OriginAccessControl {
        id: id.clone(),
        name,
        description,
        signing_protocol,
        signing_behavior,
        origin_access_control_origin_type: origin_type,
        created_at: now_iso8601(),
    };

    let result = oac_to_value(&oac);
    state.origin_access_controls.insert(id.clone(), oac);

    Ok(json!({
        "OriginAccessControl": result,
        "Location": format!("https://cloudfront.amazonaws.com/2020-05-31/origin-access-control/{id}"),
        "ETag": id,
    }))
}

pub fn list_origin_access_controls(state: &CloudFrontState) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .origin_access_controls
        .iter()
        .map(|e| oac_to_value(e.value()))
        .collect();

    let qty = items.len();

    Ok(json!({
        "OriginAccessControlList": {
            "Marker": "",
            "MaxItems": 100,
            "IsTruncated": false,
            "Quantity": qty,
            "Items": { "OriginAccessControlSummary": items }
        }
    }))
}

pub fn delete_origin_access_control(state: &CloudFrontState, id: &str) -> Result<Value, AwsError> {
    if state.origin_access_controls.remove(id).is_none() {
        return Err(not_found(id));
    }

    Ok(json!({}))
}
