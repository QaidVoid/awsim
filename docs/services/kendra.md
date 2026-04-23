# Kendra

Amazon Kendra intelligent enterprise search service for indexing and querying documents with natural language understanding.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `kendra` |
| Target Prefix | `kendra` |
| Persistence | No |

## Quick Start

Create an index, add documents, and search them:

```bash
# Create an index
INDEX_ID=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: kendra.CreateIndex" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kendra/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"my-search-index","RoleArn":"arn:aws:iam::000000000000:role/KendraRole","Edition":"DEVELOPER_EDITION"}' \
  | jq -r '.Id')

echo "Index ID: $INDEX_ID"

# Add documents
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: kendra.BatchPutDocument" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kendra/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"IndexId\":\"$INDEX_ID\",\"Documents\":[{\"Id\":\"doc1\",\"Title\":\"What is Kendra?\",\"Blob\":\"$(echo -n 'Amazon Kendra is an intelligent enterprise search service powered by machine learning.' | base64)\"},{\"Id\":\"doc2\",\"Title\":\"How to use Kendra\",\"Blob\":\"$(echo -n 'To use Kendra, create an index, add documents, and query using natural language.' | base64)\"}]}"

# Search
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: kendra.Query" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kendra/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"IndexId\":\"$INDEX_ID\",\"QueryText\":\"intelligent search\"}"
```

## Operations

### Index Management
- `CreateIndex` — create a new Kendra index
  - Input: `Name` (required), `RoleArn` (IAM role ARN), `Edition` (`DEVELOPER_EDITION` or `ENTERPRISE_EDITION`), optional `Description`, `Tags`
  - Returns: `Id` (UUID for the index)
  - Index status transitions to `ACTIVE` immediately in AWSim

- `DescribeIndex` — get index details and status
  - Input: `Id`
  - Returns: `Name`, `Id`, `Status` (`ACTIVE`), `Edition`, `DocumentMetadataConfigurations`, `IndexStatistics`

- `ListIndices` — list all indexes in the account/region
  - Input: optional `NextToken`, `MaxResults`
  - Returns: paginated `IndexConfigurationSummaryItems`

- `DeleteIndex` — delete an index and all its documents
  - Input: `Id`

- `UpdateIndex` — update index name, description, or capacity
  - Input: `Id`, optional `Name`, `Description`, `CapacityUnits`

### Data Sources
- `CreateDataSource` — create a data source connector
  - Input: `IndexId`, `Name`, `Type` (`S3`, `SHAREPOINT`, `DATABASE`, `SALESFORCE`, `CONFLUENCE`, `CUSTOM`), `Configuration`, `RoleArn`
  - Returns: `Id`

- `ListDataSources` — list data sources for an index
  - Input: `IndexId`, optional `NextToken`, `MaxResults`

- `DeleteDataSource` — delete a data source

### Documents
- `BatchPutDocument` — add or update documents in an index
  - Input: `IndexId`, `Documents` (list of `{Id, Title, Blob (base64 content), ContentType, S3Path, Attributes}`)
  - Returns: `FailedDocuments` (list of failed document IDs and reasons)

- `BatchDeleteDocument` — remove documents from an index by ID
  - Input: `IndexId`, `DocumentIdList` (list of document IDs)
  - Returns: `FailedDocuments`

### Search
- `Query` — search the index with a natural language or keyword query
  - Input: `IndexId`, `QueryText` (required), optional `AttributeFilter`, `Facets`, `RequestedDocumentAttributes`, `QueryResultTypeFilter` (`DOCUMENT`, `ANSWER`, `QUESTION_ANSWER`), `PageNumber`, `PageSize`
  - Returns: `ResultItems` (list with `Id`, `Type`, `DocumentTitle`, `DocumentExcerpt`, `DocumentId`, `ScoreAttributes`), `TotalNumberOfResults`

- `Retrieve` — retrieve specific passages from documents matching a query
  - Input: `IndexId`, `QueryText`, optional `AttributeFilter`, `RequestedDocumentAttributes`, `PageNumber`, `PageSize`
  - Returns: `ResultItems` (list with `Id`, `DocumentId`, `DocumentTitle`, `Content` (passage text), `DocumentAttributes`)

- `SubmitFeedback` — submit relevance feedback to improve search results
  - Input: `IndexId`, `QueryId`, `ClickFeedbackItems` or `RelevanceFeedbackItems`

## Curl Examples

```bash
# 1. Create an index
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: kendra.CreateIndex" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kendra/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"docs-index","RoleArn":"arn:aws:iam::000000000000:role/KendraRole","Edition":"DEVELOPER_EDITION","Description":"Documentation search index"}'

# 2. Add plain text documents
CONTENT=$(echo -n "AWS Lambda lets you run code without provisioning or managing servers." | base64)
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: kendra.BatchPutDocument" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kendra/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"IndexId\":\"YOUR_INDEX_ID\",\"Documents\":[{\"Id\":\"lambda-intro\",\"Title\":\"Lambda Introduction\",\"Blob\":\"$CONTENT\",\"ContentType\":\"PLAIN_TEXT\"}]}"

# 3. Search with Retrieve (passage-level)
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: kendra.Retrieve" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kendra/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"IndexId":"YOUR_INDEX_ID","QueryText":"serverless compute"}'
```

## SDK Example

```typescript
import {
  KendraClient,
  CreateIndexCommand,
  BatchPutDocumentCommand,
  QueryCommand,
} from '@aws-sdk/client-kendra';

const kendra = new KendraClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create index
const { Id: indexId } = await kendra.send(new CreateIndexCommand({
  Name: 'product-docs',
  RoleArn: 'arn:aws:iam::000000000000:role/KendraRole',
  Edition: 'DEVELOPER_EDITION',
}));

// Add documents
await kendra.send(new BatchPutDocumentCommand({
  IndexId: indexId!,
  Documents: [
    {
      Id: 'getting-started',
      Title: 'Getting Started Guide',
      Blob: Buffer.from('This guide covers installation, configuration, and first steps.'),
      ContentType: 'PLAIN_TEXT',
    },
    {
      Id: 'api-reference',
      Title: 'API Reference',
      Blob: Buffer.from('Comprehensive reference for all API endpoints and parameters.'),
      ContentType: 'PLAIN_TEXT',
    },
  ],
}));

// Search
const { ResultItems, TotalNumberOfResults } = await kendra.send(new QueryCommand({
  IndexId: indexId!,
  QueryText: 'how to get started',
  PageSize: 10,
}));

console.log('Total results:', TotalNumberOfResults);
ResultItems?.forEach(item => {
  console.log(`- ${item.DocumentTitle?.Text}: ${item.DocumentExcerpt?.Text}`);
});
```

## Behavior Notes

- AWSim's Kendra uses **substring-based matching** for search — documents containing the query terms are returned.
- `Query` results are ranked by number of matched terms (not semantic similarity or BM25).
- `Retrieve` returns passage-level excerpts from matching documents (substring of the document content).
- Index status transitions to `ACTIVE` immediately after creation — no indexing delay.
- Document `Blob` content is base64-encoded; use `ContentType: "PLAIN_TEXT"` for text documents.
- State is in-memory only and lost on restart.
