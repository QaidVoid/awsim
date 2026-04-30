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

/// InvokeModelWithResponseStream — returns the same mock response as InvokeModel
/// but in a single-chunk streaming envelope (we cannot truly stream in our architecture).
pub fn invoke_model_with_response_stream(input: &Value) -> Result<Value, AwsError> {
    let model_id = input["modelId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "modelId is required"))?;

    debug!(model_id = %model_id, "InvokeModelWithResponseStream (mock single-chunk)");

    // Return the same body as InvokeModel but wrapped in a streaming event envelope.
    let body = invoke_model(input)?;

    Ok(json!({
        "contentType": "application/json",
        "body": body,
    }))
}

/// ConverseStream — same as Converse but with a streaming response mock.
pub fn converse_stream(input: &Value) -> Result<Value, AwsError> {
    let model_id = input["modelId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "modelId is required"))?;

    debug!(model_id = %model_id, "ConverseStream (mock single-chunk)");

    Ok(json!({
        "stream": [
            {
                "messageStart": {
                    "role": "assistant"
                }
            },
            {
                "contentBlockDelta": {
                    "delta": {
                        "text": "This is a mock streaming response from AWSim Bedrock."
                    },
                    "contentBlockIndex": 0
                }
            },
            {
                "messageStop": {
                    "stopReason": "end_turn"
                }
            },
            {
                "metadata": {
                    "usage": {
                        "inputTokens": 10,
                        "outputTokens": 20,
                        "totalTokens": 30
                    },
                    "metrics": {
                        "latencyMs": 1
                    }
                }
            }
        ]
    }))
}
