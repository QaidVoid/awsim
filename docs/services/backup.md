# AWS Backup

Backup vault, plan, selection, and job emulation. The job lifecycle is fast-forwarded — `StartBackupJob` returns a job that is already `COMPLETED`, so callers don't have to poll. Recovery-point bookkeeping is light: each job increments the parent vault's `NumberOfRecoveryPoints`.

**Endpoint:** `http://localhost:4566`
**Signing name:** `backup`
**Protocol:** REST-JSON

## Operations

| Operation | Method / Path |
|-----------|--------------|
| `CreateBackupVault` / `DescribeBackupVault` / `DeleteBackupVault` / `ListBackupVaults` | `/backup-vaults[...]` |
| `PutBackupVaultLockConfiguration` / `DeleteBackupVaultLockConfiguration` | `/backup-vaults/{Name}/vault-lock` |
| `CreateBackupPlan` / `GetBackupPlan` / `ListBackupPlans` / `DeleteBackupPlan` / `UpdateBackupPlan` | `/backup/plans[...]` |
| `CreateBackupSelection` / `GetBackupSelection` / `ListBackupSelections` / `DeleteBackupSelection` | `/backup/plans/{Id}/selections[...]` |
| `StartBackupJob` / `DescribeBackupJob` / `ListBackupJobs` | `/backup-jobs[...]` |

`ListBackupJobs` accepts `ByBackupVaultName` and `ByState` query filters.

## Behavior notes

- `DeleteBackupVault` rejects when the vault still has recovery points (`InvalidRequestException`).
- `DeleteBackupPlan` cascades to delete every selection attached to that plan.
- `UpdateBackupPlan` increments the plan's `VersionId` (`{prefix}_{N}`) on every successful call.
- `StartBackupJob` infers `ResourceType` from the resource ARN (`DynamoDB`, `S3`, `EFS`, `RDS`, `EBS`, or `Unknown`).
- The vault lock fields (`MinRetentionDays`, `MaxRetentionDays`) round-trip but are not enforced at the recovery-point level.
