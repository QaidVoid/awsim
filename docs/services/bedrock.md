# Bedrock

Amazon Bedrock managed foundation model service for building generative AI applications.

AWSim provides **two handlers** for Bedrock: the management API and the runtime API.

## Configuration

### Management API

| Property | Value |
|----------|-------|
| Protocol | `RestJson1` |
| Signing Name | `bedrock` |
| Persistence | No |

### Runtime API

| Property | Value |
|----------|-------|
| Protocol | `RestJson1` |
| Signing Name | `bedrock-runtime` |
| Persistence | No |

## Operations

### Management API (`bedrock`)

#### Foundation Models
- `ListFoundationModels` — list all available foundation models
- `GetFoundationModel` — get details of a specific foundation model by ID

#### Model Customization
- `CreateModelCustomizationJob` — create a fine-tuning job for a foundation model
- `ListModelCustomizationJobs` — list all model customization jobs

#### Guardrails
- `CreateGuardrail` — create a content safety guardrail
- `GetGuardrail` — get details of a guardrail
- `ListGuardrails` — list all guardrails
- `DeleteGuardrail` — delete a guardrail

### Runtime API (`bedrock-runtime`)

- `InvokeModel` — invoke a foundation model with a prompt and get a completion
- `Converse` — send a multi-turn conversation to a model using the Converse API

## Example

```bash
# List available foundation models
aws --endpoint-url http://localhost:4567 \
  bedrock list-foundation-models

# Invoke a model (Bedrock Runtime)
aws --endpoint-url http://localhost:4567 \
  bedrock-runtime invoke-model \
  --model-id anthropic.claude-3-sonnet-20240229-v1:0 \
  --body '{"anthropic_version":"bedrock-2023-05-31","max_tokens":256,"messages":[{"role":"user","content":"Hello!"}]}' \
  --cli-binary-format raw-in-base64-out \
  output.json

# Use the Converse API
aws --endpoint-url http://localhost:4567 \
  bedrock-runtime converse \
  --model-id anthropic.claude-3-haiku-20240307-v1:0 \
  --messages '[{"role":"user","content":[{"type":"text","text":"What is 2+2?"}]}]'
```

## Notes

- The runtime `InvokeModel` returns a mock response in the expected format for the requested model. Responses are synthesized locally — no real LLM inference occurs.
- `Converse` returns a mock message with a plausible structure so SDKs parse correctly.
- The management API returns a built-in list of popular foundation model IDs (Claude, Titan, Llama, Mistral, etc.) so `ListFoundationModels` works without any setup.
- Guardrails and customization jobs are recorded but have no effect on inference.
