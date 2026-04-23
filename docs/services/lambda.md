# Lambda

AWS Lambda serverless compute for running code without provisioning or managing servers.

**Protocol:** `RestJson1`
**Signing name:** `lambda`
**Persistent:** No (function code and configuration are lost on restart)

## Quick Start

Package a function, create it, and invoke it:

```bash
# 1. Create a minimal Node.js handler file
cat > index.js << 'EOF'
exports.handler = async (event) => {
  console.log('Event:', JSON.stringify(event));
  return { statusCode: 200, body: JSON.stringify({ message: 'Hello!', input: event }) };
};
EOF

# 2. Package it
zip function.zip index.js

# 3. Create the function
aws --endpoint-url http://localhost:4566 lambda create-function \
  --function-name hello-world \
  --runtime nodejs20.x \
  --handler index.handler \
  --role arn:aws:iam::000000000000:role/any-role \
  --zip-file fileb://function.zip

# 4. Invoke it
aws --endpoint-url http://localhost:4566 lambda invoke \
  --function-name hello-world \
  --payload '{"name":"Alice"}' \
  response.json && cat response.json
```

## Operations

### Functions

| Operation | Description |
|-----------|-------------|
| `CreateFunction` | Create a function from a ZIP file or S3 object. Input: `FunctionName`, `Runtime`, `Handler` (e.g., `index.handler`), `Role` (IAM ARN), `Code` (`{ZipFile}` or `{S3Bucket, S3Key}`), `Environment` (`{Variables: {KEY: VALUE}}`), `Timeout` (seconds, default 3), `MemorySize` (MB) |
| `GetFunction` | Get function metadata and code location. Returns `Configuration` and `Code.Location` |
| `GetFunctionConfiguration` | Get just the configuration (runtime, handler, env vars, etc.) |
| `UpdateFunctionCode` | Update the deployment package. Input: `FunctionName`, `ZipFile` or `S3Bucket/S3Key` |
| `UpdateFunctionConfiguration` | Update runtime, handler, env vars, timeout, memory. Input: `FunctionName` plus fields to update |
| `DeleteFunction` | Delete a function. Input: `FunctionName`, optional `Qualifier` (version or alias) |
| `ListFunctions` | List all functions. Returns paginated `Functions` list |

### Invocation

| Operation | Description |
|-----------|-------------|
| `Invoke` | Invoke a function synchronously. Input: `FunctionName`, `Payload` (JSON body), `InvocationType` (`RequestResponse`, `Event`, `DryRun`). Returns: `StatusCode`, `Payload` (function response), `FunctionError` (if function threw), `LogResult` (base64 tail of logs if `LogType=Tail`) |

### Versions and Aliases

| Operation | Description |
|-----------|-------------|
| `PublishVersion` | Publish the current `$LATEST` code as a numbered version. Returns: `Version` (number string) |
| `ListVersionsByFunction` | List all published versions. Returns: `Versions` list |
| `CreateAlias` | Create an alias (e.g., `prod`) pointing to a version. Input: `FunctionName`, `Name`, `FunctionVersion` |
| `GetAlias` | Get alias configuration |
| `DeleteAlias` | Delete an alias |
| `ListAliases` | List all aliases for a function |

### Event Source Mappings

| Operation | Description |
|-----------|-------------|
| `CreateEventSourceMapping` | Map SQS, Kinesis, or DynamoDB Streams as a trigger. Input: `FunctionName`, `EventSourceArn`, `BatchSize`, `StartingPosition` (`TRIM_HORIZON` or `LATEST` for Kinesis) |
| `GetEventSourceMapping` | Get mapping configuration |
| `DeleteEventSourceMapping` | Remove a trigger |
| `ListEventSourceMappings` | List all mappings |
| `UpdateEventSourceMapping` | Update batch size or enable/disable the mapping |

### Layers

| Operation | Description |
|-----------|-------------|
| `PublishLayerVersion` | Publish a layer version. Input: `LayerName`, `Content` (`{ZipFile}`), `CompatibleRuntimes` |
| `GetLayerVersion` | Get layer version metadata |
| `ListLayers` | List layers |
| `ListLayerVersions` | List versions of a layer |

## Curl Examples

```bash
# 1. Create a function using base64-encoded ZIP
ZIP_B64=$(base64 -w 0 function.zip)
curl -s -X POST http://localhost:4566/2015-03-31/functions \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/lambda/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"FunctionName\":\"my-fn\",\"Runtime\":\"nodejs20.x\",\"Handler\":\"index.handler\",\"Role\":\"arn:aws:iam::000000000000:role/any\",\"Code\":{\"ZipFile\":\"$ZIP_B64\"}}"

# 2. Invoke a function
curl -s -X POST http://localhost:4566/2015-03-31/functions/my-fn/invocations \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/lambda/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"key":"value","nested":{"items":[1,2,3]}}'

# 3. List all functions
curl -s http://localhost:4566/2015-03-31/functions \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/lambda/aws4_request, SignedHeaders=host, Signature=fake"
```

## SDK Example

```typescript
import { LambdaClient, CreateFunctionCommand, InvokeCommand, UpdateFunctionCodeCommand } from '@aws-sdk/client-lambda';
import { readFileSync } from 'fs';

const lambda = new LambdaClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create function from ZIP
const zipBytes = readFileSync('function.zip');
await lambda.send(new CreateFunctionCommand({
  FunctionName: 'my-function',
  Runtime: 'nodejs20.x',
  Handler: 'index.handler',
  Role: 'arn:aws:iam::000000000000:role/lambda-role',
  Code: { ZipFile: zipBytes },
  Environment: {
    Variables: {
      TABLE_NAME: 'my-table',
      STAGE: 'staging',
    },
  },
  Timeout: 30,
  MemorySize: 256,
}));

// Invoke
const { Payload, StatusCode, FunctionError } = await lambda.send(new InvokeCommand({
  FunctionName: 'my-function',
  Payload: JSON.stringify({ userId: '123', action: 'getProfile' }),
  LogType: 'Tail',
}));

if (FunctionError) {
  console.error('Function error:', FunctionError);
}

const result = JSON.parse(Buffer.from(Payload!).toString());
console.log('Status:', StatusCode); // 200
console.log('Result:', result);

// Update code after changes
const newZip = readFileSync('function-v2.zip');
await lambda.send(new UpdateFunctionCodeCommand({
  FunctionName: 'my-function',
  ZipFile: newZip,
}));
```

## Supported Runtimes

- `nodejs*` — any string starting with `nodejs`, e.g. `nodejs18.x`, `nodejs20.x`, `nodejs22.x`
- `python*` — any string starting with `python`, e.g. `python3.11`, `python3.12`, `python3.13`

See [Lambda Execution](/guide/lambda-execution) for full runtime details and limitations.

## Behavior Notes

- Lambda **actually executes** Node.js and Python code as local processes on the AWSim host machine.
- Functions share the host filesystem and network — no container/sandbox isolation. A function can read `/etc/passwd` or make outbound network calls.
- `Invoke` always behaves synchronously even if `InvocationType: "Event"` is specified.
- Function output (stdout/stderr) is captured and written to CloudWatch Logs at `/aws/lambda/{function-name}`.
- Layers are stored as metadata but their contents are **not** merged into the function's execution environment.
- Function code and configuration are **not** persisted — you must re-create functions after an AWSim restart.
- Environment variables are injected as real OS environment variables during execution.
- SQS event source mappings are actively polled every 2 seconds — messages received on the queue trigger the Lambda function automatically.
