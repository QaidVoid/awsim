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

Services not in this list (e.g., KMS, Secrets Manager) are always in-memory only.

**Note on S3:** S3 bucket metadata and object metadata are persisted via the JSON snapshot. Object bodies (the raw bytes) are persisted separately to disk under `{data_dir}/s3/` whenever `--data-dir` is supplied — see [S3 object bodies](#s3-object-bodies) below.

**Note on SQS:** SQS queue metadata is persisted via the JSON snapshot. Message bodies are written separately to disk under `{data_dir}/sqs/` whenever `--data-dir` is supplied — see [SQS message bodies](#sqs-message-bodies) below.

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
