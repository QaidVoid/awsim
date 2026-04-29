# Amazon QLDB

Ledger metadata only — the journal / Ion query data plane is not implemented. This is enough for Terraform/CDK templates that provision QLDB ledgers as part of broader stacks.

**Endpoint:** `http://localhost:4566`
**Signing name:** `qldb`
**Protocol:** REST-JSON

## Operations

| Operation | Method / Path |
|-----------|--------------|
| `CreateLedger` | `POST /ledgers` |
| `DescribeLedger` | `GET /ledgers/{name}` |
| `ListLedgers` | `GET /ledgers` |
| `UpdateLedger` | `PATCH /ledgers/{name}` |
| `DeleteLedger` | `DELETE /ledgers/{name}` |
| `TagResource` / `UntagResource` / `ListTagsForResource` | `/tags/{resourceArn}` |

## Behavior notes

- `CreateLedger` requires `Name` and `PermissionsMode`. `DeletionProtection` defaults to `true`.
- `DeleteLedger` rejects with `ResourcePreconditionNotMetException` while `DeletionProtection` is on — flip it via `UpdateLedger` first.
- `State` is `ACTIVE` immediately after Create; the emulator never spends time in `CREATING` / `DELETING`.
- Tag CRUD only persists tags on a known ledger — unknown ARNs return success silently.
