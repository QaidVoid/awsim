# DynamoDB

Amazon DynamoDB fully managed NoSQL database service with single-digit millisecond performance at any scale.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_0` |
| Signing Name | `dynamodb` |
| Target Prefix | `DynamoDB_20120810` |
| Persistence | Yes |

## Quick Start

Create a table, put an item, and retrieve it:

```bash
# Create table
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.0" \
  -H "X-Amz-Target: DynamoDB_20120810.CreateTable" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/dynamodb/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"TableName":"users","KeySchema":[{"AttributeName":"pk","KeyType":"HASH"},{"AttributeName":"sk","KeyType":"RANGE"}],"AttributeDefinitions":[{"AttributeName":"pk","AttributeType":"S"},{"AttributeName":"sk","AttributeType":"S"}],"BillingMode":"PAY_PER_REQUEST"}'

# Put item
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.0" \
  -H "X-Amz-Target: DynamoDB_20120810.PutItem" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/dynamodb/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"TableName":"users","Item":{"pk":{"S":"user#1"},"sk":{"S":"profile"},"name":{"S":"Alice"},"age":{"N":"30"}}}'

# Get item
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.0" \
  -H "X-Amz-Target: DynamoDB_20120810.GetItem" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/dynamodb/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"TableName":"users","Key":{"pk":{"S":"user#1"},"sk":{"S":"profile"}}}'
```

## Operations

### Table Operations

| Operation | Description |
|-----------|-------------|
| `CreateTable` | Create a table with key schema, billing mode, and optional streams. Returns `TableDescription` with `TableStatus: "ACTIVE"` immediately |
| `DeleteTable` | Delete a table and all its data. Returns `TableDescription` with `TableStatus: "DELETING"` |
| `TruncateTable` | **awsim-only.** Wipe every item in a table while keeping the schema, indexes, and stream config. No equivalent in real DynamoDB. Input: `TableName`. Returns `{ TableName, DeletedItemCount }` |
| `DescribeTable` | Get table metadata: key schema, attribute definitions, item count (live count from SQLite), table size, stream ARN |
| `ListTables` | List all tables; supports `ExclusiveStartTableName` and `Limit` for pagination |
| `UpdateTable` | Update billing mode (`PAY_PER_REQUEST` or `PROVISIONED`) or stream specification |
| `DescribeEndpoints` | Return the regional DynamoDB endpoint. Used by SDK endpoint discovery; returns a single endpoint entry |

### TTL Operations

| Operation | Description |
|-----------|-------------|
| `DescribeTimeToLive` | Get the TTL configuration for a table. Returns `TimeToLiveDescription` with `TimeToLiveStatus` and `AttributeName` |
| `UpdateTimeToLive` | Enable or disable TTL on a table. Input: `TableName`, `TimeToLiveSpecification` (`{Enabled, AttributeName}`) |

### Backup Operations

| Operation | Description |
|-----------|-------------|
| `DescribeContinuousBackups` | Get the point-in-time recovery (PITR) status for a table. Returns `ContinuousBackupsDescription` |
| `CreateBackup` | Create an on-demand backup (stub — returns a backup ARN but does not persist) |
| `DeleteBackup` | Delete a backup (stub — always succeeds) |
| `DescribeBackup` | Describe a backup (stub — always returns not-found) |
| `ListBackups` | List on-demand backups (stub — returns empty list) |

### Global Tables

| Operation | Description |
|-----------|-------------|
| `DescribeGlobalTable` | Describe a global table (stub — returns not-found) |
| `ListGlobalTables` | List global tables (stub — returns empty list) |

### Export / Import

| Operation | Description |
|-----------|-------------|
| `DescribeExport` | Describe an export (stub — returns not-found) |
| `ExportTableToPointInTime` | Export to S3 (stub — returns not-supported error) |
| `ListExports` | List exports (stub — returns empty list) |
| `DescribeImport` | Describe an import (stub — returns not-found) |
| `ImportTable` | Import from S3 (stub — returns not-supported error) |
| `ListImports` | List imports (stub — returns empty list) |

### Account Limits

| Operation | Description |
|-----------|-------------|
| `DescribeLimits` | Return default account-level throughput limits (called by Terraform on every plan) |

### Contributor Insights

| Operation | Description |
|-----------|-------------|
| `DescribeContributorInsights` | Describe contributor insights for a table (stub — always returns DISABLED) |
| `UpdateContributorInsights` | Enable/disable contributor insights (stub — acknowledges the change) |
| `ListContributorInsights` | List contributor insights summaries (stub — returns empty list) |

### Tagging Operations

| Operation | Description |
|-----------|-------------|
| `TagResource` | Add tags to a table (by ARN). Input: `ResourceArn`, `Tags` list |
| `UntagResource` | Remove tags from a table. Input: `ResourceArn`, `TagKeys` list |
| `ListTagsOfResource` | List all tags for a table. Input: `ResourceArn`. Returns paginated `Tags` list |

### Item Operations

| Operation | Description |
|-----------|-------------|
| `PutItem` | Write an item. All attributes use DynamoDB type notation: `{"S":"string"}`, `{"N":"123"}`, `{"BOOL":true}`, `{"L":[...]}`, `{"M":{...}}` |
| `GetItem` | Read an item by primary key. Returns `Item` or empty if not found. Use `ProjectionExpression` to return subset of attributes |
| `DeleteItem` | Delete an item by primary key. Use `ReturnValues: "ALL_OLD"` to get the deleted item back |
| `UpdateItem` | Update specific attributes using `UpdateExpression` (e.g., `SET #name = :val`), `ExpressionAttributeNames`, `ExpressionAttributeValues` |

