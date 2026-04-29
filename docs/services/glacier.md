# Amazon S3 Glacier

In-memory vault + archive store. Archives are uploaded as a single shot (multipart upload is not implemented). The job lifecycle is fast-forwarded — `InitiateJob` returns a job that is already `StatusCode: Succeeded` so callers don't have to poll.

**Endpoint:** `http://localhost:4566`
**Signing name:** `glacier`
**Protocol:** REST-JSON

## Operations

| Operation | Method / Path |
|-----------|--------------|
| `CreateVault` | `PUT /{accountId}/vaults/{vaultName}` |
| `DescribeVault` / `ListVaults` / `DeleteVault` | `/{accountId}/vaults[...]` |
| `UploadArchive` | `POST /{accountId}/vaults/{vaultName}/archives` |
| `DeleteArchive` | `DELETE /{accountId}/vaults/{vaultName}/archives/{archiveId}` |
| `InitiateJob` / `DescribeJob` / `ListJobs` | `/{accountId}/vaults/{vaultName}/jobs[...]` |
| `SetVaultNotifications` / `GetVaultNotifications` / `DeleteVaultNotifications` | `/{accountId}/vaults/{vaultName}/notification-configuration` |

## Behavior notes

- `DeleteVault` is rejected with `InvalidParameterValueException` when the vault still contains archives.
- `UploadArchive` accepts the body as a base64-encoded blob; the response includes the SHA-256 of the bytes (Glacier's tree-hash collapses to a flat hash here).
- `InitiateJob` echoes back `Type`, `ArchiveId`, `Description`, `SNSTopic`, and `Tier` from `jobParameters`, then immediately marks the job `Succeeded` / `Completed`.
- `Vault.NumberOfArchives` and `Vault.SizeInBytes` are kept in sync as archives are uploaded and deleted.
