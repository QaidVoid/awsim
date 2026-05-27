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
        &Uuid::new_v4().to_string().replace('-', "")[..16]
    );
    let now = now_epoch_secs();

    let document_version = input["DocumentVersion"].as_str().map(str::to_string);
    let parameters = match &input["Parameters"] {
        Value::Null => None,
        other => Some(other.clone()),
    };
    let max_session_duration = input["MaxSessionDuration"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .or_else(|| input["MaxSessionDuration"].as_u64());
    let s3_output_url = input["OutputUrl"]["S3OutputUrl"]
        .as_str()
        .map(str::to_string);
    let cloudwatch_output_url = input["OutputUrl"]["CloudWatchOutputUrl"]
        .as_str()
        .map(str::to_string);

    let session = SsmSession {
        session_id: session_id.clone(),
        target,
        status: "Connected".to_string(),
        document_name,
        start_date: now,
        end_date: None,
        owner: "awsim-user".to_string(),
        reason: None,
        s3_output_url,
        cloudwatch_output_url,
        document_version,
        parameters,
        max_session_duration,
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
            let mut output = serde_json::Map::new();
            if let Some(ref u) = s.s3_output_url {
                output.insert("S3OutputUrl".to_string(), json!(u));
            }
            if let Some(ref u) = s.cloudwatch_output_url {
                output.insert("CloudWatchOutputUrl".to_string(), json!(u));
            }
            let mut obj = json!({
                "SessionId": s.session_id,
                "Target": s.target,
                "Status": s.status,
                "StartDate": s.start_date,
                "EndDate": s.end_date,
                "DocumentName": s.document_name,
                "Owner": s.owner,
                "OutputUrl": Value::Object(output),
            });
            if let Some(ref r) = s.reason {
                obj["Reason"] = json!(r);
            }
            if let Some(ref dv) = s.document_version {
                obj["DocumentVersion"] = json!(dv);
            }
            if let Some(ref p) = s.parameters {
                obj["Parameters"] = p.clone();
            }
            if let Some(d) = s.max_session_duration {
                obj["MaxSessionDuration"] = json!(d);
            }
            obj
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
        AwsError::bad_request(
            "DoesNotExistException",
            format!("Session '{session_id}' does not exist"),
        )
    })?;

    session.status = "Terminated".to_string();
    session.end_date = Some(now_epoch_secs());
    session.reason = input["Reason"].as_str().map(str::to_string);

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
        AwsError::bad_request(
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

#[cfg(test)]
mod session_log_field_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("ssm", "us-east-1")
    }

    #[test]
    fn start_session_persists_log_fields() {
        let state = SsmState::default();
        let resp = start_session(
            &state,
            &json!({
                "Target": "i-1234",
                "DocumentName": "MyShell",
                "DocumentVersion": "3",
                "Parameters": { "shell": ["bash"] },
                "MaxSessionDuration": "3600",
                "OutputUrl": {
                    "S3OutputUrl": "s3://logs/session/",
                    "CloudWatchOutputUrl": "arn:aws:logs:us-east-1:111:log-group:ssm",
                },
            }),
            &ctx(),
        )
        .unwrap();
        let session_id = resp["SessionId"].as_str().unwrap().to_string();
        let stored = state.sessions.get(&session_id).unwrap();
        assert_eq!(stored.document_version.as_deref(), Some("3"));
        assert_eq!(stored.s3_output_url.as_deref(), Some("s3://logs/session/"));
        assert!(stored.cloudwatch_output_url.is_some());
        assert_eq!(stored.max_session_duration, Some(3600));
        assert!(stored.parameters.is_some());
    }

    #[test]
    fn describe_sessions_surfaces_log_fields() {
        let state = SsmState::default();
        start_session(
            &state,
            &json!({
                "Target": "i-1234",
                "OutputUrl": { "S3OutputUrl": "s3://logs/" },
            }),
            &ctx(),
        )
        .unwrap();
        let resp = describe_sessions(&state, &json!({ "State": "Active" }), &ctx()).unwrap();
        let first = resp["Sessions"].as_array().unwrap().first().unwrap();
        assert_eq!(first["OutputUrl"]["S3OutputUrl"], "s3://logs/");
    }

    #[test]
    fn terminate_session_records_reason_and_moves_to_history() {
        let state = SsmState::default();
        let resp = start_session(&state, &json!({ "Target": "i-1234" }), &ctx()).unwrap();
        let session_id = resp["SessionId"].as_str().unwrap().to_string();
        terminate_session(
            &state,
            &json!({ "SessionId": session_id.clone(), "Reason": "operator-requested" }),
            &ctx(),
        )
        .unwrap();
        let resp = describe_sessions(&state, &json!({ "State": "History" }), &ctx()).unwrap();
        let first = resp["Sessions"].as_array().unwrap().first().unwrap();
        assert_eq!(first["Reason"], "operator-requested");
        assert_eq!(first["Status"], "Terminated");
        assert!(first["EndDate"].as_u64().is_some());
    }
}