### Query and Scan

| Operation | Description |
|-----------|-------------|
| `Query` | Query items using `KeyConditionExpression` on partition key (and optionally sort key). Supports `FilterExpression`, `ProjectionExpression`, `Limit`, `ScanIndexForward` (ascending/descending), pagination via `ExclusiveStartKey` |
| `Scan` | Scan all items with optional `FilterExpression`. Supports `Limit`, pagination via `ExclusiveStartKey`. Use sparingly on large tables |

### Batch Operations

| Operation | Description |
|-----------|-------------|
| `BatchGetItem` | Read up to 100 items from one or more tables in one call. Returns `Responses` (found items) and `UnprocessedKeys` (retry these) |
| `BatchWriteItem` | Write or delete up to 25 items in one call. Mix of `PutRequest` and `DeleteRequest`. Returns `UnprocessedItems` |

### Transactions

| Operation | Description |
|-----------|-------------|
| `TransactGetItems` | Atomic multi-table read of up to 25 items; all succeed or all fail |
| `TransactWriteItems` | Atomic multi-table write of up to 25 items; mix of `Put`, `Update`, `Delete`, `ConditionCheck` |

### PartiQL

| Operation | Description |
|-----------|-------------|
| `ExecuteStatement` | Execute a single PartiQL statement. Supports basic `SELECT`, `INSERT INTO ... VALUE {...}`, `UPDATE ... SET`, `DELETE FROM` with simple `WHERE` equality conditions |
| `BatchExecuteStatement` | Execute multiple PartiQL statements; returns per-statement results with partial failures |
| `ExecuteTransaction` | Execute multiple PartiQL statements as an atomic transaction; any failure aborts all |

### Streams

| Operation | Description |
|-----------|-------------|
| `DescribeStream` | Get stream metadata including shards. Stream ARN is returned in `DescribeTable` |
| `GetShardIterator` | Get a position marker (`TRIM_HORIZON`, `LATEST`, `AT_SEQUENCE_NUMBER`, `AFTER_SEQUENCE_NUMBER`) |
| `GetRecords` | Read change records from a shard; each record has `eventName` (`INSERT`, `MODIFY`, `REMOVE`) and `dynamodb` with old/new images |
| `ListStreams` | List streams for a table or account |

## SDK Example

```typescript
import {
  DynamoDBClient,
  CreateTableCommand,
  QueryCommand,
} from '@aws-sdk/client-dynamodb';
import {
  DynamoDBDocumentClient,
  PutCommand,
  GetCommand,
  UpdateCommand,
  DeleteCommand,
} from '@aws-sdk/lib-dynamodb';

const client = new DynamoDBClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

const ddb = DynamoDBDocumentClient.from(client);

// Create table
await client.send(new CreateTableCommand({
  TableName: 'users',
  KeySchema: [
    { AttributeName: 'pk', KeyType: 'HASH' },
    { AttributeName: 'sk', KeyType: 'RANGE' },
  ],
  AttributeDefinitions: [
    { AttributeName: 'pk', AttributeType: 'S' },
    { AttributeName: 'sk', AttributeType: 'S' },
  ],
  BillingMode: 'PAY_PER_REQUEST',
  StreamSpecification: { StreamEnabled: true, StreamViewType: 'NEW_AND_OLD_IMAGES' },
}));

// Put item (Document client handles type marshalling automatically)
await ddb.send(new PutCommand({
  TableName: 'users',
  Item: { pk: 'user#123', sk: 'profile', name: 'Alice', age: 30, tags: ['admin', 'beta'] },
}));

// Get item
const { Item } = await ddb.send(new GetCommand({
  TableName: 'users',
  Key: { pk: 'user#123', sk: 'profile' },
}));
console.log(Item?.name); // Alice

// Update a field
await ddb.send(new UpdateCommand({
  TableName: 'users',
  Key: { pk: 'user#123', sk: 'profile' },
  UpdateExpression: 'SET age = :newAge',
  ExpressionAttributeValues: { ':newAge': 31 },
}));

// Query all items for a user
const { Items } = await client.send(new QueryCommand({
  TableName: 'users',
  KeyConditionExpression: 'pk = :pk',
  ExpressionAttributeValues: { ':pk': { S: 'user#123' } },
}));
console.log('User items:', Items?.length);
```

