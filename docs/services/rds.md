# RDS

Amazon Relational Database Service for managing relational database instances and clusters.

For the Aurora HTTP SQL endpoint, see the [RDS Data API](rds-data.md), an opt-in
service backed by a real PostgreSQL via Docker.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsQuery` |
| Signing Name | `rds` |
| Persistence | Yes |

RDS uses the `AwsQuery` protocol: `POST` requests with `Content-Type: application/x-www-form-urlencoded` and an `Action=` parameter.

## Quick Start

Create a database instance, describe it, and stop it:

```bash
# Create a DB instance
aws --endpoint-url http://localhost:4566 \
  rds create-db-instance \
  --db-instance-identifier mydb \
  --db-instance-class db.t3.micro \
  --engine mysql \
  --master-username admin \
  --master-user-password password123 \
  --allocated-storage 20 \
  --db-name myappdb

# Describe the instance
aws --endpoint-url http://localhost:4566 \
  rds describe-db-instances \
  --db-instance-identifier mydb

# Stop the instance
aws --endpoint-url http://localhost:4566 \
  rds stop-db-instance \
  --db-instance-identifier mydb
```

## Operations

### DB Instances
- `CreateDBInstance` — create a new database instance
  - Input: `DBInstanceIdentifier` (required, unique name), `DBInstanceClass` (e.g., `db.t3.micro`, `db.r5.large`), `Engine` (`mysql`, `postgres`, `mariadb`, `oracle-se2`, `sqlserver-se`, `aurora-mysql`, `aurora-postgresql`), `MasterUsername`, `MasterUserPassword`, `AllocatedStorage` (GB), `DBName`, `VpcSecurityGroupIds`, `DBSubnetGroupName`, `PubliclyAccessible`, `MultiAZ`, `EngineVersion`, `StorageType` (`gp2`, `io1`, `standard`)
  - Returns: `DBInstance` with `DBInstanceIdentifier`, `DBInstanceStatus` (`creating` → `available`), `Endpoint` (`{Address, Port}`), `DBInstanceArn`
  - Aurora engines require a `DBClusterIdentifier` naming an existing cluster. The instance joins that cluster's member list and inherits its master username, engine version, and storage; the first instance to join is the writer and later instances are read replicas.

- `DeleteDBInstance` — delete a database instance
  - Input: `DBInstanceIdentifier`, optional `SkipFinalSnapshot`, `FinalDBSnapshotIdentifier`

- `DescribeDBInstances` — list database instances with optional filter
  - Input: optional `DBInstanceIdentifier`, `Filters`, `MaxRecords`, `Marker`
  - Returns: paginated `DBInstances` list

- `ModifyDBInstance` — update instance configuration (class, storage, multi-AZ, etc.)
  - Input: `DBInstanceIdentifier`, optional `DBInstanceClass`, `AllocatedStorage`, `ApplyImmediately`, `BackupRetentionPeriod`, `PreferredMaintenanceWindow`

- `StartDBInstance` — start a stopped database instance
  - Input: `DBInstanceIdentifier`
  - Status transitions: `stopped` → `starting` → `available`

- `StopDBInstance` — stop a running database instance
  - Input: `DBInstanceIdentifier`
  - Status transitions: `available` → `stopping` → `stopped`

- `RebootDBInstance` — reboot a database instance (useful after parameter group changes)
  - Input: `DBInstanceIdentifier`, optional `ForceFailover`

### DB Clusters (Aurora)
- `CreateDBCluster` — create an Aurora database cluster
  - Input: `DBClusterIdentifier`, `Engine` (`aurora`, `aurora-mysql`, `aurora-postgresql`), `MasterUsername`, `MasterUserPassword`, `DatabaseName`, `VpcSecurityGroupIds`
  - Returns: `DBCluster` with `DBClusterIdentifier`, `Status`, `Endpoint`, `ReaderEndpoint`

- `DeleteDBCluster` — delete a database cluster
  - Input: `DBClusterIdentifier`, optional `SkipFinalSnapshot`
  - Rejected with `InvalidParameterCombination` when the cluster has deletion protection enabled

- `DescribeDBClusters` — list database clusters with optional filter
  - Input: optional `DBClusterIdentifier`, `Filters`, `MaxRecords`, `Marker`

- `ModifyDBCluster` updates a cluster's configuration
  - Input: `DBClusterIdentifier`, optional `BackupRetentionPeriod`, `PreferredBackupWindow`, `PreferredMaintenanceWindow`, `Port`, `EngineVersion`, `VpcSecurityGroupIds`, `DeletionProtection`, `ApplyImmediately`
  - `DeletionProtection`, `PreferredMaintenanceWindow`, and security groups apply immediately; the rest follow `ApplyImmediately`, staging under `PendingModifiedValues` and flushing during the maintenance window when deferred

- `StartDBCluster` starts a stopped cluster and its member instances
  - Input: `DBClusterIdentifier`
  - Status transitions: `stopped` to `available`

- `StopDBCluster` stops a running cluster and its member instances
  - Input: `DBClusterIdentifier`
  - Status transitions: `available` to `stopped`

- `RebootDBCluster` reboots a cluster
  - Input: `DBClusterIdentifier`

- `FailoverDBCluster` promotes a reader to writer
  - Input: `DBClusterIdentifier`, optional `TargetDBInstanceIdentifier` (promotes the named member, otherwise the next reader)

### DB Subnet Groups
- `CreateDBSubnetGroup` — create a subnet group for database placement
  - Input: `DBSubnetGroupName`, `DBSubnetGroupDescription`, `SubnetIds` (list)

- `DeleteDBSubnetGroup` — delete a subnet group

- `DescribeDBSubnetGroups` — list subnet groups
  - Input: optional `DBSubnetGroupName`, `Filters`

### DB Parameter Groups
- `CreateDBParameterGroup` — create a parameter group for database configuration
  - Input: `DBParameterGroupName`, `DBParameterGroupFamily` (e.g., `mysql8.0`, `postgres15`), `Description`

- `DeleteDBParameterGroup` — delete a parameter group

- `DescribeDBParameterGroups` — list parameter groups

### DB Cluster Parameter Groups (Aurora)
- `CreateDBClusterParameterGroup` creates a cluster-level parameter group
  - Input: `DBClusterParameterGroupName`, `DBParameterGroupFamily` (e.g., `aurora-postgresql16`, `aurora-mysql8.0`), `Description`
- `DescribeDBClusterParameterGroups` lists cluster parameter groups
  - Input: optional `DBClusterParameterGroupName`
- `DeleteDBClusterParameterGroup` deletes a cluster parameter group
  - Input: `DBClusterParameterGroupName`
- `DescribeDBClusterParameters` returns the resolved parameter list for a group
  - Input: `DBClusterParameterGroupName`, optional `Source` (`user` or `engine-default`)
  - Returns: engine defaults for the family with caller overrides applied, each tagged with its `Source`
- `ModifyDBClusterParameterGroup` overrides one or more parameter values
  - Input: `DBClusterParameterGroupName`, `Parameters` (list of `{ParameterName, ParameterValue, ApplyMethod}`)
- `ResetDBClusterParameterGroup` returns parameters to their engine defaults
  - Input: `DBClusterParameterGroupName`, optional `ResetAllParameters`, `Parameters`

### DB Snapshots
- `CreateDBSnapshot` — create a snapshot from an existing instance
  - Input: `DBSnapshotIdentifier`, `DBInstanceIdentifier`
  - Returns: `DBSnapshot` with `Status` (`available`) immediately

- `DeleteDBSnapshot` — delete a snapshot
  - Input: `DBSnapshotIdentifier`

- `DescribeDBSnapshots` — list snapshots with optional filter
  - Input: optional `DBSnapshotIdentifier`, `DBInstanceIdentifier`
  - Returns: `DBSnapshots` list

- `CopyDBSnapshot` — copy snapshot metadata to a new identifier (stub)
  - Input: `SourceDBSnapshotIdentifier`, `TargetDBSnapshotIdentifier`

- `RestoreDBInstanceFromDBSnapshot` rebuilds an instance from a snapshot
  - Input: `DBInstanceIdentifier`, `DBSnapshotIdentifier`, optional `DBInstanceClass`, `StorageType`, `MultiAZ`, `PubliclyAccessible`, `DBSubnetGroupName`, `VpcSecurityGroupIds`
  - Engine, version, storage, master username, and encryption are inherited from the snapshot

### DB Cluster Snapshots (Aurora)
- `CreateDBClusterSnapshot` creates a manual snapshot of an Aurora cluster
  - Input: `DBClusterSnapshotIdentifier`, `DBClusterIdentifier`, optional `Tags`
  - Returns: `DBClusterSnapshot` with `Status` (`available`), inheriting the cluster's engine, version, and master username
- `DescribeDBClusterSnapshots` lists cluster snapshots with optional filters
  - Input: optional `DBClusterSnapshotIdentifier`, `DBClusterIdentifier`, `SnapshotType`, `MaxRecords`, `Marker`
- `DeleteDBClusterSnapshot` deletes a cluster snapshot
  - Input: `DBClusterSnapshotIdentifier`
- `CopyDBClusterSnapshot` copies a cluster snapshot to a new identifier
  - Input: `SourceDBClusterSnapshotIdentifier`, `TargetDBClusterSnapshotIdentifier`, optional `KmsKeyId`, `SourceRegion`, `Tags`
- `RestoreDBClusterFromSnapshot` rebuilds an Aurora cluster from a cluster snapshot
  - Input: `DBClusterIdentifier`, `SnapshotIdentifier`, `Engine`, optional `EngineVersion`, `VpcSecurityGroupIds`, `BackupRetentionPeriod`, `DeletionProtection`
  - The restored cluster inherits the snapshot's engine version and master username and starts with no members

### Engine Versions & Options
- `DescribeDBEngineVersions` returns available engine versions for `postgres`, `mysql`, `mariadb`, `aurora-postgresql`, and `aurora-mysql`
  - Input: optional `Engine`, `EngineVersion` filters
  - Returns: list of engine versions with `DBParameterGroupFamily`, `Status`, and Aurora capability flags (`SupportsGlobalDatabases`, `SupportsParallelQuery`)

- `DescribeOrderableDBInstanceOptions` returns available instance classes per engine
  - Input: `Engine`, optional `EngineVersion`
  - Returns: instance classes with their storage type options. Aurora engines return cluster-capable classes (including `db.serverless`) backed by `aurora` storage and report `SupportsClusters`; standalone engines return `db.t3.micro` through `db.r5.4xlarge` on `gp2`/`io1`/`standard` storage

### DB Cluster Endpoints
- `DescribeDBClusterEndpoints` — list writer, reader, and custom cluster endpoints
  - Input: optional `DBClusterIdentifier`, `DBClusterEndpointIdentifier`

- `CreateDBClusterEndpoint` — create a custom cluster endpoint
  - Input: `DBClusterIdentifier`, `DBClusterEndpointIdentifier`, `EndpointType`

- `DeleteDBClusterEndpoint` — delete a custom cluster endpoint
  - Input: `DBClusterEndpointIdentifier`

### Stubs
- `DescribeEventSubscriptions` — returns empty list
- `DescribeDBLogFiles` — returns empty list

### Tags
- `AddTagsToResource` — add tags to any RDS resource (instance, cluster, subnet group, etc.) by ARN
- `RemoveTagsFromResource` — remove tags from an RDS resource
- `ListTagsForResource` — list tags on an RDS resource

## Curl Examples

```bash
# 1. Create a PostgreSQL instance
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/rds/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=CreateDBInstance' \
  --data-urlencode 'DBInstanceIdentifier=mypostgres' \
  --data-urlencode 'DBInstanceClass=db.t3.small' \
  --data-urlencode 'Engine=postgres' \
  --data-urlencode 'MasterUsername=dbadmin' \
  --data-urlencode 'MasterUserPassword=SuperSecret123!' \
  --data-urlencode 'AllocatedStorage=20' \
  --data-urlencode 'DBName=appdb' \
  --data-urlencode 'EngineVersion=15.4'

