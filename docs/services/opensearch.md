# OpenSearch

Amazon OpenSearch Service compatible Elasticsearch REST API for full-text search, indexing, and analytics.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | Elasticsearch REST (not an AWS API protocol) |
| Signing Name | N/A |
| Persistence | No |

OpenSearch in AWSim is **not** an AWS-protocol service. It exposes an Elasticsearch-compatible REST API mounted at `/opensearch/` on the AWSim server.

## Operations

### Cluster
- `GET /` — cluster info (version, name, cluster UUID)
- `GET /_cluster/health` — cluster health status (green/yellow/red)
- `GET /_tasks/{task_id}` — get a task by ID
- `GET /_cat/indices` — list all indices in cat format

### Index Operations
- `PUT /{index}` — create an index (with optional mapping)
- `GET /{index}` — get index settings and mappings
- `HEAD /{index}` — check if an index exists (200 or 404)
- `DELETE /{index}` — delete an index
- `GET /{index}/_mapping` — get the mapping for an index
- `GET/POST /{index}/_count` — count documents matching a query

### Document Operations
- `PUT /{index}/_doc/{id}` — index a document with a specific ID
- `POST /{index}/_doc/{id}` — index or replace a document
- `POST /{index}/_doc` — index a document with an auto-generated ID
- `GET /{index}/_doc/{id}` — get a document by ID
- `DELETE /{index}/_doc/{id}` — delete a document
- `GET /{index}/_source/{id}` — get the raw `_source` of a document
- `POST /{index}/_update/{id}` — partially update a document
- `POST /{index}/_update_by_query` — update documents matching a query

### Search
- `POST /{index}/_search` — search with a query DSL body
- `GET /{index}/_search` — search with query parameters
- `POST /_msearch` — multi-search across multiple indices
- `POST /{index}/_msearch` — multi-search scoped to an index

### Bulk Operations
- `POST /_bulk` — bulk index/update/delete operations
- `POST /{index}/_bulk` — bulk operations scoped to an index

### Aliases and Reindex
- `POST /_aliases` — add or remove index aliases
- `POST /_reindex` — copy documents from one index to another

## Example

```bash
# Check cluster health
curl http://localhost:4567/opensearch/_cluster/health

# Create an index
curl -X PUT http://localhost:4567/opensearch/my-index \
  -H "Content-Type: application/json" \
  -d '{"mappings":{"properties":{"title":{"type":"text"},"price":{"type":"float"}}}}'

# Index a document
curl -X PUT http://localhost:4567/opensearch/my-index/_doc/1 \
  -H "Content-Type: application/json" \
  -d '{"title":"AWSim","price":0.0,"description":"Free AWS emulator"}'

# Search for documents
curl -X POST http://localhost:4567/opensearch/my-index/_search \
  -H "Content-Type: application/json" \
  -d '{"query":{"match":{"title":"AWSim"}}}'

# Bulk insert
curl -X POST http://localhost:4567/opensearch/_bulk \
  -H "Content-Type: application/x-ndjson" \
  --data-binary $'{"index":{"_index":"my-index","_id":"2"}}\n{"title":"Second doc","price":9.99}\n'
```

## Notes

- OpenSearch is mounted at `/opensearch/` prefix, **not** at the root. All requests must include this prefix.
- No AWS SigV4 signing is required — standard HTTP requests work directly.
- Search supports `match`, `match_all`, `term`, `terms`, `range`, `bool` (`must`, `should`, `must_not`, `filter`) query types.
- Aggregations are not supported in the current implementation.
- The `_reindex` operation copies documents between in-memory indices; no S3 or external data is accessed.
- State is in-memory only and lost on restart.
