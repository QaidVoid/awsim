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

Services not in this list (e.g., Lambda, KMS, Secrets Manager) are always in-memory only.

**Note on S3:** S3 bucket metadata and object metadata are persisted, but object data (the actual bytes) is not. After a restart, the bucket and object listings will be restored but `GetObject` will return empty content for previously stored objects.

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
