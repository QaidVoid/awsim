# Persistence

By default, AWSim is stateless. All data lives in memory and is lost when the process exits. Enable persistence with `--data-dir`.

## Enabling Persistence

```bash
./awsim --data-dir /var/lib/awsim
# or
AWSIM_DATA_DIR=/var/lib/awsim ./awsim
```

AWSim creates the directory if it does not exist.

## Services That Persist

awsim has two distinct persistence layers:

1. **JSON snapshots** for handler state (table schemas, IAM users, queue metadata, etc.) under `{data_dir}/snapshots/`.
2. **Per-service SQLite databases** for high-volume row data (DDB items, log events, metrics, kinesis records, SES outbox). These sit alongside the snapshots rather than replacing them.

Most services write and restore JSON snapshots on graceful shutdown / startup. As of the current build, 53 of 61 registered services do.

Those with behaviour worth calling out:

| Service | Signing Name | Note |
|---------|-------------|------|
| DynamoDB | `dynamodb` | Schema only. Items live in SQLite, see below |
| KMS | `kms` | Includes key material, so ciphertext produced before a restart still decrypts |
| Step Functions | `states` | Includes suspended `waitForTaskToken` executions, which would otherwise be unanswerable |
| EventBridge | `events` | Buses, rules, targets, archives and connections. The recent-events ring is excluded as debugging state |
| API Gateway | `apigateway` | The authorizer decision cache is excluded, so a stale allow/deny cannot survive a restart |
| CloudWatch Logs | `logs` | Group and stream metadata only. Events live in SQLite |
| SES | `ses` | Identities, templates, configuration sets, receipt rules and filters. Sent mail lives in SQLite |

The rest (SQS, S3, IAM, Lambda, ECR, SNS, Secrets Manager, ECS, EC2, EKS, CloudFormation, ELB, CloudFront, CloudTrail, Glue, Organizations, SSM, Batch, DataSync, SSO Admin, ACM, WAF, Scheduler, RDS, Cognito, and others) round-trip their handler state without caveats.

Still not covered: `bedrock`, `bedrock-runtime`, `comprehend`, `kendra`, `execute-api` (API Gateway v2), and `sts`. Kinesis and CloudWatch Metrics keep their row data in SQLite, so the data survives even though their handler metadata is not snapshotted.

The following services persist their primary row data into a SQLite database under `{data_dir}/`:

