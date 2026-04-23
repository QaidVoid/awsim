# OpenSearch

Amazon OpenSearch Service compatible Elasticsearch REST API for full-text search, indexing, and analytics.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | Elasticsearch REST (not an AWS API protocol) |
| Signing Name | N/A |
| Base URL | `http://localhost:4566/opensearch/` |
| Persistence | No |

OpenSearch in AWSim is **not** an AWS-protocol service. It exposes an Elasticsearch-compatible REST API mounted at `/opensearch/` on the AWSim server. **No `Authorization` header or AWS SigV4 signing is required.**

## Quick Start

Create an index, add documents, and search them:

```bash
# Check cluster health
curl http://localhost:4566/opensearch/_cluster/health | jq

# Create an index with a mapping
curl -X PUT http://localhost:4566/opensearch/products \
  -H "Content-Type: application/json" \
  -d '{"mappings":{"properties":{"name":{"type":"text"},"price":{"type":"float"},"category":{"type":"keyword"},"in_stock":{"type":"boolean"}}}}'

# Index a document with a specific ID
curl -X PUT http://localhost:4566/opensearch/products/_doc/1 \
  -H "Content-Type: application/json" \
  -d '{"name":"AWSim Local Stack","price":0.0,"category":"software","in_stock":true}'

# Index more documents
curl -X POST http://localhost:4566/opensearch/products/_doc \
  -H "Content-Type: application/json" \
  -d '{"name":"Pro Database Tool","price":49.99,"category":"software","in_stock":true}'

# Search for documents
curl -X POST http://localhost:4566/opensearch/products/_search \
  -H "Content-Type: application/json" \
  -d '{"query":{"match":{"name":"stack"}},"size":10}'
```

## Operations

### Cluster
- `GET /opensearch/` — cluster info: returns `name`, `cluster_name`, `cluster_uuid`, `version.number` (7.x compatible)
- `GET /opensearch/_cluster/health` — cluster health status
  - Returns: `status` (`green`), `number_of_nodes`, `number_of_data_nodes`, `active_shards`
- `GET /opensearch/_cat/indices` — list all indices in cat format (text, not JSON)
- `GET /opensearch/_tasks/{task_id}` — get a task by ID

### Index Operations
- `PUT /opensearch/{index}` — create an index
  - Body: optional `{"settings":{...},"mappings":{"properties":{...}}}`
  - Returns: `{"acknowledged":true,"index":"{index}"}`

- `GET /opensearch/{index}` — get index settings, mappings, and aliases
- `HEAD /opensearch/{index}` — check if an index exists (200 = exists, 404 = not found)
- `DELETE /opensearch/{index}` — delete an index and all its documents
- `GET /opensearch/{index}/_mapping` — get the current mapping for an index
- `GET/POST /opensearch/{index}/_count` — count documents matching a query

### Document Operations
- `PUT /opensearch/{index}/_doc/{id}` — index a document with a specific ID (creates or replaces)
  - Returns: `{"_index":"...","_id":"...","result":"created"/"updated","_seq_no":N}`

- `POST /opensearch/{index}/_doc` — index with an auto-generated ID
- `GET /opensearch/{index}/_doc/{id}` — get a document by ID
  - Returns: `{"_index":"...","_id":"...","found":true,"_source":{...}}`

- `DELETE /opensearch/{index}/_doc/{id}` — delete a document
- `GET /opensearch/{index}/_source/{id}` — get only the `_source` field (no metadata)
- `POST /opensearch/{index}/_update/{id}` — partially update a document
  - Body: `{"doc":{"field":"new_value"}}` — merges with existing document

- `POST /opensearch/{index}/_update_by_query` — update multiple documents matching a query

### Search
- `POST /opensearch/{index}/_search` — search with a full query DSL body
  - Body: `{"query":{...},"size":10,"from":0,"sort":[...],"_source":[...], "_id":true}`
  - Returns: `{"hits":{"total":{"value":N},"hits":[{"_id":"...","_score":1.0,"_source":{...}}]}}`
  - Supported query types: `match`, `match_all`, `term`, `terms`, `range`, `bool` (`must`, `should`, `must_not`, `filter`), `ids`, `wildcard`, `prefix`

- `GET /opensearch/{index}/_search` — search with URL query params (e.g., `?q=name:stack`)
- `POST /opensearch/_msearch` — multi-search (alternating header line + query body in ndjson)
- `POST /opensearch/{index}/_msearch` — multi-search scoped to an index

### Bulk Operations
- `POST /opensearch/_bulk` — bulk index/update/delete operations in ndjson format
  - Format: alternating action line + document line
  - Actions: `index`, `create`, `update`, `delete`

