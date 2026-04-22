# Lambda

**Protocol:** REST-JSON  
**Signing name:** `lambda`  
**Persistent:** No (function code and configuration are lost on restart)

## Implemented Operations

### Functions

| Operation | Description |
|-----------|-------------|
| `CreateFunction` | Create a function from a ZIP file or S3 object |
| `GetFunction` | Get function metadata and code location |
| `GetFunctionConfiguration` | Get function configuration |
| `UpdateFunctionCode` | Update the function's deployment package |
| `UpdateFunctionConfiguration` | Update runtime, handler, env vars, timeout |
| `DeleteFunction` | Delete a function |
| `ListFunctions` | List all functions |

### Invocation

| Operation | Description |
|-----------|-------------|
| `Invoke` | Invoke a function synchronously |

### Versions and Aliases

| Operation | Description |
|-----------|-------------|
| `PublishVersion` | Publish a new version |
| `ListVersionsByFunction` | List all versions of a function |
| `CreateAlias` | Create an alias |
| `GetAlias` | Get alias configuration |
| `DeleteAlias` | Delete an alias |
| `ListAliases` | List all aliases |

### Event Source Mappings

| Operation | Description |
|-----------|-------------|
| `CreateEventSourceMapping` | Map SQS, Kinesis, or DynamoDB Streams as a trigger |
| `GetEventSourceMapping` | Get mapping configuration |
| `DeleteEventSourceMapping` | Remove a trigger |
| `ListEventSourceMappings` | List all mappings |
| `UpdateEventSourceMapping` | Update mapping configuration |

### Layers

| Operation | Description |
|-----------|-------------|
| `PublishLayerVersion` | Publish a layer version |
| `GetLayerVersion` | Get layer version metadata |
| `ListLayers` | List layers |
| `ListLayerVersions` | List versions of a layer |

## SDK Example

```typescript
import { LambdaClient, CreateFunctionCommand, InvokeCommand } from "@aws-sdk/client-lambda";
import { readFileSync } from "fs";

const lambda = new LambdaClient({
  region: "us-east-1",
  endpoint: "http://localhost:4566",
  credentials: { accessKeyId: "test", secretAccessKey: "test" },
});

// Create function from ZIP
const zipBytes = readFileSync("function.zip");
await lambda.send(new CreateFunctionCommand({
  FunctionName: "my-function",
  Runtime: "nodejs20.x",
  Handler: "index.handler",
  Role: "arn:aws:iam::000000000000:role/lambda-role",
  Code: { ZipFile: zipBytes },
  Environment: {
    Variables: { TABLE_NAME: "my-table" },
  },
}));

// Invoke
const { Payload } = await lambda.send(new InvokeCommand({
  FunctionName: "my-function",
  Payload: JSON.stringify({ key: "value" }),
}));

const result = JSON.parse(Buffer.from(Payload!).toString());
console.log(result);
```

## CLI Example

```bash
# Package
zip function.zip index.js

# Create
aws --endpoint-url http://localhost:4566 lambda create-function \
  --function-name my-function \
  --runtime nodejs20.x \
  --handler index.handler \
  --role arn:aws:iam::000000000000:role/any \
  --zip-file fileb://function.zip

# Invoke
aws --endpoint-url http://localhost:4566 lambda invoke \
  --function-name my-function \
  --payload '{"hello":"world"}' \
  out.json && cat out.json
```

## Supported Runtimes

- `nodejs*` — any string starting with `nodejs`, e.g. `nodejs18.x`, `nodejs20.x`
- `python*` — any string starting with `python`, e.g. `python3.11`, `python3.12`

See [Lambda Execution](/guide/lambda-execution) for full runtime details and limitations.

## Known Limitations

- Function code is not persisted — you must re-create functions after an AWSim restart.
- No container/sandbox isolation. Functions share the host filesystem and network.
- Layers are stored as metadata but their contents are not merged into the function execution environment.
- Asynchronous invocation (`InvocationType: Event`) is accepted but behaves synchronously in AWSim.
