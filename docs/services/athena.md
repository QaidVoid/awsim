# Athena

Amazon Athena interactive SQL query service for analyzing data in S3 using standard SQL.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `athena` |
| Target Prefix | `AmazonAthena` |
| Persistence | No |

## Quick Start

Submit a query, poll for completion, then retrieve results:

```bash
# Start a query execution
EXEC_ID=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonAthena.StartQueryExecution" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/athena/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"QueryString":"SELECT * FROM my_table LIMIT 10","QueryExecutionContext":{"Database":"my_db"},"ResultConfiguration":{"OutputLocation":"s3://my-bucket/results/"}}' \
  | jq -r '.QueryExecutionId')

echo "Execution ID: $EXEC_ID"

# Get query status (returns SUCCEEDED immediately in AWSim)
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonAthena.GetQueryExecution" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/athena/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"QueryExecutionId\":\"$EXEC_ID\"}"
```

## Operations

### Workgroups
- `CreateWorkGroup` — create a workgroup with configuration settings
  - Input: `Name` (required), `Configuration` (output location, encryption, query result settings), `Description`, `Tags`
  - Returns: empty response

- `DeleteWorkGroup` — delete a workgroup (must be empty of active queries)
  - Input: `WorkGroup`, `RecursiveDeleteOption` (boolean, delete all contained queries)

- `GetWorkGroup` — get workgroup details and configuration
  - Input: `WorkGroup`
  - Returns: `WorkGroup` object with `State` (`ENABLED` or `DISABLED`), `Configuration`

- `ListWorkGroups` — list all workgroups
  - Returns: paginated `WorkGroups` list; `primary` is always present

- `UpdateWorkGroup` — update workgroup description, state, or output location
  - Input: `WorkGroup`, optional `Description`, `State` (`ENABLED`/`DISABLED`), `ConfigurationUpdates.ResultConfigurationUpdates.OutputLocation`

### Query Executions
- `StartQueryExecution` — submit a SQL query for execution
  - Input: `QueryString` (SQL), `QueryExecutionContext` (`Database`), `ResultConfiguration` (`OutputLocation` as `s3://` URI), optional `WorkGroup`
  - Returns: `QueryExecutionId` (UUID)

- `GetQueryExecution` — get the status and metadata of a query execution
  - Input: `QueryExecutionId`
  - Returns: `QueryExecution` with `Status.State` (`QUEUED`, `RUNNING`, `SUCCEEDED`, `FAILED`), `Statistics` (execution time, data scanned)

- `GetQueryResults` — retrieve the result set of a completed query
  - Input: `QueryExecutionId`, optional `MaxResults`, `NextToken`
  - Returns: `ResultSet` with `Rows` and `ResultSetMetadata.ColumnInfo`

- `ListQueryExecutions` — list query execution IDs
  - Input: optional `WorkGroup`, `MaxResults`, `NextToken`
  - Returns: paginated `QueryExecutionIds` list

- `StopQueryExecution` — cancel a running query
  - Input: `QueryExecutionId`

- `BatchGetQueryExecution` — retrieve multiple query executions in a single call
  - Input: `QueryExecutionIds` (list of UUIDs)
  - Returns: `QueryExecutions` (found), `UnprocessedQueryExecutionIds` (not found)

### Named Queries
- `CreateNamedQuery` — save a named SQL query for reuse
  - Input: `Name`, `Database`, `QueryString`, optional `Description`, `WorkGroup`
  - Returns: `NamedQueryId`

- `GetNamedQuery` — retrieve a named query by ID
  - Input: `NamedQueryId`
  - Returns: full query object

- `ListNamedQueries` — list named queries
  - Input: optional `WorkGroup`, `MaxResults`, `NextToken`

- `DeleteNamedQuery` — delete a named query
  - Input: `NamedQueryId`

- `BatchGetNamedQuery` — retrieve multiple named queries in a single call
  - Input: `NamedQueryIds` (list of UUIDs)
  - Returns: `NamedQueries` (found), `UnprocessedNamedQueryIds` (not found)

### Databases
- `ListDatabases` — list databases in a data catalog
  - Input: `CatalogName` (use `AwsDataCatalog`)
  - Returns: `DatabaseList`

- `GetDatabase` — get details of a specific database
  - Input: `CatalogName`, `DatabaseName`

