# Persistence

By default, AWSim is stateless — all data lives in memory and is lost when the process exits. Enable persistence with `--data-dir`.

## Enabling Persistence

```bash
./awsim --data-dir /var/lib/awsim
# or
AWSIM_DATA_DIR=/var/lib/awsim ./awsim
```

AWSim creates the directory if it does not exist.

## Services That Persist

The following services write and restore snapshots:

| Service | Signing Name |
|---------|-------------|
| SQS | `sqs` |
| DynamoDB | `dynamodb` |
| IAM | `iam` |
| S3 | `s3` |
| RDS | `rds` |
| Cognito User Pools | `cognito-idp` |
| Cognito Identity Pools | `cognito-identity` |
| ACM | `acm` |
| WAF | `wafv2` |
| Scheduler | `scheduler` |
| SNS | `sns` |
| Lambda | `lambda` |
| ECR | `ecr` |
| CloudWatch Logs | `logs` |

Services not in this list (e.g., KMS, Secrets Manager) are always in-memory only.

## Named Snapshots

Beyond the automatic save-on-shutdown / restore-on-startup flow,
AWSim supports point-in-time **named snapshots** — bundles of every
service's serialised state plus billing + chaos rules — so you can
freeze a complex test scenario and restore it on demand.

```bash
# Save the current state under a name.
awsim snapshot save baseline

# Make changes (apply chaos, create resources, …)
awsim chaos clear
aws --endpoint-url http://localhost:4566 s3 mb s3://scratch

# Restore — overwrites live state for any service captured in the snapshot.
awsim snapshot load baseline

# Inspect what's saved.
awsim snapshot list

# Drop one when you're done.
awsim snapshot delete baseline
```

Snapshots live under `{data_dir}/named-snapshots/{NAME}/` and the
HTTP API is also available directly:

| Method | Path |
| --- | --- |
| GET | `/_awsim/snapshots` |
| POST | `/_awsim/snapshots/{name}` |
| POST | `/_awsim/snapshots/{name}/load` |
| DELETE | `/_awsim/snapshots/{name}` |

**Limitations (v1):** named snapshots only capture
JSON-serialisable handler state. DynamoDB rows (SQLite) and
body-store payloads (S3 object bytes, Lambda code, SQS message
bodies) are *not* in the bundle — buckets/queues/tables are
recreated on load but their contents are not. This is good enough
for sharing topology, IAM, Cognito and chaos scenarios; deeper
bundling is on the roadmap.

`awsim snapshot` requires `--data-dir` to be set on the running
server.

