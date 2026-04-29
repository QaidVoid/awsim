# Amazon Neptune

Neptune's control plane shares the RDS API surface and signs requests with the `rds` signing name. AWSim handles Neptune calls via the existing `awsim-rds` service — there's no separate Neptune endpoint or crate. The Neptune Data API (graph queries) is not implemented.

**Endpoint:** `http://localhost:4566`
**Signing name:** `rds` (Neptune SDK uses RDS for sigv4)
**Protocol:** AWS-Query

## Operations

Use the standard RDS cluster + instance operations with `Engine: neptune`:

| Operation | Engine field |
|-----------|--------------|
| `CreateDBCluster` | `neptune` |
| `CreateDBInstance` | `neptune` |
| `DescribeDBClusters` / `DescribeDBInstances` | filter as usual |
| `DeleteDBCluster` / `DeleteDBInstance` | standard |
| `ModifyDBCluster` / `ModifyDBInstance` | standard |

## Behavior notes

- Default engine version: `1.3.1.0`. Default port: `8182`.
- Cluster endpoint pattern matches Aurora.
- The Neptune Data API (`POST /sparql`, `POST /gremlin`, `POST /openCypher`)
  is **not** emulated — only the control plane works.

## Example

```bash
aws --endpoint-url http://localhost:4566 rds create-db-cluster \
  --db-cluster-identifier graph-cluster \
  --engine neptune \
  --master-username admin --master-user-password supersecret
```
