# EventBridge

Amazon EventBridge event bus and rule engine for routing events between AWS services and applications.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `events` |
| Target Prefix | `AWSEvents` |
| Persistence | No |

## Quick Start

Create an event bus, add a rule, configure a Lambda target, and publish an event:

```bash
# Create a custom event bus
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSEvents.CreateEventBus" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/events/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"my-app-events"}'

# Create a rule on the custom bus
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSEvents.PutRule" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/events/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"order-placed","EventBusName":"my-app-events","EventPattern":"{\"source\":[\"myapp\"],\"detail-type\":[\"OrderPlaced\"]}","State":"ENABLED"}'

# Publish an event
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSEvents.PutEvents" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/events/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Entries":[{"Source":"myapp","DetailType":"OrderPlaced","Detail":"{\"orderId\":\"123\",\"amount\":49.99}","EventBusName":"my-app-events"}]}'
```

## Operations

### Event Buses
- `CreateEventBus` — create a custom event bus
  - Input: `Name` (required), optional `EventSourceName`, `Tags`
  - Returns: `EventBusArn` (e.g., `arn:aws:events:us-east-1:000000000000:event-bus/my-app-events`)

- `DeleteEventBus` — delete an event bus (cannot delete the `default` bus)
  - Input: `Name`

- `DescribeEventBus` — get details of an event bus
  - Input: `Name`
  - Returns: `Name`, `Arn`, `Policy` (if set)

- `ListEventBuses` — list all event buses
  - Input: optional `NamePrefix`, `NextToken`, `Limit`
  - Returns: paginated `EventBuses` list; `default` bus is always present

### Rules
- `PutRule` — create or update an event rule
  - Input: `Name` (required), `EventBusName` (default: `default`), one of `ScheduleExpression` (e.g., `rate(5 minutes)`, `cron(0 12 * * ? *)`) or `EventPattern` (JSON filter), `State` (`ENABLED` or `DISABLED`), `Description`, `RoleArn`, `Tags`
  - Returns: `RuleArn`

- `DeleteRule` — delete a rule (must first remove all targets)
  - Input: `Name`, `EventBusName`

- `DescribeRule` — get full rule configuration
  - Input: `Name`, `EventBusName`
  - Returns: `Name`, `Arn`, `EventPattern`, `ScheduleExpression`, `State`, `Targets`

- `ListRules` — list rules with optional event bus filter
  - Input: optional `EventBusName`, `NamePrefix`, `NextToken`, `Limit`

- `EnableRule` / `DisableRule` — change rule state
  - Input: `Name`, `EventBusName`

### Targets
- `PutTargets` — add targets to a rule (up to 5 per call)
  - Input: `Rule`, `EventBusName`, `Targets` (list of `{Id, Arn, RoleArn, InputTransformer, Input, InputPath}`)
  - `Arn` can be Lambda ARN, SQS queue ARN, SNS topic ARN, Step Functions state machine ARN, etc.
  - Returns: `FailedEntryCount`, `FailedEntries`

- `RemoveTargets` — remove targets from a rule
  - Input: `Rule`, `EventBusName`, `Ids` (list of target IDs)

- `ListTargetsByRule` — list targets associated with a rule
  - Input: `Rule`, `EventBusName`
  - Returns: `Targets` list

### Events
- `PutEvents` — publish events to an event bus (up to 10 per call)
  - Input: `Entries` (list of `{Source, DetailType, Detail, EventBusName, Time, Resources}`)
  - `Detail` must be a JSON string
  - Returns: `FailedEntryCount`, `Entries` (each with `EventId` on success, or `ErrorCode`/`ErrorMessage` on failure)

### Tags
- `TagResource` / `UntagResource` / `ListTagsForResource` — manage tags on event buses and rules

### Event Sources
- `DescribeEventSource` — stub returning event source details. Input: `Name`
- `ListEventSources` — returns empty list (partner event sources are not provisioned locally)
- `PutPartnerEventSource` — stub that accepts and returns an ARN. Input: `Name`, `Account`

### Archives
- `CreateArchive` — create an event archive attached to an event bus. Input: `ArchiveName` (required), `EventSourceArn` (required), optional `Description`, `EventPattern`, `RetentionDays`
- `DeleteArchive` — delete an archive. Input: `ArchiveName`
- `DescribeArchive` — get archive details. Input: `ArchiveName`. Returns `ArchiveName`, `ArchiveArn`, `EventSourceArn`, `RetentionDays`, `State`
- `ListArchives` — list all archives. Returns `Archives` list

