# EventBridge Pipes

Point-to-point integrations between a source and a target with optional content filtering and Lambda enrichment. Pipes runs as a separate AWS service (`pipes`) and uses RestJson1, not the EventBridge events bus.

**Endpoint:** `http://localhost:4566`
**Signing name:** `pipes`
**Protocol:** REST-JSON

## Operations

| Operation | Method / Path |
|-----------|--------------|
| `CreatePipe` | `POST /v1/pipes/{Name}` |
| `DescribePipe` | `GET /v1/pipes/{Name}` |
| `ListPipes` | `GET /v1/pipes` |
| `UpdatePipe` | `PUT /v1/pipes/{Name}` |
| `DeletePipe` | `DELETE /v1/pipes/{Name}` |
| `StartPipe` | `POST /v1/pipes/{Name}/start` |
| `StopPipe` | `POST /v1/pipes/{Name}/stop` |
| `TagResource` | `POST /tags/{ResourceArn}` |
| `UntagResource` | `DELETE /tags/{ResourceArn}` |
| `ListTagsForResource` | `GET /tags/{ResourceArn}` |

`ListPipes` accepts the standard `NamePrefix`, `SourcePrefix`, `TargetPrefix`, `CurrentState`, and `DesiredState` query filters.

## Runner behavior

A background runner spawned by the AWSim binary drives every pipe whose `CurrentState` is `RUNNING`:

1. **Source poll.** Reads up to `SourceParameters.SqsQueueParameters.BatchSize` messages from the source SQS queue (default 10).
2. **Filter.** Applies `SourceParameters.FilterCriteria` if present — same content-pattern syntax as Lambda event source mapping FilterCriteria (equality arrays, `prefix`, `suffix`, `exists`, `anything-but`, `numeric`). Records that fail the filter are deleted from the source queue.
3. **Enrichment** *(optional)*. If `Enrichment` is a Lambda ARN, invokes it with the kept records as the payload and uses the response as the next-stage payload.
4. **Target dispatch.** Forwards the payload to the target. Source messages are deleted only after the target dispatch succeeds; failed dispatches leave them in the source queue for the next tick.

### Supported sources
- **SQS** (`arn:aws:sqs:...`)

### Supported targets
- **Lambda function** (`arn:aws:lambda:...:function:NAME`) — invoked with `InvocationType=Event`
- **Step Functions state machine** (`arn:aws:states:...:stateMachine:NAME`) — `StartExecution`
- **SQS queue** (`arn:aws:sqs:...`) — `SendMessage`
- **SNS topic** (`arn:aws:sns:...`) — `Publish`

Other source types (Kinesis, DynamoDB Streams, MSK) and target types (EventBridge bus, API destination, ECS task, etc.) accept the create call but the runner skips them — the pipe stays `RUNNING` but no records flow.

## State machine

`Create` initializes `CurrentState` to `RUNNING` (or `STOPPED` if `DesiredState=STOPPED` was passed). `StartPipe` / `StopPipe` flip both `CurrentState` and `DesiredState` immediately — the emulator never lingers in transitional `CREATING` / `STOPPING` states the way real AWS does.

## Persistence

When AWSim is started with `--data-dir`, pipes are snapshotted to disk and restored on the next startup. The runner picks up where it left off because pipe state lives in the snapshot.

## Limitations

- Source-poll cadence is fixed at 2s. Real Pipes is event-driven.
- No CloudWatch Logs integration for `LogConfiguration` — the field round-trips on Describe but does not emit logs.
- `MaximumBatchingWindowInSeconds` and `Concurrency` parameters round-trip but are not honored by the runner.
