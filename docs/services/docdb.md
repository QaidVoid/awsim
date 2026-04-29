# Amazon DocumentDB

DocumentDB shares the RDS API surface and signs requests with the `rds` signing name. AWSim handles DocumentDB calls via the existing `awsim-rds` service — there's no separate DocDB endpoint or crate.

**Endpoint:** `http://localhost:4566`
**Signing name:** `rds` (DocumentDB SDK uses RDS for sigv4)
**Protocol:** AWS-Query

## Operations

Use the standard RDS cluster + instance operations with `Engine: docdb`:

| Operation | Engine field |
|-----------|--------------|
| `CreateDBCluster` | `docdb` |
| `CreateDBInstance` | `docdb` |
| `DescribeDBClusters` / `DescribeDBInstances` | filter as usual |
| `DeleteDBCluster` / `DeleteDBInstance` | standard |
| `ModifyDBCluster` / `ModifyDBInstance` | standard |

## Behavior notes

- Default engine version: `5.0.0`. Default port: `27017`.
- The cluster endpoint follows the same naming pattern as Aurora:
  `{cluster}.cluster.awsim.{region}.rds.localhost`.
- Cross-region global cluster operations work via `CreateGlobalCluster` — same
  flow as Aurora Global Database.

## Example

```bash
aws --endpoint-url http://localhost:4566 rds create-db-cluster \
  --db-cluster-identifier orders-docdb \
  --engine docdb \
  --master-username admin --master-user-password supersecret
```
