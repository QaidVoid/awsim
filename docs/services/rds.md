# RDS

Amazon Relational Database Service for managing relational database instances and clusters.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsQuery` |
| Signing Name | `rds` |
| Persistence | Yes |

## Operations

### DB Instances
- `CreateDBInstance` — create a new database instance
- `DeleteDBInstance` — delete a database instance
- `DescribeDBInstances` — list database instances with optional filters
- `ModifyDBInstance` — update instance configuration (class, storage, etc.)
- `StartDBInstance` — start a stopped database instance
- `StopDBInstance` — stop a running database instance
- `RebootDBInstance` — reboot a database instance

### DB Clusters
- `CreateDBCluster` — create an Aurora database cluster
- `DeleteDBCluster` — delete a database cluster
- `DescribeDBClusters` — list database clusters with optional filters

### DB Subnet Groups
- `CreateDBSubnetGroup` — create a subnet group for database placement
- `DeleteDBSubnetGroup` — delete a subnet group
- `DescribeDBSubnetGroups` — list subnet groups

### DB Parameter Groups
- `CreateDBParameterGroup` — create a parameter group for database configuration
- `DeleteDBParameterGroup` — delete a parameter group
- `DescribeDBParameterGroups` — list parameter groups

### Tags
- `AddTagsToResource` — add tags to an RDS resource
- `RemoveTagsFromResource` — remove tags from an RDS resource
- `ListTagsForResource` — list tags on an RDS resource

## Example

```bash
# Create a DB instance
aws --endpoint-url http://localhost:4567 \
  rds create-db-instance \
  --db-instance-identifier mydb \
  --db-instance-class db.t3.micro \
  --engine mysql \
  --master-username admin \
  --master-user-password password123 \
  --allocated-storage 20

# Describe the instance
aws --endpoint-url http://localhost:4567 \
  rds describe-db-instances \
  --db-instance-identifier mydb

# Stop the instance
aws --endpoint-url http://localhost:4567 \
  rds stop-db-instance \
  --db-instance-identifier mydb
```

## Notes

- RDS in AWSim tracks instance metadata and state only — no actual database engine is started.
- Persistence is enabled: instances, clusters, subnet groups, and parameter groups survive restarts.
- RDS uses the `AwsQuery` protocol (form-encoded POST with `Action=` parameter).
- Engine types (`mysql`, `postgres`, `aurora`, etc.) are accepted but not validated against a supported list.
