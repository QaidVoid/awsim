# Kendra

Amazon Kendra intelligent enterprise search service for indexing and querying documents with natural language understanding.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `kendra` |
| Persistence | No |

## Operations

### Index Management
- `CreateIndex` — create a new Kendra index
- `DescribeIndex` — get index details and status
- `ListIndices` — list all indexes in the account/region
- `DeleteIndex` — delete an index and all its documents
- `UpdateIndex` — update index name, description, or capacity

### Data Sources
- `CreateDataSource` — create a data source connector (S3, SharePoint, etc.)
- `ListDataSources` — list data sources for an index
- `DeleteDataSource` — delete a data source

### Documents
- `BatchPutDocument` — add or update documents in an index
- `BatchDeleteDocument` — remove documents from an index by ID

### Search
- `Query` — search the index with a natural language or keyword query
- `Retrieve` — retrieve specific passages from documents in the index
- `SubmitFeedback` — submit relevance feedback to improve search results

## Example

```bash
# Create an index
aws --endpoint-url http://localhost:4567 \
  kendra create-index \
  --name my-search-index \
  --role-arn arn:aws:iam::000000000000:role/KendraRole \
  --edition DEVELOPER_EDITION

# Add documents to the index
aws --endpoint-url http://localhost:4567 \
  kendra batch-put-document \
  --index-id <index-id> \
  --documents '[
    {"Id":"doc1","Title":"AWS Kendra","Content":{"DataContent":"Kendra is an intelligent search service.","DataContentType":"PLAIN_TEXT"}}
  ]'

# Search the index
aws --endpoint-url http://localhost:4567 \
  kendra query \
  --index-id <index-id> \
  --query-text "What is Kendra?"
```

## Notes

- AWSim's Kendra uses **substring-based matching** for search — documents containing the query terms are returned.
- The `Query` operation returns matching document excerpts ranked by relevance (number of matched terms).
- `Retrieve` returns passage-level excerpts from matching documents.
- Index status transitions to `ACTIVE` immediately after creation.
- State is in-memory only and lost on restart.
