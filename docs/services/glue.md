# Glue

AWS Glue ETL (Extract, Transform, Load) service and Data Catalog for managing databases, tables, crawlers, and jobs.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `glue` |
| Target Prefix | `AWSGlue` |
| Persistence | No |

## Quick Start

Create a Glue database, add a table, and define a crawler:

```bash
# Create a database in the Glue Data Catalog
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSGlue.CreateDatabase" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/glue/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"DatabaseInput":{"Name":"analytics","Description":"Analytics data catalog","LocationUri":"s3://my-data-bucket/"}}'

# Create a table in that database
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSGlue.CreateTable" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/glue/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"DatabaseName":"analytics","TableInput":{"Name":"events","Description":"User events","StorageDescriptor":{"Columns":[{"Name":"user_id","Type":"string"},{"Name":"event_type","Type":"string"},{"Name":"timestamp","Type":"bigint"}],"Location":"s3://my-data-bucket/events/","InputFormat":"org.apache.hadoop.mapred.TextInputFormat","OutputFormat":"org.apache.hadoop.hive.ql.io.HiveIgnoreKeyTextOutputFormat","SerdeInfo":{"SerializationLibrary":"org.apache.hadoop.hive.serde2.lazy.LazySimpleSerDe"}}}}'
```

## Operations

### Databases
- `CreateDatabase` — create a database in the Glue Data Catalog
  - Input: `DatabaseInput` object with `Name` (required), `Description`, `LocationUri`, `Parameters`
  - Returns: empty response (HTTP 200)

- `GetDatabase` — get a specific database by name
  - Input: `Name`
  - Returns: `Database` with `Name`, `Description`, `LocationUri`, `CreateTime`

- `GetDatabases` — list all databases in the catalog
  - Input: optional `NextToken`, `MaxResults`
  - Returns: paginated `DatabaseList`

- `DeleteDatabase` — delete a database and optionally its tables
  - Input: `Name`

- `UpdateDatabase` — update database properties
  - Input: `Name`, `DatabaseInput`

### Tables
- `CreateTable` — create a table in a Glue database
  - Input: `DatabaseName`, `TableInput` (with `Name`, `StorageDescriptor` containing `Columns`, `Location`, `InputFormat`, `SerdeInfo`)
  - Returns: empty response

- `GetTable` — get a specific table by database and name
  - Input: `DatabaseName`, `Name`
  - Returns: `Table` with full schema including `StorageDescriptor`

- `GetTables` — list tables in a database
  - Input: `DatabaseName`, optional `NextToken`, `MaxResults`
  - Returns: paginated `TableList`

- `DeleteTable` — delete a table
  - Input: `DatabaseName`, `Name`

- `UpdateTable` — update table schema or properties
  - Input: `DatabaseName`, `TableInput`

### Crawlers
- `CreateCrawler` — create a crawler to discover and catalog data sources
  - Input: `Name` (required), `Role` (IAM role ARN), `DatabaseName`, `Targets` (`{S3Targets: [{Path: "s3://..."}]}`)
  - Returns: empty response; crawler starts in `READY` state

- `GetCrawler` — get crawler details and current state
  - Input: `Name`
  - Returns: `Crawler` with `Name`, `State` (`READY`, `RUNNING`, `STOPPING`), `LastCrawl`

- `GetCrawlers` — list all crawlers

- `StartCrawler` — start a crawler run
  - Input: `Name`
  - Transitions: `READY` → `RUNNING` → `READY`

- `StopCrawler` — stop a running crawler
  - Input: `Name`

- `DeleteCrawler` — delete a crawler

### Jobs
- `CreateJob` — create an ETL job definition
  - Input: `Name` (required), `Role` (IAM role ARN), `Command` (`{Name: "glueetl", ScriptLocation: "s3://..."}`)
  - Returns: `Name`

- `GetJob` — get job details by name
  - Input: `JobName`
  - Returns: `Job` with `Name`, `Role`, `Command`, `MaxCapacity`

- `GetJobs` — list all ETL jobs

- `DeleteJob` — delete a job definition

## Curl Examples

```bash
# 1. List all databases
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSGlue.GetDatabases" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/glue/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{}'

# 2. Create an ETL job
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSGlue.CreateJob" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/glue/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"events-etl","Role":"arn:aws:iam::000000000000:role/GlueRole","Command":{"Name":"glueetl","ScriptLocation":"s3://my-scripts/transform.py","PythonVersion":"3"},"MaxCapacity":2.0}'

# 3. Start a crawler
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSGlue.StartCrawler" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/glue/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"my-crawler"}'
```

## SDK Example

```typescript
import {
  GlueClient,
  CreateDatabaseCommand,
  CreateTableCommand,
  CreateCrawlerCommand,
  GetTablesCommand,
} from '@aws-sdk/client-glue';

const glue = new GlueClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create database
await glue.send(new CreateDatabaseCommand({
  DatabaseInput: {
    Name: 'analytics',
    Description: 'Analytics data catalog',
  },
}));

// Create table with schema
await glue.send(new CreateTableCommand({
  DatabaseName: 'analytics',
  TableInput: {
    Name: 'events',
    StorageDescriptor: {
      Columns: [
        { Name: 'user_id', Type: 'string' },
        { Name: 'event_type', Type: 'string' },
        { Name: 'created_at', Type: 'timestamp' },
        { Name: 'metadata', Type: 'map<string,string>' },
      ],
      Location: 's3://my-data-bucket/events/',
      InputFormat: 'org.apache.hadoop.mapred.TextInputFormat',
      OutputFormat: 'org.apache.hadoop.hive.ql.io.HiveIgnoreKeyTextOutputFormat',
      SerdeInfo: {
        SerializationLibrary: 'org.openx.data.jsonserde.JsonSerDe',
        Parameters: { 'serialization.format': '1' },
      },
    },
  },
}));

// List tables
const { TableList } = await glue.send(new GetTablesCommand({
  DatabaseName: 'analytics',
}));
console.log('Tables:', TableList?.map(t => t.Name));

// Create crawler
await glue.send(new CreateCrawlerCommand({
  Name: 'data-crawler',
  Role: 'arn:aws:iam::000000000000:role/GlueRole',
  DatabaseName: 'analytics',
  Targets: {
    S3Targets: [{ Path: 's3://my-data-bucket/' }],
  },
}));
```

## Behavior Notes

- Glue in AWSim manages catalog metadata (databases, tables, crawlers, jobs) but does **not** execute ETL code or run actual crawl jobs.
- `StartCrawler` transitions the crawler state `READY` → `RUNNING` → `READY` quickly (simulated) but does not discover or catalog any data from S3 or other sources.
- Job runs (`StartJobRun`) are not implemented — Glue jobs are for metadata/IaC testing only.
- The Glue Data Catalog is shared across services — Athena references the same catalog when listing databases.
- State is in-memory only and lost on restart.
