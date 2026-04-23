use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use base64::Engine;
use serde_json::{Value, json};

use crate::state::{PollyState, SpeechSynthesisTask};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn synthesize_speech(
    _state: &PollyState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let _text = input["Text"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidSampleRateException", "Text is required"))?;

    let format = input["OutputFormat"].as_str().unwrap_or("mp3");
    let content_type = match format {
        "mp3" => "audio/mpeg",
        "ogg_vorbis" => "audio/ogg",
        "pcm" => "audio/pcm",
        "json" => "application/x-json-stream",
        _ => "audio/mpeg",
    };

    let dummy_audio: &[u8] = b"\x00\x00\x00\x00";
    let encoded = base64::engine::general_purpose::STANDARD.encode(dummy_audio);

    Ok(json!({
        "__raw_body": encoded,
        "__content_type": content_type,
    }))
}

pub fn start_speech_synthesis_task(
    state: &PollyState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let text = input["Text"].as_str().unwrap_or("").to_string();
    let voice_id = input["VoiceId"].as_str().unwrap_or("Joanna").to_string();
    let format = input["OutputFormat"].as_str().unwrap_or("mp3").to_string();
    let bucket = input["OutputS3BucketName"].as_str().unwrap_or("");

    let task_id = uuid::Uuid::new_v4().to_string();
    let output_uri = format!("https://s3.{}.amazonaws.com/{}/{}.{}", ctx.region, bucket, task_id, format);

    let task = SpeechSynthesisTask {
        task_id: task_id.clone(),
        status: "completed".to_string(),
        output_uri: output_uri.clone(),
        text,
        voice_id: voice_id.clone(),
        output_format: format.clone(),
        created_at: now_secs(),
    };

    state.tasks.insert(task_id.clone(), task);

    Ok(json!({
        "SynthesisTask": {
            "TaskId": task_id,
            "TaskStatus": "completed",
            "OutputUri": output_uri,
            "CreationTime": 0,
            "VoiceId": voice_id,
            "OutputFormat": format,
        }
    }))
}

pub fn get_speech_synthesis_task(
    state: &PollyState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let task_id = input["TaskId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidTaskIdException", "TaskId is required"))?;

    let t = state.tasks.get(task_id).ok_or_else(|| {
        AwsError::not_found("SynthesisTaskNotFoundException", format!("Task not found: {task_id}"))
    })?;

    Ok(json!({
        "SynthesisTask": {
            "TaskId": t.task_id,
            "TaskStatus": t.status,
            "OutputUri": t.output_uri,
            "CreationTime": t.created_at,
            "VoiceId": t.voice_id,
            "OutputFormat": t.output_format,
        }
    }))
}

pub fn list_speech_synthesis_tasks(
    state: &PollyState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list: Vec<Value> = state
        .tasks
        .iter()
        .map(|e| {
            let t = e.value();
            json!({
                "TaskId": t.task_id,
                "TaskStatus": t.status,
                "OutputUri": t.output_uri,
                "CreationTime": t.created_at,
                "VoiceId": t.voice_id,
                "OutputFormat": t.output_format,
            })
        })
        .collect();

    Ok(json!({ "SynthesisTasks": list }))
}
