use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SesState;

// ---------------------------------------------------------------------------
// GetAccount
// ---------------------------------------------------------------------------

pub fn get_account(
    _state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({
        "DedicatedIpAutoWarmupEnabled": false,
        "EnforcementStatus": "HEALTHY",
        "ProductionAccessEnabled": true,
        "SendingEnabled": true,
        "SendQuota": {
            "Max24HourSend": 50000.0,
            "MaxSendRate": 14.0,
            "SentLast24Hours": 0.0
        },
        "SuppressionAttributes": {
            "SuppressedReasons": []
        },
        "Details": {
            "MailType": "TRANSACTIONAL",
            "WebsiteURL": "https://awsim.local",
            "UseCaseDescription": "Local development emulator"
        }
    }))
}
