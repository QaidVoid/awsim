# Glue

AWS Glue ETL (Extract, Transform, Load) service and Data Catalog for managing databases, tables, crawlers, and jobs.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `glue` |
| Persistence | No |

## Operations

### Databases
- `CreateDatabase` — create a database in the Glue Data Catalog
- `GetDatabase` — get a specific database by name
- `GetDatabases` — list all databases in the catalog
- `DeleteDatabase` — delete a database and optionally its tables
- `UpdateDatabase` — update database properties

### Tables
- `CreateTable` — create a table in a Glue database
- `GetTable` — get a specific table by database and name
- `GetTables` — list tables in a database
- `DeleteTable` — delete a table
- `UpdateTable` — update table schema or properties

### Crawlers
- `CreateCrawler` — create a crawler to discover and catalog data sources
- `GetCrawler` — get crawler details
- `GetCrawlers` — list all crawlers
- `DeleteCrawler` — delete a crawler
- `StartCrawler` — start a crawler run
- `StopCrawler` — stop a running crawler

### Jobs
- `CreateJob` — create an ETL job definition
- `GetJob` — get job details by name
- `GetJobs` — list all ETL jobs
- `DeleteJob` — delete a job definition

## Example

```bash
# Create a Glue database
aws --endpoint-url http://localhost:4567 \
  glue create-database \
  --database-input '{"Name":"my_catalog_db","Description":"My database"}'

# Create a table
aws --endpoint-url http://localhost:4567 \
  glue create-table \
  --database-name my_catalog_db \
  --table-input '{"Name":"events","StorageDescriptor":{"Columns":[{"Name":"id","Type":"string"},{"Name":"ts","Type":"bigint"}],"Location":"s3://my-bucket/events/","InputFormat":"org.apache.hadoop.mapred.TextInputFormat"}}'

# Create a crawler
aws --endpoint-url http://localhost:4567 \
  glue create-crawler \
  --name my-crawler \
  --role arn:aws:iam::000000000000:role/GlueRole \
  --database-name my_catalog_db \
  --targets '{"S3Targets":[{"Path":"s3://my-bucket/data/"}]}'

# Create an ETL job
aws --endpoint-url http://localhost:4567 \
  glue create-job \
  --name my-etl-job \
  --role arn:aws:iam::000000000000:role/GlueRole \
  --command '{"Name":"glueetl","ScriptLocation":"s3://my-bucket/scripts/job.py"}'
```

## Notes

- Glue in AWSim manages catalog metadata (databases, tables, crawlers, jobs) but does not execute ETL code or run actual crawl jobs.
- `StartCrawler` transitions the crawler to `RUNNING` and then `READY` state but does not discover any data.
- Job runs are not implemented — use AWSim Glue for IaC and SDK integration testing only.
- State is in-memory only and lost on restart.
