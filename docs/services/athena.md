# Athena

Amazon Athena interactive SQL query service for analyzing data in S3 using standard SQL.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `athena` |
| Persistence | No |

## Operations

### Workgroups
- `CreateWorkGroup` — create a workgroup with configuration settings
- `DeleteWorkGroup` — delete a workgroup
- `GetWorkGroup` — get workgroup details and configuration
- `ListWorkGroups` — list all workgroups

### Query Executions
- `StartQueryExecution` — submit a SQL query for execution
- `GetQueryExecution` — get the status and metadata of a query execution
- `GetQueryResults` — retrieve the result set of a completed query
- `ListQueryExecutions` — list query execution IDs with optional workgroup filter
- `StopQueryExecution` — cancel a running query execution

### Named Queries
- `CreateNamedQuery` — save a named SQL query for reuse
- `GetNamedQuery` — retrieve a named query by ID
- `ListNamedQueries` — list named queries with optional workgroup filter
- `DeleteNamedQuery` — delete a named query

### Databases
- `ListDatabases` — list databases in a data catalog
- `GetDatabase` — get details of a specific database

## Example

```bash
# Start a query
aws --endpoint-url http://localhost:4567 \
  athena start-query-execution \
  --query-string "SELECT * FROM my_table LIMIT 10" \
  --query-execution-context '{"Database":"my_db"}' \
  --result-configuration '{"OutputLocation":"s3://my-bucket/results/"}'

# Check query status
aws --endpoint-url http://localhost:4567 \
  athena get-query-execution \
  --query-execution-id <execution-id>

# Get results
aws --endpoint-url http://localhost:4567 \
  athena get-query-results \
  --query-execution-id <execution-id>

# Create a named query
aws --endpoint-url http://localhost:4567 \
  athena create-named-query \
  --name daily-report \
  --database my_db \
  --query-string "SELECT count(*) FROM events WHERE date = '2024-01-01'"
```

## Notes

- AWSim's Athena does not actually execute SQL — queries are accepted, recorded as `SUCCEEDED`, and return empty or mock result sets.
- A built-in `primary` workgroup is always available.
- The output location in `ResultConfiguration` is recorded but no files are written to S3.
- `ListDatabases` and `GetDatabase` are stub operations returning minimal metadata.