**Note on S3:** S3 bucket metadata and object metadata are persisted via the JSON snapshot. Object bodies (the raw bytes) are persisted separately to disk under `{data_dir}/s3/` whenever `--data-dir` is supplied — see [S3 object bodies](#s3-object-bodies) below.

**Note on SQS:** SQS queue metadata is persisted via the JSON snapshot. Message bodies are written separately to disk under `{data_dir}/sqs/` whenever `--data-dir` is supplied — see [SQS message bodies](#sqs-message-bodies) below.

**Note on DynamoDB:** Only table schema metadata rides in the JSON snapshot. Items live in a dedicated SQLite database at `{data_dir}/dynamodb.db` — see [DynamoDB SQLite store](#dynamodb-sqlite-store) below.

## S3 object bodies

When `--data-dir` is set, the S3 service writes each `PutObject`, `CopyObject`, and assembled multipart upload to disk through a body store rooted at `{data_dir}/s3/`:

```
/var/lib/awsim/
  s3/
    objects/
      <bucket>/<key>
    multipart/
      <bucket>/<upload-id>/<part-number>
```

Object metadata still rides in the regular `s3.json` snapshot. On restore, each object is wired up to its on-disk path and bytes are read lazily by `GetObject` rather than preloaded — keeping startup cheap even for large datasets. `DeleteObject`, `DeleteBucket`, `AbortMultipartUpload`, and `CompleteMultipartUpload` clean up their files on a best-effort basis (failures are logged via `tracing`).

If a body file is missing on disk after a restart (for example, the data directory was partially wiped), `GetObject` returns `NoSuchKey` for that object.

When `--data-dir` is not supplied, the service stays fully in-memory and object bodies are lost on shutdown.

## DynamoDB SQLite store

Unlike the other services, DynamoDB does not persist its items through the JSON snapshot. Items are written directly to a single SQLite database at `{data_dir}/dynamodb.db` (or a per-process tempfile when `--data-dir` is unset). One database serves every account/region — partitioning is handled by `(account, region, table_name)` columns on the `items` table.

```
/var/lib/awsim/
  dynamodb.db          # WAL-mode sqlite, items + table-schema rows
  dynamodb.db-wal
  dynamodb.db-shm
  snapshots/
    dynamodb.json      # only table schemas / stream config / tags
```

### Why SQLite

The original implementation stored every item in an in-memory `BTreeMap` per table; bulk imports of millions of rows pushed the AWSim process well past 10 GiB of resident memory. SQLite gives us:

- **Bounded memory.** Items are never loaded into the process; reads stream a single partition at a time.
- **Indexed lookups for GSIs.** Up to five GSI key column pairs (`gsi1_pk`, `gsi1_sk`, ..., `gsi5_pk`, `gsi5_sk`) are materialized at write time and covered by partial indexes (`WHERE gsiN_pk IS NOT NULL`), matching DynamoDB's sparse-index semantics.
- **Real ACID transactions.** `TransactWriteItems` runs phase 1 (validate every condition) and phase 2 (apply every mutation) inside one `BEGIN IMMEDIATE` transaction. A failed condition or any sqlite error rolls back the entire batch via `Drop`.
- **Snapshot-consistent batch reads.** `TransactGetItems` runs inside a deferred transaction so multi-row reads see the same commit point.

### What the JSON snapshot still carries

`{data_dir}/snapshots/dynamodb.json` is the source of truth for everything sqlite *doesn't* hold: table key schema, attribute definitions, GSI / LSI definitions, stream configuration (enabled, ARN, view type, sequence counter, the bounded ring buffer of recent change records), TTL settings, and tags. On restore, the schema is rehydrated into the in-memory `DashMap` and also mirrored into sqlite's `tables` table so a fresh process started without the JSON snapshot can still bootstrap from sqlite alone.

### `TruncateTable`

The awsim-only `TruncateTable` op clears every item in a table while leaving schema, indexes, and stream config intact — backed by a single `DELETE FROM items WHERE account=? AND region=? AND table_name=?`. Useful for "reset between tests" loops in the admin UI; not available in real DynamoDB.

When `--data-dir` is unset, `dynamodb.db` is created in `std::env::temp_dir()` with a per-process UUID suffix and dies with the process.

## Lambda function code

When `--data-dir` is set, the Lambda service writes each function's zip bytes to disk under `{data_dir}/lambda/`:

```
/var/lib/awsim/
  lambda/
    <function-name>/
      $LATEST          # current editable code
      1                # published version 1
      2                # published version 2
```

`CreateFunction` and `UpdateFunctionCode` write the current code to `$LATEST`. `PublishVersion` snapshots the current bytes into a per-version file (named after the version number) so each published version keeps an immutable copy independent of further edits to `$LATEST`. `DeleteFunction` removes the entire `{function-name}` subtree on a best-effort basis.

The `lambda.json` snapshot stores function metadata only (configuration, version metadata, aliases). On restore, each function's `code` field is rebound to its on-disk path; bytes are read lazily by `Invoke` rather than preloaded. Invocation history is intentionally not persisted.

When `--data-dir` is not supplied, function code stays in memory and is lost on shutdown.

## SQS message bodies

When `--data-dir` is set, the SQS service writes each accepted message body to disk under `{data_dir}/sqs/`:

```
/var/lib/awsim/
  sqs/
    <queue-name>/
      <message-id>
```

`SendMessage` and `SendMessageBatch` write the body to `{data_dir}/sqs/{queue}/{message_id}` and store an on-disk reference on the in-memory message; `ReceiveMessage` reads the bytes back lazily when responding. `DeleteMessage` and `DeleteMessageBatch` remove the per-message blob; `PurgeQueue` and `DeleteQueue` drop the entire queue subtree. When a message is redriven to a configured DLQ, its blob is migrated from the source queue's bucket to the DLQ's bucket so it survives source-queue cleanup. All cleanup is best-effort and failures are logged via `tracing` rather than failing the API call.

The `sqs.json` snapshot stores queue and message metadata only — body bytes for on-disk messages are omitted from the snapshot. On restore, each message's body is rebound to its on-disk path. If a body file is missing on disk after restart, `ReceiveMessage` returns an internal error for that message rather than fabricating an empty body.

When `--data-dir` is not supplied, message bodies stay in memory and are lost on shutdown.

## ECR layers

When `--data-dir` is set, the ECR service writes each completed layer's bytes to disk under `{data_dir}/ecr/`:

```
/var/lib/awsim/
  ecr/
    <repository>/
      sha256:abc...    # layer body, named by digest
```

`CompleteLayerUpload` finalizes an upload, hashes the buffered bytes into a sha256 digest, and writes them to `{data_dir}/ecr/{repository}/{digest}`. Repository and image metadata still ride in the regular `ecr.json` snapshot — the snapshot only stores layer digest, size, and media type, never the bytes. On restore, each layer is rebound to its on-disk path; bytes are read lazily by the `/v2/{repo}/blobs/{digest}` HTTP endpoint.

`BatchDeleteImage` parses each removed image manifest and best-effort deletes any referenced layer blobs. `DeleteRepository` best-effort removes the entire `{repository}/` subtree.

In-progress upload buffers are kept in memory only; if AWSim crashes mid-upload the partial data is lost (the client retries from `InitiateLayerUpload`).

When `--data-dir` is not supplied, layer bodies stay in memory and are lost on shutdown.

## CloudWatch Logs events

When `--data-dir` is set, the CloudWatch Logs service appends each `PutLogEvents` batch to a per-stream JSON-lines file under `{data_dir}/cloudwatch-logs/`:

```
/var/lib/awsim/
  cloudwatch-logs/
    <log-group-name>/
      <log-stream-name>      # JSONL: one event per line
```

Each line is a JSON object of the form `{"ts":<timestamp>,"msg":<message>,"ing":<ingestion-time>}`. A single `PutLogEvents` API call performs one `append`, regardless of batch size. Group and stream metadata still rides in the regular `logs.json` snapshot — the snapshot omits the events themselves. On restore, each stream's JSONL file is parsed and replayed into the in-memory `Vec<LogEvent>` (malformed lines are skipped with a warning). `DeleteLogStream` best-effort removes the per-stream file; `DeleteLogGroup` removes the entire log-group subtree. Append-write failures are logged via `tracing` and do not fail the API call.

The in-memory event log still grows unbounded (the disk file is a durability layer, not a memory cap). Combine with `--max-blob-bytes` to bound the on-disk side; FIFO eviction may delete the JSONL file of the oldest stream, in which case its events disappear from disk but stay in memory until restart.

When `--data-dir` is not supplied, log events stay in memory and are lost on shutdown.

## Snapshot Format

Snapshots are written to `{data_dir}/snapshots/` as JSON files, one per service:

```
/var/lib/awsim/
  snapshots/
    s3.json
    dynamodb.json
    sqs.json
    iam.json
    ...
```

## Auto-Save

AWSim saves snapshots every **30 seconds** while running.

## Graceful Shutdown

When AWSim receives `SIGINT` (Ctrl+C) or `SIGTERM`, it saves all snapshots before exiting. This ensures data written in the last interval is not lost.

## Atomic Writes

Snapshots are written atomically: AWSim writes to a temporary file first, then renames it to the final path. This prevents corrupt snapshots from a mid-write crash.

## Restoring State

On startup, AWSim reads each `{data_dir}/snapshots/{service}.json` file and restores the service state. If a snapshot file is missing or malformed, AWSim starts that service with empty state and logs a warning.

## Garbage Collection

After snapshot restore, AWSim sweeps each persisted service's body store for orphaned files — disk blobs that no longer correspond to anything in the in-memory state. Orphans typically appear after a process crash, an out-of-band file deletion, or any other abnormal shutdown that left the snapshot and the body store out of sync.

The GC walks only the directories it owns:

| Service | Body store root | Groups (top-level subdirs) |
|---------|-----------------|----------------------------|
| S3 | `{data_dir}/s3/` | `objects`, `multipart` |
| Lambda | `{data_dir}/` | `lambda` |
| SQS | `{data_dir}/` | `sqs` |
| ECR | `{data_dir}/` | `ecr` |
| CloudWatch Logs | `{data_dir}/` | `cloudwatch-logs` |

Each service's GC pass deletes any file under its groups whose `(group, bucket, key)` triple is not present in the restored in-memory inventory, then collapses any empty bucket and group directories. The `{data_dir}/snapshots/` directory and any other top-level paths are never touched.

A short summary is logged for each service:

```
INFO BodyStore GC reclaimed orphaned blobs service="s3" deleted=70 freed_bytes=12345678
```

To opt out, pass `--no-gc` (or set `AWSIM_NO_GC=1`). Disabling GC leaves orphaned files in place; they accumulate until the next GC-enabled startup.

### Periodic GC

By default, the orphan sweep runs only at startup. Pass `--gc-interval-secs <N>` (or set `AWSIM_GC_INTERVAL_SECS=N`) to also re-run it every `N` seconds in the background:

```bash
./awsim --data-dir /var/lib/awsim --gc-interval-secs 300
```

Each iteration walks the same per-service inventories as the startup sweep and logs a one-line summary per service. The flag is opt-in; leaving it unset preserves the current "startup only" behavior.

## Disk space limit

Long-running services with high write volume — large S3 uploads, busy SQS queues, frequent Lambda code updates, and pushes to ECR — can grow the body store unbounded. Pass `--max-blob-bytes <N>` (or set `AWSIM_MAX_BLOB_BYTES=N`) to cap each persisted service's body store at `N` bytes:

```bash
./awsim --data-dir /var/lib/awsim --max-blob-bytes 1073741824   # 1 GiB per service
```

The cap is applied independently to S3, Lambda, SQS, ECR, and CloudWatch Logs — each service may use up to `N` bytes. When a `write_blob` would push a service over its cap, AWSim deletes the oldest files (by modification time) until the new write fits, then writes the new blob.

Eviction caveats:

- Evicted blobs are removed from disk but their metadata still lives in the in-memory inventory (and the next snapshot). Subsequent `GetObject`, `ReceiveMessage`, `Invoke`, or layer-blob fetches for an evicted blob return `NoSuchKey` / "missing body" errors. The cap takes precedence over data integrity.
- A single write larger than the cap fails immediately with an out-of-space error after attempting eviction.
- The cap is per-service, not global. To bound the total directory, divide your overall budget across services and set the smallest reasonable `--max-blob-bytes`.

When the flag is unset, body stores grow without limit (the current default).
