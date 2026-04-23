# CloudWatch Logs

Amazon CloudWatch Logs for collecting, storing, and querying log data from applications and AWS services.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `logs` |
| Persistence | No |

## Operations

### Log Groups
- `CreateLogGroup` — create a new log group
- `DeleteLogGroup` — delete a log group and all its streams
- `DescribeLogGroups` — list log groups with optional prefix filter
- `PutRetentionPolicy` — set the retention period for a log group (days)
- `DeleteRetentionPolicy` — remove the retention policy from a log group
- `TagLogGroup` — add tags to a log group
- `UntagLogGroup` — remove tags from a log group
- `ListTagsLogGroup` — list tags on a log group

### Log Streams
- `CreateLogStream` — create a log stream within a log group
- `DeleteLogStream` — delete a log stream
- `DescribeLogStreams` — list log streams within a log group

### Log Events
- `PutLogEvents` — write log events to a log stream
- `GetLogEvents` — read log events from a log stream with optional time range
- `FilterLogEvents` — search across log streams using a filter pattern

## Example

```bash
# Create a log group
aws --endpoint-url http://localhost:4567 \
  logs create-log-group \
  --log-group-name /myapp/production

# Set 30-day retention
aws --endpoint-url http://localhost:4567 \
  logs put-retention-policy \
  --log-group-name /myapp/production \
  --retention-in-days 30

# Create a log stream
aws --endpoint-url http://localhost:4567 \
  logs create-log-stream \
  --log-group-name /myapp/production \
  --log-stream-name app-instance-1

# Write log events
aws --endpoint-url http://localhost:4567 \
  logs put-log-events \
  --log-group-name /myapp/production \
  --log-stream-name app-instance-1 \
  --log-events '[{"timestamp":1700000000000,"message":"Application started"}]'

# Filter events by pattern
aws --endpoint-url http://localhost:4567 \
  logs filter-log-events \
  --log-group-name /myapp/production \
  --filter-pattern "ERROR"
```

## Notes

- `PutLogEvents` requires a `sequenceToken` on subsequent calls to the same stream; AWSim accepts any token or none.
- `FilterLogEvents` performs simple substring matching — complex CloudWatch filter syntax is not fully supported.
- Retention policies are stored but not enforced (log events are not automatically deleted).
- Lambda functions in AWSim automatically write their output to `/aws/lambda/{function-name}` log groups.
