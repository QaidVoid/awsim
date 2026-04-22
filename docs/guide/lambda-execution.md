# Lambda Execution

AWSim actually executes Lambda function code. It extracts the deployment package, writes a bootstrap wrapper, and runs it as a local process using the system's Node.js or Python interpreter.

## Supported Runtimes

| Runtime | Prefix |
|---------|--------|
| Node.js | `nodejs*` (e.g., `nodejs18.x`, `nodejs20.x`) |
| Python | `python*` (e.g., `python3.11`, `python3.12`) |

Any runtime starting with `nodejs` invokes `node`. Any runtime starting with `python` invokes `python3`. Other runtimes return an `UnsupportedRuntime` error.

## How It Works

1. **CreateFunction** — AWSim stores the base64-encoded ZIP from `ZipFile` (or fetches from S3 if `S3Bucket`/`S3Key` are provided).
2. **Invoke** — AWSim extracts the ZIP to a temp directory, generates a thin bootstrap script, and runs the handler as a subprocess.
3. **Bootstrap** — the wrapper script loads the handler module, passes the event JSON and a context object, and captures stdout as the return value.
4. **Result** — stdout is parsed as the function response. Non-zero exit codes or stderr output is returned as a function error.

## Handler Format

The handler is specified as `module.function`:

```
index.handler     → loads ./index.js, calls exports.handler
src/app.process   → loads ./src/app.js, calls exports.process
```

For Python: `app.handler` → loads `app.py`, calls `handler(event, context)`.

## Context Object

Your function receives a `context` object with:

```javascript
{
  functionName: "my-function",
  functionVersion: "$LATEST",
  invokedFunctionArn: "arn:aws:lambda:...",
  memoryLimitInMB: "128",
  awsRequestId: "local",
  logGroupName: "/aws/lambda/my-function",
  logStreamName: "local",
  getRemainingTimeInMillis: () => <timeout_ms>,
}
```

## Environment Variables

Your function receives the environment variables you configured on the function, plus:

- `AWS_LAMBDA_FUNCTION_NAME`
- `AWS_LAMBDA_FUNCTION_MEMORY_SIZE`
- `AWS_REGION`
- `AWS_DEFAULT_REGION`

## Deploying a Function

```bash
# Package your code
zip function.zip index.js

# Create the function
aws --endpoint-url http://localhost:4566 lambda create-function \
  --function-name my-function \
  --runtime nodejs20.x \
  --handler index.handler \
  --role arn:aws:iam::000000000000:role/lambda-role \
  --zip-file fileb://function.zip

# Invoke it
aws --endpoint-url http://localhost:4566 lambda invoke \
  --function-name my-function \
  --payload '{"key":"value"}' \
  output.json

cat output.json
```

## Example: Node.js Handler

```javascript
// index.js
exports.handler = async (event, context) => {
  console.log("Event:", JSON.stringify(event));
  return {
    statusCode: 200,
    body: JSON.stringify({ message: "hello from AWSim Lambda" }),
  };
};
```

## Example: Python Handler

```python
# handler.py
import json

def handler(event, context):
    print("Event:", json.dumps(event))
    return {
        "statusCode": 200,
        "body": json.dumps({"message": "hello from AWSim Lambda"}),
    }
```

## Known Limitations

- **No container/Docker runtime** — functions run directly as child processes. There is no isolation between functions or from the host filesystem.
- **No Lambda layers execution** — layers can be created and listed, but their content is not merged into the function runtime environment.
- **No VPC networking** — VPC configuration is accepted but ignored.
- **Timeout enforcement** — the timeout is passed to the context object but is not strictly enforced for long-running functions.
- **Node.js require only** — the bootstrap uses CommonJS `require()`. ESM (`import`) works only if your runtime supports it natively.
