# Step Functions

AWS Step Functions serverless orchestration service for coordinating distributed application components using state machines.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_0` |
| Signing Name | `states` |
| Target Prefix | `AWSStepFunctions` |
| Persistence | No |

## Quick Start

Create a state machine and start an execution:

```bash
# Create a simple Pass state machine
SM_ARN=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.0" \
  -H "X-Amz-Target: AWSStepFunctions.CreateStateMachine" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/states/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"name":"my-workflow","definition":"{\"Comment\":\"Example workflow\",\"StartAt\":\"Hello\",\"States\":{\"Hello\":{\"Type\":\"Pass\",\"Result\":\"Hello World\",\"End\":true}}}","roleArn":"arn:aws:iam::000000000000:role/StepFunctionsRole"}' \
  | jq -r '.stateMachineArn')

echo "State Machine ARN: $SM_ARN"

# Start an execution
EXEC_ARN=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.0" \
  -H "X-Amz-Target: AWSStepFunctions.StartExecution" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/states/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"stateMachineArn\":\"$SM_ARN\",\"name\":\"exec-001\",\"input\":\"{\\\"userId\\\":\\\"123\\\"}\"}" \
  | jq -r '.executionArn')

# Check execution status
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.0" \
  -H "X-Amz-Target: AWSStepFunctions.DescribeExecution" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/states/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"executionArn\":\"$EXEC_ARN\"}"
```

## Operations

### State Machines
- `CreateStateMachine` — create a state machine from an Amazon States Language (ASL) definition
  - Input: `name` (required), `definition` (JSON string of ASL definition), `roleArn` (IAM role ARN), optional `type` (`STANDARD` or `EXPRESS`), `loggingConfiguration`, `tracingConfiguration`, `tags`
  - Returns: `stateMachineArn` (e.g., `arn:aws:states:us-east-1:000000000000:stateMachine:my-workflow`), `creationDate`

- `DeleteStateMachine` — delete a state machine
  - Input: `stateMachineArn`

- `DescribeStateMachine` — get the definition and metadata of a state machine
  - Input: `stateMachineArn`
  - Returns: `name`, `definition` (the ASL JSON string), `roleArn`, `status` (`ACTIVE`), `type`, `creationDate`

- `ListStateMachines` — list all state machines in the account/region
  - Input: optional `maxResults`, `nextToken`
  - Returns: paginated `stateMachines` list with `name`, `stateMachineArn`, `type`, `creationDate`

- `UpdateStateMachine` — update the definition or role ARN of a state machine
  - Input: `stateMachineArn`, optional `definition`, `roleArn`, `loggingConfiguration`

### Executions
- `StartExecution` — start a new execution of a state machine
  - Input: `stateMachineArn`, optional `name` (must be unique per state machine), `input` (JSON string, passed as the first state's input)
  - Returns: `executionArn`, `startDate`

- `StopExecution` — stop a running execution
  - Input: `executionArn`, optional `error` (error code), `cause` (human-readable message)

- `DescribeExecution` — get the status and output of an execution
  - Input: `executionArn`
  - Returns: `executionArn`, `stateMachineArn`, `name`, `status` (`RUNNING`, `SUCCEEDED`, `FAILED`, `TIMED_OUT`, `ABORTED`), `startDate`, `stopDate`, `input`, `output` (JSON string of final output)

- `ListExecutions` — list executions with optional filters
  - Input: `stateMachineArn`, optional `statusFilter` (`RUNNING`, `SUCCEEDED`, `FAILED`, `TIMED_OUT`, `ABORTED`), `maxResults`, `nextToken`
  - Returns: paginated `executions` list

- `GetExecutionHistory` — retrieve the event history of an execution
  - Input: `executionArn`, optional `maxResults`, `reverseOrder`, `nextToken`
  - Returns: paginated `events` list with `timestamp`, `type` (e.g., `ExecutionStarted`, `StateEntered`, `StateExited`, `ExecutionSucceeded`), event-specific details

- `DescribeStateMachineForExecution` — given an execution ARN, return the state machine definition that was used
  - Input: `executionArn`
  - Returns: same shape as `DescribeStateMachine`

### Tags

- `TagResource` — add or update tags on a state machine or activity
  - Input: `resourceArn`, `tags` (list of `{key, value}`)

- `UntagResource` — remove tags from a state machine or activity
  - Input: `resourceArn`, `tagKeys` (list of strings)

- `ListTagsForResource` — list tags on a state machine or activity
  - Input: `resourceArn`
  - Returns: `tags` list of `{key, value}`

### Activities

- `CreateActivity` — create an activity (idempotent — returns existing ARN if already present)
  - Input: `name` (required), optional `tags` list
  - Returns: `activityArn`, `creationDate`

- `DeleteActivity` — delete an activity
  - Input: `activityArn`

- `DescribeActivity` — get activity details
  - Input: `activityArn`
  - Returns: `activityArn`, `name`, `creationDate`

- `ListActivities` — list all activities sorted by name
  - Input: optional `maxResults`, `nextToken`
  - Returns: `activities` list

### Task Token Callbacks

For `Task` states using `.waitForTaskToken`, these endpoints accept the callback and succeed silently (useful for dev/testing workflows without implementing the worker side):

- `SendTaskSuccess` — mark a task token as succeeded. Input: `taskToken` (required), `output` (JSON string, required)
- `SendTaskFailure` — mark a task token as failed. Input: `taskToken` (required), optional `error`, `cause`
- `SendTaskHeartbeat` — send a heartbeat for a running task. Input: `taskToken` (required)

## Curl Examples

```bash
# 1. Create a Lambda-invoking state machine
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.0" \
  -H "X-Amz-Target: AWSStepFunctions.CreateStateMachine" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/states/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{
    "name": "process-order",
    "definition": "{\"Comment\":\"Order processing\",\"StartAt\":\"ValidateOrder\",\"States\":{\"ValidateOrder\":{\"Type\":\"Task\",\"Resource\":\"arn:aws:lambda:us-east-1:000000000000:function:validate-order\",\"Next\":\"SendConfirmation\"},\"SendConfirmation\":{\"Type\":\"Task\",\"Resource\":\"arn:aws:lambda:us-east-1:000000000000:function:send-confirmation\",\"End\":true}}}",
    "roleArn": "arn:aws:iam::000000000000:role/StepFunctionsRole"
  }'