- `POST /opensearch/{index}/_bulk` — bulk operations scoped to an index

### Aliases and Reindex
- `POST /opensearch/_aliases` — add or remove index aliases
  - Body: `{"actions":[{"add":{"index":"my-index","alias":"my-alias"}},{"remove":{"index":"old","alias":"current"}}]}`

- `POST /opensearch/_reindex` — copy documents from one index to another
  - Body: `{"source":{"index":"source-index"},"dest":{"index":"dest-index"}}`

## Curl Examples

```bash
# 1. Bulk insert multiple documents
curl -X POST http://localhost:4566/opensearch/_bulk \
  -H "Content-Type: application/x-ndjson" \
  --data-binary $'{"index":{"_index":"logs","_id":"1"}}\n{"level":"INFO","message":"Server started","ts":1700000000}\n{"index":{"_index":"logs","_id":"2"}}\n{"level":"ERROR","message":"Connection refused","ts":1700000001}\n{"index":{"_index":"logs","_id":"3"}}\n{"level":"WARN","message":"High memory usage","ts":1700000002}\n'

# 2. Search with a boolean query
curl -X POST http://localhost:4566/opensearch/logs/_search \
  -H "Content-Type: application/json" \
  -d '{
    "query": {
      "bool": {
        "must": [{"match": {"message": "connection"}}],
        "filter": [{"term": {"level": "ERROR"}}]
      }
    }
  }'

# 3. Range query on a numeric field
curl -X POST http://localhost:4566/opensearch/products/_search \
  -H "Content-Type: application/json" \
  -d '{"query":{"range":{"price":{"gte":10,"lte":100}}},"sort":[{"price":"asc"}],"size":5}'

# 4. Multi-search across two indices
curl -X POST http://localhost:4566/opensearch/_msearch \
  -H "Content-Type: application/x-ndjson" \
  --data-binary $'{"index":"products"}\n{"query":{"match_all":{}},"size":3}\n{"index":"logs"}\n{"query":{"term":{"level":"ERROR"}}}\n'

# 5. Add an alias
curl -X POST http://localhost:4566/opensearch/_aliases \
  -H "Content-Type: application/json" \
  -d '{"actions":[{"add":{"index":"products","alias":"catalog"}}]}'
```

## SDK Example

```typescript
// OpenSearch is compatible with the Elasticsearch 7.x client
import { Client } from '@elastic/elasticsearch';

const es = new Client({
  node: 'http://localhost:4566/opensearch',
});

// Create index
await es.indices.create({
  index: 'articles',
  body: {
    mappings: {
      properties: {
        title: { type: 'text' },
        author: { type: 'keyword' },
        publishedAt: { type: 'date' },
        content: { type: 'text' },
        tags: { type: 'keyword' },
      },
    },
  },
});

// Bulk index
await es.bulk({
  body: [
    { index: { _index: 'articles', _id: '1' } },
    { title: 'Getting started with OpenSearch', author: 'Alice', tags: ['search', 'aws'] },
    { index: { _index: 'articles', _id: '2' } },
    { title: 'Advanced query DSL', author: 'Bob', tags: ['search', 'elasticsearch'] },
  ],
});

// Search
const { body } = await es.search({
  index: 'articles',
  body: {
    query: {
      bool: {
        must: [{ match: { title: 'OpenSearch' } }],
        filter: [{ term: { tags: 'search' } }],
      },
    },
    sort: [{ _score: 'desc' }],
    size: 10,
  },
});

console.log('Total:', body.hits.total.value);
body.hits.hits.forEach((hit: any) => {
  console.log(`${hit._id}: ${hit._source.title} (score: ${hit._score})`);
});
```

You can also use the `@opensearch-project/opensearch` client:

```typescript
import { Client } from '@opensearch-project/opensearch';

const client = new Client({ node: 'http://localhost:4566/opensearch' });

const response = await client.search({
  index: 'articles',
  body: { query: { match: { title: 'search' } } },
});
```

## Behavior Notes

- OpenSearch is mounted at `/opensearch/` prefix — **all requests must include this prefix**.
- No AWS SigV4 signing is required — standard HTTP requests work directly.
- Search supports `match`, `match_all`, `term`, `terms`, `range`, `bool` (`must`, `should`, `must_not`, `filter`), `ids`, `wildcard`, `prefix` query types.
- Aggregations are **not** supported in the current implementation.
- The `_reindex` operation copies documents between in-memory indices; no S3 or external data is accessed.
- `_update_by_query` updates in place but without scripting support.
- State is in-memory only and lost on restart.
