# DynamoDB

**Protocol:** JSON (`X-Amz-Target: DynamoDB_20120810.*`)  
**Signing name:** `dynamodb`  
**Persistent:** Yes

## Implemented Operations

### Table Operations

| Operation | Description |
|-----------|-------------|
| `CreateTable` | Create a table with key schema, billing mode, and optional streams |
| `DeleteTable` | Delete a table and all its data |
| `DescribeTable` | Get table metadata |
| `ListTables` | List all tables |
| `UpdateTable` | Update billing mode or stream specification |

### Item Operations

| Operation | Description |
|-----------|-------------|
| `PutItem` | Write an item |
| `GetItem` | Read an item by primary key |
| `DeleteItem` | Delete an item by primary key |
| `UpdateItem` | Update specific attributes of an item |

### Query and Scan

| Operation | Description |
|-----------|-------------|
| `Query` | Query items using key condition expressions |
| `Scan` | Scan all items with optional filter expression |

### Batch Operations

| Operation | Description |
|-----------|-------------|
| `BatchGetItem` | Read up to 100 items in one request |
| `BatchWriteItem` | Write or delete up to 25 items in one request |

### Transactions

| Operation | Description |
|-----------|-------------|
| `TransactGetItems` | Atomic multi-table read (up to 25 items) |
| `TransactWriteItems` | Atomic multi-table write (up to 25 items) |

### Streams

| Operation | Description |
|-----------|-------------|
| `DescribeStream` | Get stream metadata |
| `GetShardIterator` | Get a shard iterator |
| `GetRecords` | Read records from a shard |
| `ListStreams` | List streams for a table |

## SDK Example

```typescript
import { DynamoDBClient, CreateTableCommand, PutItemCommand, GetItemCommand, QueryCommand } from "@aws-sdk/client-dynamodb";
import { DynamoDBDocumentClient, PutCommand, GetCommand, QueryCommand as DocQueryCommand } from "@aws-sdk/lib-dynamodb";

const client = new DynamoDBClient({
  region: "us-east-1",
  endpoint: "http://localhost:4566",
  credentials: { accessKeyId: "test", secretAccessKey: "test" },
});

const ddb = DynamoDBDocumentClient.from(client);

// Create table
await client.send(new CreateTableCommand({
  TableName: "users",
  KeySchema: [
    { AttributeName: "pk", KeyType: "HASH" },
    { AttributeName: "sk", KeyType: "RANGE" },
  ],
  AttributeDefinitions: [
    { AttributeName: "pk", AttributeType: "S" },
    { AttributeName: "sk", AttributeType: "S" },
  ],
  BillingMode: "PAY_PER_REQUEST",
}));

// Put item
await ddb.send(new PutCommand({
  TableName: "users",
  Item: { pk: "user#123", sk: "profile", name: "Alice", age: 30 },
}));

// Get item
const result = await ddb.send(new GetCommand({
  TableName: "users",
  Key: { pk: "user#123", sk: "profile" },
}));
console.log(result.Item);
```

## CLI Example

```bash
# Create table
aws --endpoint-url http://localhost:4566 dynamodb create-table \
  --table-name users \
  --key-schema AttributeName=pk,KeyType=HASH \
  --attribute-definitions AttributeName=pk,AttributeType=S \
  --billing-mode PAY_PER_REQUEST

# Put item
aws --endpoint-url http://localhost:4566 dynamodb put-item \
  --table-name users \
  --item '{"pk": {"S": "user#1"}, "name": {"S": "Alice"}}'

# Get item
aws --endpoint-url http://localhost:4566 dynamodb get-item \
  --table-name users \
  --key '{"pk": {"S": "user#1"}}'

# Scan
aws --endpoint-url http://localhost:4566 dynamodb scan --table-name users
```

## Known Limitations

- Global Secondary Indexes (GSI) and Local Secondary Indexes (LSI) are accepted in `CreateTable` but queries cannot target them by `IndexName`.
- Conditional expressions (`ConditionExpression`) are not enforced on `PutItem` / `DeleteItem` / `UpdateItem`.
- TTL (Time to Live) configuration is accepted but items are not automatically expired.
- PartiQL (`ExecuteStatement`, `BatchExecuteStatement`) is not implemented.