### Data Catalogs
- `ListDataCatalogs` — list available data catalogs
  - Returns: `DataCatalogsSummary`; always includes the built-in `AwsDataCatalog` (type `GLUE`)

- `GetDataCatalog` — get details of a data catalog
  - Input: `Name`
  - Returns: `DataCatalog` with `CatalogName`, `Type`, `Description`, `Parameters`

- `CreateDataCatalog` — create a custom data catalog
  - Input: `Name`, `Type` (`LAMBDA`, `GLUE`, or `HIVE`), optional `Description`, `Parameters`

- `DeleteDataCatalog` — delete a data catalog
  - Input: `Name`

### Prepared Statements
- `CreatePreparedStatement` — save a parameterized SQL statement for reuse
  - Input: `StatementName`, `WorkGroup`, `QueryStatement`, optional `Description`
  - Returns: empty response

- `GetPreparedStatement` — retrieve a prepared statement
  - Input: `StatementName`, `WorkGroup`
  - Returns: `PreparedStatement` with `QueryStatement`, `LastModifiedTime`

- `ListPreparedStatements` — list prepared statements in a workgroup
  - Input: `WorkGroup`
  - Returns: `PreparedStatements` list with name and modification time

- `DeletePreparedStatement` — delete a prepared statement
  - Input: `StatementName`, `WorkGroup`

### Table Metadata
- `GetTableMetadata` — retrieve metadata for a table in a catalog (stub)
  - Input: `CatalogName`, `DatabaseName`, `TableName`
  - Returns: `TableMetadata` with `Name`, `TableType`, empty `Columns` and `PartitionKeys`

- `ListTableMetadata` — list table metadata for a database (stub returning empty list)
  - Input: `CatalogName`, `DatabaseName`

## Curl Examples

```bash
# 1. Start a query
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonAthena.StartQueryExecution" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/athena/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"QueryString":"SELECT count(*) as cnt FROM events","QueryExecutionContext":{"Database":"analytics"},"ResultConfiguration":{"OutputLocation":"s3://my-results/"}}'

# 2. Create a named query for repeated use
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonAthena.CreateNamedQuery" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/athena/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"daily-event-count","Database":"analytics","QueryString":"SELECT date, count(*) FROM events GROUP BY date","Description":"Daily event count rollup"}'

# 3. List all query executions
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonAthena.ListQueryExecutions" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/athena/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{}'
```

## SDK Example

```typescript
import {
  AthenaClient,
  StartQueryExecutionCommand,
  GetQueryExecutionCommand,
  GetQueryResultsCommand,
} from '@aws-sdk/client-athena';

const athena = new AthenaClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Start a query
const { QueryExecutionId } = await athena.send(new StartQueryExecutionCommand({
  QueryString: 'SELECT * FROM events LIMIT 100',
  QueryExecutionContext: { Database: 'analytics' },
  ResultConfiguration: { OutputLocation: 's3://my-bucket/results/' },
}));

// Check status
const { QueryExecution } = await athena.send(new GetQueryExecutionCommand({
  QueryExecutionId,
}));

console.log('State:', QueryExecution?.Status?.State); // SUCCEEDED

// Retrieve results
const { ResultSet } = await athena.send(new GetQueryResultsCommand({
  QueryExecutionId,
}));

console.log('Columns:', ResultSet?.ResultSetMetadata?.ColumnInfo?.map(c => c.Name));
console.log('Rows:', ResultSet?.Rows?.length);
```

## Behavior Notes

- AWSim's Athena does **not** execute SQL — queries are accepted, recorded as `SUCCEEDED`, and return empty or mock result sets immediately.
- A built-in `primary` workgroup is always available and cannot be deleted.
- The `OutputLocation` in `ResultConfiguration` is recorded but no files are written to S3.
- `ListDatabases` and `GetDatabase` are stubs returning minimal metadata.
- Query execution times in `Statistics` are simulated (not real measurements).
- `ListDataCatalogs` always includes the built-in `AwsDataCatalog` (type `GLUE`).
- `GetTableMetadata` and `ListTableMetadata` are stubs; real table schemas are not tracked.
- Prepared statements are scoped by workgroup — the same name can exist in different workgroups.
- State is in-memory only and lost on restart.