| Service | DB file | Holds |
|---------|---------|-------|
| DynamoDB | `dynamodb.db` | Items + table-schema rows |
| CloudWatch Logs | `cloudwatch-logs.db` | Log events (per-group / per-stream rows) |
| CloudWatch Metrics | `cloudwatch-metrics.db` | Metric data points |
| Kinesis | `kinesis.db` | Stream records (per-shard, sequence-numbered) |
| SES | `ses.db` | Captured outbound emails (see [Outbox](/services/ses#outbox)) |

Each DB uses WAL mode + a 16 MiB mmap with a tight 2 MiB page cache and an r2d2 connection pool (`min_idle=1, max_size=4`) so a fresh awsim process holds only ~256 KiB of resident SQLite per service until traffic arrives.

Services in neither list are in-memory only and lost on restart. This is a coverage gap rather than a design choice, and it is reported rather than hidden: a named snapshot records those services under `not_captured`, and loading one returns `complete: false`.

## Named Snapshots

Beyond the automatic save-on-shutdown / restore-on-startup flow,
AWSim supports point-in-time **named snapshots**: bundles of the
serialised state of every service that supports snapshots, plus billing
and chaos rules, so you can freeze a complex test scenario and restore it
on demand.

A snapshot does not capture everything. Services with no snapshot support
are excluded, and so is data held outside handler state: DynamoDB items,
CloudWatch log events, Kinesis records, and body-store payloads such as S3
object contents. Table schemas, queues and buckets come back; their
contents do not.

Both gaps are reported rather than inferred. The load response is:

```json
{
  "name": "baseline",
  "complete": false,
  "restored": ["s3", "sqs", "..."],
  "unsupported": [],
  "not_captured": ["kms", "ssm", "states", "..."],
  "failed": []
}
```

Check `complete` to know whether the restore was total. `not_captured`
lists services whose state was never in the bundle, `unsupported` lists
services that could not restore what was there, and `failed` lists real
errors.

```bash
# Save the current state under a name.
awsim snapshot save baseline

# Make changes (apply chaos, create resources, and so on)
awsim chaos clear
aws --endpoint-url http://localhost:4566 s3 mb s3://scratch

# Restore. This overwrites live state for any service captured in the snapshot.
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
bodies) are *not* in the bundle. Buckets, queues and tables are
recreated on load but their contents are not. This is good enough
for sharing topology, IAM, Cognito and chaos scenarios; deeper
bundling is on the roadmap.

`awsim snapshot` requires `--data-dir` to be set on the running
server.

**Note on S3:** S3 bucket metadata and object metadata are persisted via the JSON snapshot. Object bodies (the raw bytes) are persisted separately to disk under `{data_dir}/s3/` whenever `--data-dir` is supplied. See [S3 object bodies](#s3-object-bodies) below.

**Note on SQS:** SQS queue metadata is persisted via the JSON snapshot. Message bodies are written separately to disk under `{data_dir}/sqs/` whenever `--data-dir` is supplied. See [SQS message bodies](#sqs-message-bodies) below.

**Note on DynamoDB:** Only table schema metadata rides in the JSON snapshot. Items live in a dedicated SQLite database at `{data_dir}/dynamodb.db`. See [DynamoDB SQLite store](#dynamodb-sqlite-store) below.

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

Object metadata still rides in the regular `s3.json` snapshot. On restore, each object is wired up to its on-disk path and bytes are read lazily by `GetObject` rather than preloaded, which keeps startup cheap even for large datasets. `DeleteObject`, `DeleteBucket`, `AbortMultipartUpload`, and `CompleteMultipartUpload` clean up their files on a best-effort basis (failures are logged via `tracing`).

If a body file is missing on disk after a restart (for example, the data directory was partially wiped), `GetObject` returns `NoSuchKey` for that object.

When `--data-dir` is not supplied, the service stays fully in-memory and object bodies are lost on shutdown.

## DynamoDB SQLite store

Unlike the other services, DynamoDB does not persist its items through the JSON snapshot. Items are written directly to a single SQLite database at `{data_dir}/dynamodb.db` (or a per-process tempfile when `--data-dir` is unset). One database serves every account/region, with partitioning handled by `(account, region, table_name)` columns on the `items` table.

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

The awsim-only `TruncateTable` op clears every item in a table while leaving schema, indexes, and stream config intact, backed by a single `DELETE FROM items WHERE account=? AND region=? AND table_name=?`. Useful for "reset between tests" loops in the admin UI; not available in real DynamoDB.

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

The `sqs.json` snapshot stores queue and message metadata only, so body bytes for on-disk messages are omitted from the snapshot. On restore, each message's body is rebound to its on-disk path. If a body file is missing on disk after restart, `ReceiveMessage` returns an internal error for that message rather than fabricating an empty body.

When `--data-dir` is not supplied, message bodies stay in memory and are lost on shutdown.

## ECR layers

When `--data-dir` is set, the ECR service writes each completed layer's bytes to disk under `{data_dir}/ecr/`:

```
/var/lib/awsim/
  ecr/
    <repository>/
      sha256:abc...    # layer body, named by digest
```

`CompleteLayerUpload` finalizes an upload, hashes the buffered bytes into a sha256 digest, and writes them to `{data_dir}/ecr/{repository}/{digest}`. Repository and image metadata still ride in the regular `ecr.json` snapshot, which stores only layer digest, size, and media type, never the bytes. On restore, each layer is rebound to its on-disk path; bytes are read lazily by the `/v2/{repo}/blobs/{digest}` HTTP endpoint.

`BatchDeleteImage` parses each removed image manifest and best-effort deletes any referenced layer blobs. `DeleteRepository` best-effort removes the entire `{repository}/` subtree.

In-progress upload buffers are kept in memory only; if AWSim crashes mid-upload the partial data is lost (the client retries from `InitiateLayerUpload`).

When `--data-dir` is not supplied, layer bodies stay in memory and are lost on shutdown.

## CloudWatch Logs events

CloudWatch Logs events live in a single SQLite database at `{data_dir}/cloudwatch-logs.db`. Without `--data-dir`, the events go into a per-process tempdir DB that's cleaned up on shutdown.

Each `PutLogEvents` batch is committed inside one SQLite transaction. `FilterLogEvents` queries push timestamp + message-pattern filters down into SQL so the index does the work. Group and stream metadata still rides in the regular `logs.json` snapshot. The schema includes `account`, `region`, `log_group`, `log_stream`, `ts`, `ingestion_ts`, `message` columns plus a composite index on the time fields.

`DeleteLogStream` deletes only that stream's rows; `DeleteLogGroup` cascades to every stream in the group. Both operations run inside a single transaction.

## CloudWatch Metrics

Metric data points live at `{data_dir}/cloudwatch-metrics.db`. Each `PutMetricData` call inserts one row per data point with `account`, `region`, `namespace`, `metric_name`, `value`, `unit`, `timestamp`, `ts_ms`, and a JSON-encoded `dimensions` column. `GetMetricData` queries push the namespace + metric + dimensions filters down to SQL.

A 15-day retention sweep runs on every `PutMetricData` so the DB doesn't grow unbounded. Anything older than 15 days is deleted lazily.

## Kinesis records

Each shard's records live in `{data_dir}/kinesis.db`. Schema: `(account, region, stream, shard, seq, partition_key, data, ts_ms)` with `seq` as the AWS-style monotonic sequence number. Iterators that ask for records "after seq N" run a single indexed range scan. The store also tracks per-stream `RetentionPeriodHours` and trims expired records on the next put.

This replaced the original `Vec<KinesisRecord>` per shard that grew without bound and never honoured `RetentionPeriodHours`.

## SES outbound emails

Every `SendEmail` / `SendBulkEmail` / `SendCustomVerificationEmail` call writes one row to `{data_dir}/ses.db`. The Outbox UI tab and `/_awsim/ses/sent` admin endpoint both read from this store. An hourly retention sweep deletes rows older than `--ses-retention-hours` (default 30 days; `0` disables).

See the [SES service doc](/services/ses#outbox) for the full schema + admin endpoint.

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

After snapshot restore, AWSim sweeps each persisted service's body store for orphaned files, meaning disk blobs that no longer correspond to anything in the in-memory state. Orphans typically appear after a process crash, an out-of-band file deletion, or any other abnormal shutdown that left the snapshot and the body store out of sync.

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

Long-running services with high write volume can grow the body store unbounded: large S3 uploads, busy SQS queues, frequent Lambda code updates, and pushes to ECR. Pass `--max-blob-bytes <N>` (or set `AWSIM_MAX_BLOB_BYTES=N`) to cap each persisted service's body store at `N` bytes:

```bash
./awsim --data-dir /var/lib/awsim --max-blob-bytes 1073741824   # 1 GiB per service
```

The cap is applied independently to S3, Lambda, SQS, ECR, and CloudWatch Logs, so each service may use up to `N` bytes. When a `write_blob` would push a service over its cap, AWSim deletes the oldest files (by modification time) until the new write fits, then writes the new blob.

Eviction caveats:

- Evicted blobs are removed from disk but their metadata still lives in the in-memory inventory (and the next snapshot). Subsequent `GetObject`, `ReceiveMessage`, `Invoke`, or layer-blob fetches for an evicted blob return `NoSuchKey` / "missing body" errors. The cap takes precedence over data integrity.
- A single write larger than the cap fails immediately with an out-of-space error after attempting eviction.
- The cap is per-service, not global. To bound the total directory, divide your overall budget across services and set the smallest reasonable `--max-blob-bytes`.

When the flag is unset, body stores grow without limit (the current default).
