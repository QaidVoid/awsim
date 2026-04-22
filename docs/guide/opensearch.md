# OpenSearch

AWSim includes a built-in OpenSearch / Elasticsearch-compatible REST API mounted at the `/opensearch/` path prefix. It is not a real AWS OpenSearch Service endpoint — it does not use the AWS signing protocol. It speaks the Elasticsearch REST API directly.

## Base URL

```
http://localhost:4566/opensearch/
```

All standard Elasticsearch API paths are supported under this prefix.

## Supported Endpoints

### Cluster

| Endpoint | Description |
|----------|-------------|
| `GET /opensearch/` | Cluster info |
| `GET /opensearch/_cluster/health` | Cluster health |
| `GET /opensearch/_tasks/{task_id}` | Task status |
| `GET /opensearch/_cat/indices` | List all indices |
| `POST /opensearch/_aliases` | Manage aliases |
| `POST /opensearch/_reindex` | Reindex |
| `POST /opensearch/_bulk` | Bulk operations |
| `POST /opensearch/_msearch` | Multi-search |

### Index Management

| Endpoint | Description |
|----------|-------------|
| `PUT /opensearch/{index}` | Create index |
| `GET /opensearch/{index}` | Get index info |
| `HEAD /opensearch/{index}` | Check index exists |
| `DELETE /opensearch/{index}` | Delete index |
| `GET /opensearch/{index}/_mapping` | Get mapping |

### Document Operations

| Endpoint | Description |
|----------|-------------|
| `PUT /opensearch/{index}/_doc/{id}` | Index a document (with ID) |
| `POST /opensearch/{index}/_doc/{id}` | Index a document (with ID) |
| `POST /opensearch/{index}/_doc` | Index a document (auto-ID) |
| `GET /opensearch/{index}/_doc/{id}` | Get document |
| `DELETE /opensearch/{index}/_doc/{id}` | Delete document |
| `GET /opensearch/{index}/_source/{id}` | Get document source |
| `POST /opensearch/{index}/_update/{id}` | Update document |
| `POST /opensearch/{index}/_update_by_query` | Update by query |
| `POST /opensearch/{index}/_bulk` | Bulk operations on index |

### Search

| Endpoint | Description |
|----------|-------------|
| `POST /opensearch/{index}/_search` | Search |
| `GET /opensearch/{index}/_search` | Search (GET with body) |
| `POST /opensearch/{index}/_count` | Count matching documents |
| `GET /opensearch/{index}/_count` | Count matching documents |
| `POST /opensearch/{index}/_msearch` | Multi-search on index |

## Supported Query Types

- `match_all` — matches every document
- `match` — full-text search on a single field
- `multi_match` — full-text search across multiple fields
- `bool` — compound queries with `must`, `should`, `filter`
- `term` — exact-value match
- `query_string` — simple query string syntax

## Examples

```bash
# Create an index
curl -X PUT http://localhost:4566/opensearch/products \
  -H "Content-Type: application/json" \
  -d '{"mappings": {"properties": {"name": {"type": "text"}, "price": {"type": "float"}}}}'

# Index a document
curl -X POST http://localhost:4566/opensearch/products/_doc \
  -H "Content-Type: application/json" \
  -d '{"name": "Widget", "price": 9.99}'

# Search
curl -X POST http://localhost:4566/opensearch/products/_search \
  -H "Content-Type: application/json" \
  -d '{
    "query": {
      "match": { "name": "Widget" }
    }
  }'

# Bool query
curl -X POST http://localhost:4566/opensearch/products/_search \
  -H "Content-Type: application/json" \
  -d '{
    "query": {
      "bool": {
        "must": [
          { "match": { "name": "Widget" } }
        ],
        "filter": [
          { "term": { "price": 9.99 } }
        ]
      }
    }
  }'

# Bulk insert
curl -X POST http://localhost:4566/opensearch/_bulk \
  -H "Content-Type: application/x-ndjson" \
  --data-binary '{"index": {"_index": "products", "_id": "1"}}
{"name": "Widget A", "price": 9.99}
{"index": {"_index": "products", "_id": "2"}}
{"name": "Widget B", "price": 19.99}
'
```

## Alias Support

```bash
# Create an alias
curl -X POST http://localhost:4566/opensearch/_aliases \
  -H "Content-Type: application/json" \
  -d '{
    "actions": [
      { "add": { "index": "products-v2", "alias": "products" } }
    ]
  }'

# Search via alias
curl -X POST http://localhost:4566/opensearch/products/_search \
  -H "Content-Type: application/json" \
  -d '{"query": {"match_all": {}}}'
```

## Wildcard Index Patterns

Search supports wildcard index patterns:

```bash
curl -X POST "http://localhost:4566/opensearch/logs-*/_search" \
  -H "Content-Type: application/json" \
  -d '{"query": {"match_all": {}}}'
```

## Notes

- This is not the AWS OpenSearch Service API — you do not sign requests.
- Mappings are stored but not enforced during indexing. Any JSON document can be indexed in any index.
- Aggregations are not yet supported.
- The `from` / `size` pagination parameters are supported. The default size is 10.
