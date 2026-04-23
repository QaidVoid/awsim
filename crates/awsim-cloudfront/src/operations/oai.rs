use awsim_core::AwsError;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    ids::{new_etag, now_iso8601},
    state::{CloudFrontState, OriginAccessIdentity},
};

fn not_found(id: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchCloudFrontOriginAccessIdentity",
        format!("The specified origin access identity does not exist: {id}"),
    )
}

fn oai_to_value(oai: &OriginAccessIdentity) -> Value {
    json!({
        "Id": oai.id,
        "S3CanonicalUserId": oai.s3_canonical_user_id,
        "CloudFrontOriginAccessIdentityConfig": {
            "CallerReference": oai.caller_reference,
            "Comment": oai.comment,
        }
    })
}

/// POST /2020-05-31/origin-access-identity/cloudfront
pub fn create_oai(state: &CloudFrontState, input: &Value) -> Result<Value, AwsError> {
    let config = input
        .get("CloudFrontOriginAccessIdentityConfig")
        .unwrap_or(input);

    let caller_reference = config
        .get("CallerReference")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let comment = config
        .get("Comment")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let id = Uuid::new_v4().to_string();
    let s3_canonical_user_id = format!("{id}-canonical");
    let etag = new_etag();

    let oai = OriginAccessIdentity {
        id: id.clone(),
        s3_canonical_user_id,
        comment,
        caller_reference,
        created_at: now_iso8601(),
    };

    let result = oai_to_value(&oai);
    state.oais.insert(id.clone(), oai);

    Ok(json!({
        "CloudFrontOriginAccessIdentity": result,
        "Location": format!("https://cloudfront.amazonaws.com/2020-05-31/origin-access-identity/cloudfront/{id}"),
        "ETag": etag,
    }))
}

/// GET /2020-05-31/origin-access-identity/cloudfront/{Id}
pub fn get_oai(state: &CloudFrontState, id: &str) -> Result<Value, AwsError> {
    let oai = state
        .oais
        .get(id)
        .ok_or_else(|| not_found(id))?;

    let etag = new_etag();
    Ok(json!({
        "CloudFrontOriginAccessIdentity": oai_to_value(&oai),
        "ETag": etag,
    }))
}

/// GET /2020-05-31/origin-access-identity/cloudfront
pub fn list_oais(state: &CloudFrontState) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .oais
        .iter()
        .map(|e| oai_to_value(e.value()))
        .collect();

    let qty = items.len();

    Ok(json!({
        "CloudFrontOriginAccessIdentityList": {
            "Marker": "",
            "MaxItems": 100,
            "IsTruncated": false,
            "Quantity": qty,
            "Items": { "CloudFrontOriginAccessIdentitySummary": items }
        }
    }))
}
