use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::CloudFrontState;

/// GET /2020-05-31/response-headers-policy
pub fn list_response_headers_policies(
    _state: &CloudFrontState,
) -> Result<Value, AwsError> {
    Ok(json!({
        "ResponseHeadersPolicyList": {
            "MaxItems": 100,
            "Quantity": 0,
            "Items": { "ResponseHeadersPolicySummary": [] }
        }
    }))
}
