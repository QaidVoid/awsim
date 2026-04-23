# Kinesis

Amazon Kinesis Data Streams for real-time data streaming and processing.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `kinesis` |
| Target Prefix | `Kinesis_20131202` |
| Persistence | No |

## Quick Start

Create a stream, put records, get a shard iterator, and read the records back:

```bash
# Create a stream with 2 shards
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Kinesis_20131202.CreateStream" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kinesis/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"StreamName":"my-stream","ShardCount":2}'

# Put a record (data must be base64-encoded)
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Kinesis_20131202.PutRecord" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kinesis/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"StreamName":"my-stream","PartitionKey":"user-123","Data":"SGVsbG8gV29ybGQ="}'

# Get a shard iterator (TRIM_HORIZON = from beginning)
ITERATOR=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Kinesis_20131202.GetShardIterator" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kinesis/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"StreamName":"my-stream","ShardId":"shardId-000000000000","ShardIteratorType":"TRIM_HORIZON"}' \
  | jq -r '.ShardIterator')

# Read records
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Kinesis_20131202.GetRecords" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kinesis/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"ShardIterator\":\"$ITERATOR\",\"Limit\":100}"
```

## Operations

### Streams
- `CreateStream` — create a new data stream
  - Input: `StreamName` (required), `ShardCount` (required, number of shards 1–N)
  - Returns: empty response; stream becomes `ACTIVE` immediately

- `DeleteStream` — delete a stream and all its data
  - Input: `StreamName`, optional `EnforceConsumerDeletion`

- `DescribeStream` — get full stream description including shard details
  - Input: `StreamName`, optional `Limit`, `ExclusiveStartShardId`
  - Returns: `StreamDescription` with `StreamName`, `StreamARN`, `StreamStatus` (`ACTIVE`), `Shards` (list with `ShardId`, `SequenceNumberRange`, `HashKeyRange`)

- `DescribeStreamSummary` — get a lightweight stream summary
  - Input: `StreamName`
  - Returns: `StreamDescriptionSummary` (no shard details)

- `ListStreams` — list all streams in the account/region
  - Returns: `StreamNames` list

- `ListShards` — list shards in a stream with pagination
  - Input: `StreamName`, optional `NextToken`, `MaxResults`
  - Returns: `Shards` list, `NextToken`

### Records
- `PutRecord` — write a single data record to a stream
  - Input: `StreamName`, `PartitionKey` (determines shard via MD5 hash), `Data` (base64-encoded bytes)
  - Returns: `ShardId`, `SequenceNumber` (monotonically increasing within a shard)

- `PutRecords` — write multiple records in a single batch call (up to 500)
  - Input: `StreamName`, `Records` (list of `{Data, PartitionKey}`)
  - Returns: `Records` (list with `ShardId`, `SequenceNumber`, or `ErrorCode`), `FailedRecordCount`

- `GetShardIterator` — get a position marker for reading records from a shard
  - Input: `StreamName`, `ShardId` (e.g., `shardId-000000000000`), `ShardIteratorType` (`TRIM_HORIZON` | `LATEST` | `AT_SEQUENCE_NUMBER` | `AFTER_SEQUENCE_NUMBER` | `AT_TIMESTAMP`)
  - Returns: `ShardIterator` (opaque string, valid for 5 minutes in real AWS)

- `GetRecords` — read records from a shard starting at a shard iterator
  - Input: `ShardIterator`, optional `Limit` (max records per call)
  - Returns: `Records` (list with `SequenceNumber`, `ApproximateArrivalTimestamp`, `Data` (base64), `PartitionKey`), `NextShardIterator`, `MillisBehindLatest`

### Shard Management
- `MergeShards` — merge two adjacent shards into one
  - Input: `StreamName`, `ShardToMerge`, `AdjacentShardToMerge`

- `SplitShard` — split a shard into two shards
  - Input: `StreamName`, `ShardToSplit`, `NewStartingHashKey` (MD5 hex value at which to split)

### Tags
- `AddTagsToStream` / `RemoveTagsFromStream` / `ListTagsForStream` — manage tags on streams

### Retention
- `IncreaseStreamRetentionPeriod` — extend the data retention period (up to 365 days)
  - Input: `StreamName`, `RetentionPeriodHours`

- `DecreaseStreamRetentionPeriod` — reduce the data retention period (minimum 24 hours)
  - Input: `StreamName`, `RetentionPeriodHours`

## Curl Examples

```bash
# 1. Put multiple records in a batch
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Kinesis_20131202.PutRecords" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kinesis/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"StreamName":"my-stream","Records":[{"Data":"eyJldmVudCI6ImNsaWNrIn0=","PartitionKey":"user-1"},{"Data":"eyJldmVudCI6InZpZXcifQ==","PartitionKey":"user-2"}]}'

# 2. List all streams
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Kinesis_20131202.ListStreams" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kinesis/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{}'

# 3. Increase retention to 7 days
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Kinesis_20131202.IncreaseStreamRetentionPeriod" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kinesis/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"StreamName":"my-stream","RetentionPeriodHours":168}'
```

## SDK Example

```typescript
import {
  KinesisClient,
  CreateStreamCommand,
  PutRecordsCommand,
  GetShardIteratorCommand,
  GetRecordsCommand,
  DescribeStreamCommand,
} from '@aws-sdk/client-kinesis';

const kinesis = new KinesisClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create stream
await kinesis.send(new CreateStreamCommand({
  StreamName: 'events-stream',
  ShardCount: 2,
}));

// Put records
const { Records, FailedRecordCount } = await kinesis.send(new PutRecordsCommand({
  StreamName: 'events-stream',
  Records: [
    {
      Data: Buffer.from(JSON.stringify({ type: 'click', userId: 'u-123' })),
      PartitionKey: 'u-123',
    },
    {
      Data: Buffer.from(JSON.stringify({ type: 'purchase', userId: 'u-456', amount: 29.99 })),
      PartitionKey: 'u-456',
    },
  ],
}));

console.log('Failed:', FailedRecordCount);
console.log('Placed on shard:', Records?.[0]?.ShardId);

// Read records back from shard 0
const { ShardIterator } = await kinesis.send(new GetShardIteratorCommand({
  StreamName: 'events-stream',
  ShardId: 'shardId-000000000000',
  ShardIteratorType: 'TRIM_HORIZON',
}));

const { Records: readRecords } = await kinesis.send(new GetRecordsCommand({
  ShardIterator: ShardIterator!,
  Limit: 100,
}));

readRecords?.forEach(record => {
  const data = JSON.parse(Buffer.from(record.Data!).toString());
  console.log(`${record.SequenceNumber}: ${JSON.stringify(data)}`);
});
```

## Behavior Notes

- Records are stored in memory per shard. The default retention period is 24 hours but automatic eviction is not enforced in AWSim.
- Partition keys are hashed using MD5 to route records to shards — this matches real Kinesis behavior, ensuring consistent shard assignment for the same partition key.
- Shard IDs are zero-padded 12-digit integers: `shardId-000000000000`, `shardId-000000000001`, etc.
- `GetShardIterator` with `TRIM_HORIZON` always returns an iterator pointing to the first record in the shard.
- `NextShardIterator` in `GetRecords` response can be used for subsequent polling.
- State is in-memory only and lost on restart.
