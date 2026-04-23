# Kinesis

Amazon Kinesis Data Streams for real-time data streaming and processing.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `kinesis` |
| Persistence | No |

## Operations

### Streams
- `CreateStream` — create a new data stream with a specified shard count
- `DeleteStream` — delete a stream and all its data
- `DescribeStream` — get full stream description including shard details
- `DescribeStreamSummary` — get a lightweight stream summary
- `ListStreams` — list all streams in the account/region
- `ListShards` — list shards in a stream with pagination support

### Records
- `PutRecord` — write a single data record to a stream
- `PutRecords` — write multiple records in a single batch call
- `GetShardIterator` — get a position marker for reading records from a shard
- `GetRecords` — read records from a shard starting at a shard iterator

### Shard Management
- `MergeShards` — merge two adjacent shards into one
- `SplitShard` — split a shard into two shards

### Tags
- `AddTagsToStream` — add tags to a stream
- `RemoveTagsFromStream` — remove tags from a stream
- `ListTagsForStream` — list tags on a stream

### Retention
- `IncreaseStreamRetentionPeriod` — extend the data retention period (up to 365 days)
- `DecreaseStreamRetentionPeriod` — reduce the data retention period (minimum 24 hours)

## Example

```bash
# Create a stream with 2 shards
aws --endpoint-url http://localhost:4567 \
  kinesis create-stream \
  --stream-name my-stream \
  --shard-count 2

# Put a record
aws --endpoint-url http://localhost:4567 \
  kinesis put-record \
  --stream-name my-stream \
  --partition-key "user-123" \
  --data "SGVsbG8gV29ybGQ="

# Get a shard iterator
aws --endpoint-url http://localhost:4567 \
  kinesis get-shard-iterator \
  --stream-name my-stream \
  --shard-id shardId-000000000000 \
  --shard-iterator-type TRIM_HORIZON

# Read records
aws --endpoint-url http://localhost:4567 \
  kinesis get-records \
  --shard-iterator <iterator>
```

## Notes

- Records are stored in memory per shard. The default retention period is 24 hours but is not enforced with automatic eviction in AWSim.
- Partition keys are used to route records to shards using MD5 hashing, matching the real Kinesis behavior.
- Shard iterators are position tokens that point to a sequence number within a shard.