### Connections
- `CreateConnection` — create an API destination connection (auth config). Input: `Name` (required), `AuthorizationType` (required, e.g. `API_KEY`, `OAUTH_CLIENT_CREDENTIALS`, `BASIC`), optional `Description`, `AuthParameters`
- `DeleteConnection` — delete a connection. Input: `Name`
- `DescribeConnection` — get connection details. Input: `Name`
- `ListConnections` — list all connections. Returns `Connections` list

### API Destinations
- `CreateApiDestination` — create an HTTP API destination. Input: `Name` (required), `ConnectionArn` (required), `InvocationEndpoint` (required), `HttpMethod` (required), optional `Description`, `InvocationRateLimitPerSecond`
- `DeleteApiDestination` — delete an API destination. Input: `Name`
- `DescribeApiDestination` — get API destination details. Input: `Name`
- `ListApiDestinations` — list all API destinations. Returns `ApiDestinations` list

### Replays
- `StartReplay` — start an event replay from an archive. Input: `ReplayName` (required), `EventSourceArn` (required), `Destination`, `EventStartTime`, `EventEndTime`, optional `Description`. Returns immediately as `COMPLETED` (dev stub)
- `CancelReplay` — cancel a running replay. Input: `ReplayName`
- `DescribeReplay` — get replay details. Input: `ReplayName`
- `ListReplays` — list all replays. Returns `Replays` list

## Curl Examples

```bash
# 1. Create a scheduled rule (every 15 minutes)
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSEvents.PutRule" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/events/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"cleanup-task","ScheduleExpression":"rate(15 minutes)","State":"ENABLED"}'

# 2. Add a Lambda target to the rule
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSEvents.PutTargets" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/events/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Rule":"cleanup-task","Targets":[{"Id":"cleanup-lambda","Arn":"arn:aws:lambda:us-east-1:000000000000:function:cleanup"}]}'

# 3. Publish multiple events
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSEvents.PutEvents" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/events/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Entries":[{"Source":"myapp.orders","DetailType":"OrderCreated","Detail":"{\"id\":\"111\"}","EventBusName":"default"},{"Source":"myapp.orders","DetailType":"OrderShipped","Detail":"{\"id\":\"222\"}","EventBusName":"default"}]}'
```

## SDK Example

```typescript
import {
  EventBridgeClient,
  CreateEventBusCommand,
  PutRuleCommand,
  PutTargetsCommand,
  PutEventsCommand,
  ListRulesCommand,
} from '@aws-sdk/client-eventbridge';

const events = new EventBridgeClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create event bus
const { EventBusArn } = await events.send(new CreateEventBusCommand({
  Name: 'my-app-events',
}));

// Create pattern-matching rule
await events.send(new PutRuleCommand({
  Name: 'process-orders',
  EventBusName: 'my-app-events',
  EventPattern: JSON.stringify({
    source: ['myapp.orders'],
    'detail-type': ['OrderCreated'],
  }),
  State: 'ENABLED',
}));

// Add Lambda target
await events.send(new PutTargetsCommand({
  Rule: 'process-orders',
  EventBusName: 'my-app-events',
  Targets: [{
    Id: 'order-processor',
    Arn: 'arn:aws:lambda:us-east-1:000000000000:function:process-order',
  }],
}));

// Publish an event
const { FailedEntryCount, Entries } = await events.send(new PutEventsCommand({
  Entries: [{
    Source: 'myapp.orders',
    DetailType: 'OrderCreated',
    Detail: JSON.stringify({ orderId: 'order-123', amount: 99.99, userId: 'user-456' }),
    EventBusName: 'my-app-events',
  }],
}));

console.log('Failed:', FailedEntryCount); // 0
console.log('Event ID:', Entries?.[0]?.EventId);
```

## Behavior Notes

- Events published via `PutEvents` are accepted and stored but **no actual event routing or target invocation** occurs in AWSim.
- The `default` event bus is automatically available and cannot be deleted.
- Schedule expressions and event patterns are stored as strings but not evaluated or matched.
- `PutEvents` returns a unique `EventId` for each entry, matching real EventBridge behavior.
- State is in-memory only and lost on restart.
