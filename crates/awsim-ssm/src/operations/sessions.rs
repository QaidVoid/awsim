use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{SsmSession, SsmState};

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn start_session(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let target = input["Target"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Target is required"))?
        .to_string();

    let document_name = input["DocumentName"]
        .as_str()
        .unwrap_or("SSM-SessionManagerRunShell")
        .to_string();

    let session_id = format!(
        "session-{}",
        Uuid::new_v4().to_string().replace('-', "")[..16].to_string()
    );
    let now = now_epoch_secs();

    let session = SsmSession {
        session_id: session_id.clone(),
        target,
        status: "Connected".to_string(),
        document_name,
        start_date: now,
        end_date: None,
        owner: "awsim-user".to_string(),
    };

    state.sessions.insert(session_id.clone(), session);

    Ok(json!({
        "SessionId": session_id,
        "TokenValue": "awsim-stub-token",
        "StreamUrl": format!("wss://localhost/stream/{session_id}"),
    }))
}

pub fn describe_sessions(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let max_results = input["MaxResults"].as_u64().unwrap_or(50) as usize;
    let state_filter = input["State"].as_str().unwrap_or("Active");

    let sessions: Vec<Value> = state
        .sessions
        .iter()
        .filter(|e| {
            let s = e.value();
            match state_filter {
                "Active" => s.end_date.is_none(),
                "History" => s.end_date.is_some(),
                _ => true,
            }
        })
        .map(|e| {
            let s = e.value();
            json!({
                "SessionId": s.session_id,
                "Target": s.target,
                "Status": s.status,
                "StartDate": s.start_date,
                "EndDate": s.end_date,
                "DocumentName": s.document_name,
                "Owner": s.owner,
            })
        })
        .take(max_results)
        .collect();

    Ok(json!({ "Sessions": sessions }))
}

pub fn terminate_session(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let session_id = input["SessionId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "SessionId is required"))?;

    let mut session = state.sessions.get_mut(session_id).ok_or_else(|| {
        AwsError::not_found(
            "DoesNotExistException",
            format!("Session '{session_id}' does not exist"),
        )
    })?;

    session.status = "Terminated".to_string();
    session.end_date = Some(now_epoch_secs());

    Ok(json!({ "SessionId": session_id }))
}

pub fn resume_session(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let session_id = input["SessionId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "SessionId is required"))?;

    let _ = state.sessions.get(session_id).ok_or_else(|| {
        AwsError::not_found(
            "DoesNotExistException",
            format!("Session '{session_id}' does not exist"),
        )
    })?;

    Ok(json!({
        "SessionId": session_id,
        "TokenValue": "awsim-stub-token",
        "StreamUrl": format!("wss://localhost/stream/{session_id}"),
    }))
}