## CLI Example

```bash
# Create table
aws --endpoint-url http://localhost:4566 dynamodb create-table \
  --table-name users \
  --key-schema AttributeName=pk,KeyType=HASH AttributeName=sk,KeyType=RANGE \
  --attribute-definitions AttributeName=pk,AttributeType=S AttributeName=sk,AttributeType=S \
  --billing-mode PAY_PER_REQUEST

# Put item
aws --endpoint-url http://localhost:4566 dynamodb put-item \
  --table-name users \
  --item '{"pk":{"S":"user#1"},"sk":{"S":"profile"},"name":{"S":"Alice"}}'

# Get item
aws --endpoint-url http://localhost:4566 dynamodb get-item \
  --table-name users \
  --key '{"pk":{"S":"user#1"},"sk":{"S":"profile"}}'

# Query
aws --endpoint-url http://localhost:4566 dynamodb query \
  --table-name users \
  --key-condition-expression "pk = :pk" \
  --expression-attribute-values '{":pk":{"S":"user#1"}}'

# Scan with filter
aws --endpoint-url http://localhost:4566 dynamodb scan \
  --table-name users \
  --filter-expression "age > :age" \
  --expression-attribute-values '{":age":{"N":"25"}}'
```

## Behavior Notes

- DynamoDB is persistent: tables and items survive AWSim restarts. Items live in a SQLite database (`{data_dir}/dynamodb.db`) — see [DynamoDB SQLite store](../guide/persistence.md#dynamodb-sqlite-store) for details.
- Global Secondary Indexes (GSI) and Local Secondary Indexes (LSI) are accepted in `CreateTable` but queries with `IndexName` are not yet pushed down to the GSI key columns — they fall back to a full table scan. (The SQLite store materializes up to 5 GSI key column pairs per item, so the data is there; the planner just hasn't been wired to use them yet.)
- Conditional expressions (`ConditionExpression` on `PutItem`, `UpdateItem`, `DeleteItem`) are evaluated against the existing item — failed checks return `ConditionalCheckFailedException`.
- `TransactWriteItems` is genuinely atomic: phase 1 (validate every condition) and phase 2 (apply every mutation) run inside a single SQLite write transaction. A failing condition anywhere in the batch rolls back every mutation that had already been applied. `TransactGetItems` reads see a single consistent snapshot.
- TTL (Time to Live) configuration (`UpdateTimeToLive`) is accepted and stored but items are not automatically expired.
- `DescribeContinuousBackups` returns a stub response indicating PITR is disabled — no actual backups are made.
- `DescribeEndpoints` returns a single endpoint entry for SDK endpoint discovery compatibility.
- Table tags (`TagResource`, `UntagResource`, `ListTagsOfResource`) are stored and returned correctly.
- PartiQL (`ExecuteStatement`, `BatchExecuteStatement`, `ExecuteTransaction`) supports basic `SELECT * FROM "Table" WHERE "key" = 'value'`, `INSERT INTO "Table" VALUE {...}`, `UPDATE "Table" SET attr = val WHERE "key" = 'val'`, and `DELETE FROM "Table" WHERE "key" = 'val'`. Full PartiQL expressions, nested paths, and `?` parameter binding (with `Parameters` list) are partially supported.
- Streams store change records in a bounded in-memory ring buffer (last 1 000 per table); use `GetShardIterator` with `TRIM_HORIZON` to read from the beginning. Items themselves live in SQLite.
- `TruncateTable` is awsim-only and does not exist in real DynamoDB — guard your code with a feature flag if you also target the real service.
