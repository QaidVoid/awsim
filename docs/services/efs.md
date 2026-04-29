# Amazon EFS

In-memory file system metadata for tests that touch EFS through Terraform, CDK, or the SDK. AWSim does not actually back files with bytes; the service tracks file systems, mount targets, access points, and lifecycle/backup policies so descriptive operations return the right shapes.

**Endpoint:** `http://localhost:4566`
**Signing name:** `elasticfilesystem`
**Protocol:** REST-JSON

## Operations

| Operation | Method / Path |
|-----------|--------------|
| `CreateFileSystem` | `POST /2015-02-01/file-systems` |
| `DescribeFileSystems` | `GET /2015-02-01/file-systems` |
| `DeleteFileSystem` | `DELETE /2015-02-01/file-systems/{FileSystemId}` |
| `UpdateFileSystem` | `PUT /2015-02-01/file-systems/{FileSystemId}` |
| `PutLifecycleConfiguration` / `DescribeLifecycleConfiguration` | `/2015-02-01/file-systems/{FileSystemId}/lifecycle-configuration` |
| `PutBackupPolicy` / `DescribeBackupPolicy` | `/2015-02-01/file-systems/{FileSystemId}/backup-policy` |
| `CreateMountTarget` / `DescribeMountTargets` / `DeleteMountTarget` | `/2015-02-01/mount-targets[...]` |
| `DescribeMountTargetSecurityGroups` / `ModifyMountTargetSecurityGroups` | `/2015-02-01/mount-targets/{MountTargetId}/security-groups` |
| `CreateAccessPoint` / `DescribeAccessPoints` / `DeleteAccessPoint` | `/2015-02-01/access-points[...]` |
| `TagResource` / `UntagResource` / `ListTagsForResource` | `/2015-02-01/resource-tags/{ResourceId}` |

## Behavior notes

- `CreateFileSystem` and `CreateAccessPoint` are idempotent on `CreationToken` / `ClientToken`: a second call with the same token returns the existing resource.
- `DeleteFileSystem` is rejected with `FileSystemInUse` if the file system still has mount targets.
- File-system tags can come in via `Tags` on create; `Name` is mirrored to the `Name` attribute on the file system.
- The mount-target IP address is fixed at `10.0.0.10` unless `IpAddress` is supplied; AZ id/name are derived from the request region.
- `PerformanceMode` defaults to `generalPurpose`; `ThroughputMode` defaults to `bursting`.
