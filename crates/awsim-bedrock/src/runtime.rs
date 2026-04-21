use awsim_core::AwsError;
use serde_json::{Value, json};
use tracing::debug;

/// Produce a mock InvokeModel response based on model type.
pub fn invoke_model(input: &Value) -> Result<Value, AwsError> {
    let model_id = input["modelId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "modelId is required"))?;

    debug!(model_id = %model_id, "InvokeModel (mock)");

    if model_id.starts_with("anthropic.claude") {
        Ok(json!({
            "id": "msg_mock",
            "type": "message",
            "role": "assistant",
            "content": [
                {
                    "type": "text",
                    "text": "This is a mock response from AWSim Bedrock."
                }
            ],
            "model": model_id,
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 20
            }
        }))
    } else if model_id.starts_with("amazon.titan-embed") {
        // Embedding model: return a small dummy embedding
        Ok(json!({
            "embedding": vec![0.01f32; 256],
            "inputTextTokenCount": 5,
        }))
    } else if model_id.starts_with("stability.") {
        // Image model: return empty artifacts stub
        Ok(json!({
            "result": "success",
            "artifacts": [],
        }))
    } else {
        // Generic text model response
        Ok(json!({
            "generation": "This is a mock response from AWSim Bedrock.",
            "stop_reason": "stop",
            "model_id": model_id,
        }))
    }
}

/// Produce a mock Converse response.
pub fn converse(input: &Value) -> Result<Value, AwsError> {
    let model_id = input["modelId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "modelId is required"))?;

    debug!(model_id = %model_id, "Converse (mock)");

    Ok(json!({
        "output": {
            "message": {
                "role": "assistant",
                "content": [
                    {
                        "text": "This is a mock conversational response from AWSim Bedrock."
                    }
                ]
            }
        },
        "stopReason": "end_turn",
        "usage": {
            "inputTokens": 10,
            "outputTokens": 20,
            "totalTokens": 30
        },
        "metrics": {
            "latencyMs": 1
        }
    }))
}
