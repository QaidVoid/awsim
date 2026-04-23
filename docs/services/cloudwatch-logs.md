# CloudWatch Logs

Amazon CloudWatch Logs for collecting, storing, and querying log data from applications and AWS services.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `logs` |
| Target Prefix | `Logs_20140328` |
| Persistence | No |

## Quick Start

Create a log group, add a stream, write some events, and read them back:

```bash
# Create a log group
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Logs_20140328.CreateLogGroup" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/logs/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"logGroupName":"/myapp/production"}'

# Create a log stream
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Logs_20140328.CreateLogStream" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/logs/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"logGroupName":"/myapp/production","logStreamName":"app-instance-1"}'

# Write log events (timestamp in milliseconds)
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Logs_20140328.PutLogEvents" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/logs/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"logGroupName":"/myapp/production","logStreamName":"app-instance-1","logEvents":[{"timestamp":1700000000000,"message":"Application started"},{"timestamp":1700000001000,"message":"Request received: GET /api/users"}]}'
```

## Operations

### Log Groups
- `CreateLogGroup` — create a new log group
  - Input: `logGroupName` (required), optional `kmsKeyId`, `tags`
  - Returns: empty response (HTTP 200)

- `DeleteLogGroup` — delete a log group and all its streams and events
  - Input: `logGroupName`

- `DescribeLogGroups` — list log groups with optional prefix filter
  - Input: optional `logGroupNamePrefix`, `limit`, `nextToken`
  - Returns: paginated `logGroups` list with `logGroupName`, `creationTime`, `retentionInDays`, `storedBytes`

- `PutRetentionPolicy` — set the retention period for a log group (in days)
  - Input: `logGroupName`, `retentionInDays` (valid values: 1, 3, 5, 7, 14, 30, 60, 90, 120, 150, 180, 365, 400, 545, 731, 1096, 1827, 2192, 2557, 2922, 3288, 3653)

- `DeleteRetentionPolicy` — remove the retention policy from a log group
  - Input: `logGroupName`

- `TagLogGroup` / `UntagLogGroup` / `ListTagsLogGroup` — manage tags on log groups

### Log Streams
- `CreateLogStream` — create a log stream within a log group
  - Input: `logGroupName`, `logStreamName`

- `DeleteLogStream` — delete a log stream and all its events

- `DescribeLogStreams` — list log streams within a log group
  - Input: `logGroupName`, optional `logStreamNamePrefix`, `orderBy` (`LogStreamName` or `LastEventTime`), `descending`, `limit`, `nextToken`
  - Returns: paginated `logStreams` list with `logStreamName`, `firstEventTimestamp`, `lastEventTimestamp`, `uploadSequenceToken`

### Log Events
- `PutLogEvents` — write log events to a log stream
  - Input: `logGroupName`, `logStreamName`, `logEvents` (list of `{timestamp, message}`), optional `sequenceToken`
  - Returns: `nextSequenceToken` (use for subsequent puts to same stream)
  - `timestamp` must be in **milliseconds** since Unix epoch

- `GetLogEvents` — read log events from a log stream
  - Input: `logGroupName`, `logStreamName`, optional `startTime`, `endTime` (milliseconds), `limit`, `startFromHead`
  - Returns: `events` list with `timestamp`, `message`, `ingestionTime`

- `FilterLogEvents` — search across log streams using a filter pattern
  - Input: `logGroupName`, optional `logStreamNames`, `startTime`, `endTime`, `filterPattern`, `limit`
  - Returns: `events` list with matching log entries

## Curl Examples

```bash
# 1. Set 30-day retention on a log group
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Logs_20140328.PutRetentionPolicy" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/logs/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"logGroupName":"/myapp/production","retentionInDays":30}'

# 2. Filter log events by pattern
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Logs_20140328.FilterLogEvents" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/logs/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"logGroupName":"/myapp/production","filterPattern":"ERROR","startTime":1700000000000}'

# 3. Read events from a stream
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Logs_20140328.GetLogEvents" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/logs/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"logGroupName":"/myapp/production","logStreamName":"app-instance-1","startFromHead":true}'
```

## SDK Example

```typescript
import {
  CloudWatchLogsClient,
  CreateLogGroupCommand,
  CreateLogStreamCommand,
  PutLogEventsCommand,
  FilterLogEventsCommand,
} from '@aws-sdk/client-cloudwatch-logs';

const logs = new CloudWatchLogsClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create log group and stream
await logs.send(new CreateLogGroupCommand({ logGroupName: '/myapp/prod' }));
await logs.send(new CreateLogStreamCommand({
  logGroupName: '/myapp/prod',
  logStreamName: 'instance-1',
}));

// Write log events
await logs.send(new PutLogEventsCommand({
  logGroupName: '/myapp/prod',
  logStreamName: 'instance-1',
  logEvents: [
    { timestamp: Date.now(), message: 'Application started' },
    { timestamp: Date.now() + 100, message: 'ERROR: Database connection failed' },
  ],
}));

// Filter for errors
const { events } = await logs.send(new FilterLogEventsCommand({
  logGroupName: '/myapp/prod',
  filterPattern: 'ERROR',
}));

console.log('Error events:', events?.map(e => e.message));
```

## Behavior Notes

- `PutLogEvents` accepts any `sequenceToken` (or none) on subsequent calls — sequence validation is not strictly enforced.
- `FilterLogEvents` performs simple substring matching on the `filterPattern` — complex CloudWatch filter syntax (JSON matchers, metric filters, etc.) is not fully supported.
- Retention policies are stored but not enforced: log events are not automatically deleted after the retention period.
- Lambda functions in AWSim automatically write their stdout/stderr to `/aws/lambda/{function-name}` log groups.
- Timestamps must be in milliseconds (not seconds) since Unix epoch.
- State is in-memory only and lost on restart.
