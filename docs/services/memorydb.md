# Amazon MemoryDB for Redis

In-memory metadata for MemoryDB clusters, users, ACLs, snapshots, subnet groups, and parameter groups. AWSim does not actually back the cluster with a Redis process — `Status` flips to `available` immediately after `CreateCluster` and the synthesized `ClusterEndpoint` resolves to a fake hostname.

**Endpoint:** `http://localhost:4566`
**Signing name:** `memorydb`
**Protocol:** AWS-JSON 1.1 (X-Amz-Target prefix: `AmazonMemoryDB`)

## Operations

| Group | Operations |
|-------|-----------|
| Clusters | `CreateCluster`, `DescribeClusters`, `UpdateCluster`, `DeleteCluster` |
| Users | `CreateUser`, `DescribeUsers`, `UpdateUser`, `DeleteUser` |
| ACLs | `CreateACL`, `DescribeACLs`, `DeleteACL` |
| Subnet groups | `CreateSubnetGroup`, `DescribeSubnetGroups` |
| Parameter groups | `CreateParameterGroup`, `DescribeParameterGroups` |
| Snapshots | `CreateSnapshot`, `DescribeSnapshots`, `DeleteSnapshot` |

## Behavior notes

- Default engine version is `7.1`; default port `6379`. `TLSEnabled` defaults to `true`.
- Duplicate `ClusterName` / `UserName` / `ACLName` returns the AWS-style `*AlreadyExistsFault` error.
- The synthesized `ClusterEndpoint.Address` follows `clustercfg.{ClusterName}.{region}.memorydb.amazonaws.com`.
- Per-shard sharding is metadata-only — `Shards: []` is returned.