# 2. Describe all instances
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/rds/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=DescribeDBInstances'

# 3. Tag an instance
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/rds/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=AddTagsToResource' \
  --data-urlencode 'ResourceName=arn:aws:rds:us-east-1:000000000000:db:mydb' \
  --data-urlencode 'Tags.member.1.Key=environment' \
  --data-urlencode 'Tags.member.1.Value=staging'
```

## SDK Example

```typescript
import {
  RDSClient,
  CreateDBInstanceCommand,
  DescribeDBInstancesCommand,
  ModifyDBInstanceCommand,
  StopDBInstanceCommand,
} from '@aws-sdk/client-rds';

const rds = new RDSClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create DB instance
const { DBInstance } = await rds.send(new CreateDBInstanceCommand({
  DBInstanceIdentifier: 'mydb',
  DBInstanceClass: 'db.t3.micro',
  Engine: 'mysql',
  MasterUsername: 'admin',
  MasterUserPassword: 'password123',
  AllocatedStorage: 20,
  DBName: 'myapp',
  Tags: [{ Key: 'environment', Value: 'staging' }],
}));

console.log('Instance ID:', DBInstance?.DBInstanceIdentifier);
console.log('Status:', DBInstance?.DBInstanceStatus); // creating
console.log('ARN:', DBInstance?.DBInstanceArn);

// Describe instances
const { DBInstances } = await rds.send(new DescribeDBInstancesCommand({
  DBInstanceIdentifier: 'mydb',
}));

const instance = DBInstances?.[0];
console.log('Endpoint:', instance?.Endpoint?.Address, ':', instance?.Endpoint?.Port);

// Modify instance
await rds.send(new ModifyDBInstanceCommand({
  DBInstanceIdentifier: 'mydb',
  AllocatedStorage: 50,
  ApplyImmediately: true,
}));

// Stop instance
await rds.send(new StopDBInstanceCommand({
  DBInstanceIdentifier: 'mydb',
}));
```

## Behavior Notes

- RDS in AWSim tracks instance metadata and state transitions only — **no actual database engine is started**.
- Connecting to the `Endpoint.Address` returned by RDS will fail — it is a placeholder address, not a real database server.
- Persistence is enabled: instances, clusters, subnet groups, and parameter groups survive AWSim restarts.
- Engine types (`mysql`, `postgres`, `aurora`, etc.) and engine versions are accepted without validation.
- Status transitions (`creating` → `available`) happen quickly (simulated); real AWS may take several minutes.
- `MultiAZ` instances are tracked but no actual failover or replication occurs.
