# Step Functions

AWS Step Functions serverless orchestration service for coordinating distributed application components using state machines.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_0` |
| Signing Name | `states` |
| Persistence | No |

## Operations

### State Machines
- `CreateStateMachine` — create a state machine from an Amazon States Language (ASL) definition
- `DeleteStateMachine` — delete a state machine
- `DescribeStateMachine` — get the definition and metadata of a state machine
- `ListStateMachines` — list all state machines in the account/region
- `UpdateStateMachine` — update the definition or role ARN of a state machine

### Executions
- `StartExecution` — start a new execution of a state machine
- `StopExecution` — stop a running execution
- `DescribeExecution` — get the status and output of an execution
- `ListExecutions` — list executions filtered by state machine or status
- `GetExecutionHistory` — retrieve the event history of an execution

## Example

```bash
# Create a simple state machine
aws --endpoint-url http://localhost:4567 \
  stepfunctions create-state-machine \
  --name my-workflow \
  --definition '{"Comment":"Example","StartAt":"Hello","States":{"Hello":{"Type":"Pass","Result":"Hello World","End":true}}}' \
  --role-arn arn:aws:iam::000000000000:role/StepFunctionsRole

# Start an execution
aws --endpoint-url http://localhost:4567 \
  stepfunctions start-execution \
  --state-machine-arn arn:aws:states:us-east-1:000000000000:stateMachine:my-workflow \
  --input '{"key":"value"}'

# Check execution status
aws --endpoint-url http://localhost:4567 \
  stepfunctions describe-execution \
  --execution-arn <execution-arn>
```

## Notes

- AWSim includes a basic ASL (Amazon States Language) interpreter that handles `Pass`, `Task`, `Choice`, `Wait`, `Succeed`, `Fail`, and `Parallel` state types.
- `Task` states with Lambda ARNs will attempt to invoke the Lambda function if the Lambda service is running in AWSim.
- Executions complete synchronously in the background; the status transitions from `RUNNING` to `SUCCEEDED` or `FAILED`.
- Complex flow control such as `Map` state iteration may have limited support.
