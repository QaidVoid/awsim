use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SesState;

// ---------------------------------------------------------------------------
// GetAccount
// ---------------------------------------------------------------------------

pub fn get_account(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let suppression = state
        .account_suppression_attributes
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| json!({ "SuppressedReasons": [] }));
    let mut response = json!({
        "DedicatedIpAutoWarmupEnabled": false,
        "EnforcementStatus": "HEALTHY",
        "ProductionAccessEnabled": true,
        "SendingEnabled": true,
        "SendQuota": {
            "Max24HourSend": 50000.0,
            "MaxSendRate": 14.0,
            "SentLast24Hours": 0.0
        },
        "SuppressionAttributes": suppression,
        "Details": {
            "MailType": "TRANSACTIONAL",
            "WebsiteURL": "https://awsim.local",
            "UseCaseDescription": "Local development emulator"
        }
    });
    if let Some(vdm) = state.account_vdm_attributes.lock().unwrap().clone() {
        response["VdmAttributes"] = vdm;
    }
    Ok(response)
}