# 2. List all executions for a state machine
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.0" \
  -H "X-Amz-Target: AWSStepFunctions.ListExecutions" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/states/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"stateMachineArn":"arn:aws:states:us-east-1:000000000000:stateMachine:my-workflow","statusFilter":"SUCCEEDED"}'

# 3. Get execution history (ordered events)
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.0" \
  -H "X-Amz-Target: AWSStepFunctions.GetExecutionHistory" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/states/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"executionArn":"arn:aws:states:us-east-1:000000000000:execution:my-workflow:exec-001"}'
```

## SDK Example

```typescript
import {
  SFNClient,
  CreateStateMachineCommand,
  StartExecutionCommand,
  DescribeExecutionCommand,
  GetExecutionHistoryCommand,
  ListStateMachinesCommand,
} from '@aws-sdk/client-sfn';

const sfn = new SFNClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create a state machine with Choice and Task states
const definition = {
  Comment: 'Order processing workflow',
  StartAt: 'CheckAmount',
  States: {
    CheckAmount: {
      Type: 'Choice',
      Choices: [{
        Variable: '$.amount',
        NumericGreaterThan: 1000,
        Next: 'RequireApproval',
      }],
      Default: 'ProcessPayment',
    },
    RequireApproval: {
      Type: 'Task',
      Resource: 'arn:aws:lambda:us-east-1:000000000000:function:request-approval',
      Next: 'ProcessPayment',
    },
    ProcessPayment: {
      Type: 'Task',
      Resource: 'arn:aws:lambda:us-east-1:000000000000:function:process-payment',
      End: true,
    },
  },
};

const { stateMachineArn } = await sfn.send(new CreateStateMachineCommand({
  name: 'order-workflow',
  definition: JSON.stringify(definition),
  roleArn: 'arn:aws:iam::000000000000:role/StepFunctionsRole',
  type: 'STANDARD',
}));

console.log('State Machine ARN:', stateMachineArn);

// Start execution
const { executionArn } = await sfn.send(new StartExecutionCommand({
  stateMachineArn,
  name: `exec-${Date.now()}`,
  input: JSON.stringify({ orderId: 'ord-789', amount: 500, userId: 'usr-123' }),
}));

// Poll for completion (in a real app, use EventBridge or polling)
let status = 'RUNNING';
while (status === 'RUNNING') {
  const { status: s, output } = await sfn.send(new DescribeExecutionCommand({ executionArn }));
  status = s!;

  if (status === 'SUCCEEDED') {
    console.log('Execution succeeded:', JSON.parse(output!));
  } else if (status === 'FAILED') {
    console.error('Execution failed');
  }
}

// Get execution history
const { events } = await sfn.send(new GetExecutionHistoryCommand({
  executionArn,
  reverseOrder: false,
}));

events?.forEach(event => {
  console.log(`[${event.type}] ${JSON.stringify(event.stateEnteredEventDetails || event.stateExitedEventDetails || '')}`);
});
```

## Supported ASL State Types

| State Type | Support |
|------------|---------|
| `Pass` | Full — passes input/result through, useful for testing |
| `Task` | Lambda ARNs invoke the Lambda function via AWSim's Lambda service |
| `Choice` | Supported — evaluates conditions and branches |
| `Wait` | Accepted — waits are simulated (not real-time delays) |
| `Succeed` | Full — terminates execution successfully |
| `Fail` | Full — terminates execution with error and cause |
| `Parallel` | Limited — branches are registered but may not run concurrently |
| `Map` | Limited — iteration is supported for simple cases |

## Behavior Notes

- AWSim includes a basic ASL interpreter that handles common state types. `Task` states with Lambda ARNs will attempt to invoke the Lambda function if it's registered in AWSim's Lambda service.
- Executions complete synchronously (blocking the response until done) or in the background depending on complexity.
- `DescribeExecution` returns `SUCCEEDED` or `FAILED` quickly after `StartExecution`.
- `GetExecutionHistory` returns a realistic sequence of events including `ExecutionStarted`, `StateEntered`, `StateExited`, and `ExecutionSucceeded`.
- State is in-memory only and lost on restart.
