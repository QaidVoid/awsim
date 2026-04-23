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

## Quick Start

Invoke a foundation model with a prompt:

```bash
# List available foundation models
curl -s http://localhost:4566/foundation-models \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/bedrock/aws4_request, SignedHeaders=host, Signature=fake"

# Invoke Claude via Bedrock Runtime (response goes to output.json)
curl -s -X POST "http://localhost:4566/model/anthropic.claude-3-haiku-20240307-v1:0/invoke" \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/bedrock-runtime/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"anthropic_version":"bedrock-2023-05-31","max_tokens":256,"messages":[{"role":"user","content":"What is AWS?"}]}'
```

## Operations

### Management API (`bedrock`)

#### Foundation Models
- `ListFoundationModels` — list all available foundation models
  - Path: `GET /foundation-models`
  - Returns: `modelSummaries` list with `modelId`, `modelName`, `providerName`, `inputModalities`, `outputModalities`
  - No input required; returns a built-in set of popular model IDs

- `GetFoundationModel` — get details of a specific foundation model
  - Path: `GET /foundation-models/{modelIdentifier}`
  - Returns: full `modelDetails` including `modelId`, `modelArn`, supported inference parameters

#### Model Customization
- `CreateModelCustomizationJob` — create a fine-tuning job for a foundation model
  - Input: `jobName`, `customModelName`, `baseModelIdentifier`, `trainingDataConfig`, `outputDataConfig`
  - Returns: `jobArn`

- `ListModelCustomizationJobs` — list all model customization jobs
  - Returns: paginated `modelCustomizationJobSummaries`

#### Guardrails
- `CreateGuardrail` — create a content safety guardrail
  - Input: `name`, `contentPolicyConfig` (blocked topics), `wordPolicyConfig` (blocked words), `sensitiveInformationPolicyConfig`
  - Returns: `guardrailId`, `guardrailArn`

- `GetGuardrail` — get details of a guardrail
  - Input: `guardrailIdentifier`, optional `guardrailVersion`

- `ListGuardrails` — list all guardrails
- `DeleteGuardrail` — delete a guardrail

### Runtime API (`bedrock-runtime`)

- `InvokeModel` — invoke a foundation model with a prompt and get a completion
  - Path: `POST /model/{modelId}/invoke`
  - Input: body format depends on model provider (Anthropic, Amazon Titan, Meta Llama, etc.)
  - Returns: model-specific JSON response; for Anthropic Claude it includes `content[].text`

- `Converse` — send a multi-turn conversation using the unified Converse API
  - Path: `POST /model/{modelId}/converse`
  - Input: `messages` list with `role` and `content`, optional `system`, `inferenceConfig`
  - Returns: `output.message` with assistant response, `usage` (token counts), `stopReason`

## Curl Examples

```bash
# 1. List foundation models
curl -s http://localhost:4566/foundation-models \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/bedrock/aws4_request, SignedHeaders=host, Signature=fake" \
  | jq '.modelSummaries[].modelId'

# 2. Invoke Anthropic Claude (Messages API format)
curl -s -X POST "http://localhost:4566/model/anthropic.claude-3-sonnet-20240229-v1:0/invoke" \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/bedrock-runtime/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"anthropic_version":"bedrock-2023-05-31","max_tokens":512,"messages":[{"role":"user","content":"Explain S3 in one sentence."}]}'

# 3. Use the Converse API (unified across all models)
curl -s -X POST "http://localhost:4566/model/amazon.titan-text-express-v1/converse" \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/bedrock-runtime/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"messages":[{"role":"user","content":[{"type":"text","text":"Hello!"}]}],"inferenceConfig":{"maxTokens":100}}'
```

## SDK Example

```typescript
import { BedrockClient, ListFoundationModelsCommand } from '@aws-sdk/client-bedrock';
import { BedrockRuntimeClient, InvokeModelCommand, ConverseCommand } from '@aws-sdk/client-bedrock-runtime';

const bedrock = new BedrockClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

const runtime = new BedrockRuntimeClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// List available models
const { modelSummaries } = await bedrock.send(new ListFoundationModelsCommand({}));
console.log('Models:', modelSummaries?.map(m => m.modelId));

// Invoke via Converse API (works with any model ID)
const response = await runtime.send(new ConverseCommand({
  modelId: 'anthropic.claude-3-haiku-20240307-v1:0',
  messages: [{ role: 'user', content: [{ type: 'text' as const, text: 'What is 2+2?' }] }],
  inferenceConfig: { maxTokens: 100 },
}));

const text = response.output?.message?.content?.[0];
if (text && 'text' in text) {
  console.log('Response:', text.text);
}

// Invoke directly (model-specific request format)
const body = JSON.stringify({
  anthropic_version: 'bedrock-2023-05-31',
  max_tokens: 256,
  messages: [{ role: 'user', content: 'Hello!' }],
});

const { body: responseBody } = await runtime.send(new InvokeModelCommand({
  modelId: 'anthropic.claude-3-sonnet-20240229-v1:0',
  body: Buffer.from(body),
  contentType: 'application/json',
}));

const result = JSON.parse(Buffer.from(responseBody).toString());
console.log(result.content[0].text);
```

## Behavior Notes

- `InvokeModel` returns a mock response in the expected format for the requested model — no real LLM inference occurs.
- `Converse` returns a plausible mock message structure so SDKs parse the response correctly.
- The management API returns a built-in list of popular foundation model IDs (Claude, Titan, Llama, Mistral, Cohere, etc.) so `ListFoundationModels` works without any setup.
- Guardrails and customization jobs are recorded but have no effect on inference.
- The mock response text changes based on the query but is generated locally without any neural network.
