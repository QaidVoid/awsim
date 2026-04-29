# AWSim

A fully offline, free AWS development environment. Single binary, sub-second startup, 61 services, real IAM policy enforcement, and an estimated-bill dashboard so you can watch how much your workload would cost on real AWS.

## Quick Start

### From source

```bash
cargo run -- --port 4566
```

### Docker

```bash
docker compose up
```

### Usage

Configure any AWS SDK to point to AWSim:

```bash
export AWS_ENDPOINT_URL=http://localhost:4566
export AWS_ACCESS_KEY_ID=test
export AWS_SECRET_ACCESS_KEY=test
export AWS_DEFAULT_REGION=us-east-1

aws s3 mb s3://my-bucket
aws sqs create-queue --queue-name my-queue
aws dynamodb create-table --table-name my-table \
    --key-schema AttributeName=id,KeyType=HASH \
    --attribute-definitions AttributeName=id,AttributeType=S \
    --billing-mode PAY_PER_REQUEST
```

## Supported services

All services share the same `http://localhost:4566` endpoint; routing comes from the `X-Amz-Target` header (JSON protocols), the URL path (REST), or the `Action=` form field (Query). See [docs/services/](docs/services/index.md) for the full per-service operation lists.

| Category | Services |
|---|---|
| Storage | S3, EFS-style via S3 |
| Compute | Lambda, ECS, EKS, Batch, EC2 (partial) |
| Networking | API Gateway (REST + HTTP), Route 53, ELB, CloudFront |
| Database | DynamoDB (SQLite-backed), RDS metadata |
| Messaging | SQS, SNS, EventBridge, EventBridge Scheduler, Kinesis, Firehose |
| Identity | IAM, STS, Cognito (User + Identity Pools), SSO Admin, Organizations |
| Security | KMS, Secrets Manager, ACM, WAF, CloudTrail |
| Observability | CloudWatch Logs, CloudWatch Metrics |
| Containers | ECR, ECS, EKS |
| Data + AI | Athena, Glue, Bedrock + Bedrock Runtime, Comprehend, Kendra, Polly |
| Email | SES v2 |
| Orchestration | Step Functions, CloudFormation, AppSync, DataSync |
| Search | OpenSearch (Elasticsearch-compatible REST) |

## Highlights

- **DynamoDB on SQLite.** Items live in a single WAL-mode SQLite database (`{data_dir}/dynamodb.db`) — bounded memory regardless of row count, real ACID `TransactWriteItems`, and an awsim-only `TruncateTable` op for fast resets. See [docs/services/dynamodb.md](docs/services/dynamodb.md).
- **Real IAM policy enforcement.** Opt-in via `AWSIM_IAM_ENFORCE=true`. Implements identity policies, resource policies, permissions boundaries, SCPs, and session policies with all 26 condition operators. Currently enforced on S3, DynamoDB, KMS, SQS, SNS, Secrets Manager, Lambda, and IAM. See [docs/guide/iam-enforcement.md](docs/guide/iam-enforcement.md).
- **Lambda execution.** Real container-based Lambda runtimes (Node, Python, Go, Rust) via `docker run`. Supports event source mappings from SQS, Kinesis, and DynamoDB Streams. See [docs/guide/lambda-execution.md](docs/guide/lambda-execution.md).
- **Persistence.** `--data-dir` enables snapshot-based recovery for tables, queues, secrets, IAM, etc. Object/message/code/layer bodies are written to per-service body stores; orphan GC sweeps them on startup. See [docs/guide/persistence.md](docs/guide/persistence.md).
- **Admin console.** SvelteKit UI at `http://localhost:5173` (after `cd ui && bun run dev`) — browse buckets, scan DynamoDB, invoke Lambda, list IAM principals, etc. See [docs/guide/admin-console.md](docs/guide/admin-console.md).
- **Estimated billing dashboard.** Real-time rolling AWS bill at `/billing` — every metered request × vendored AWS pricing, with per-service breakdown, 30-min cost trajectory chart, and a "time to bankruptcy" widget. Pricing data is pulled directly from the AWS Pricing Bulk JSON via `cargo run -p awsim-billing --bin refresh-pricing --features refresh`. Covers per-request, byte-ingest, data-transfer, GB-month storage, GB-second compute, instance-hours, state-transition and per-character billing axes across 22+ services. See [docs/guide/billing.md](docs/guide/billing.md).
- **Chaos engine.** Inject synthetic AWS errors and latency on a per-service / per-operation basis to test retry, backoff, circuit-breaker and graceful-degradation logic. Drive it from the `/chaos` dashboard, the `awsim chaos` CLI, or the `/_awsim/chaos/*` HTTP API. Six built-in presets (`flaky-s3`, `ddb-throttle`, `slow-lambda`, `kms-outage`, `regional-failover`, `network-jitter`) cover the common scenarios. See [docs/guide/chaos.md](docs/guide/chaos.md).

## Configuration

| Flag | Env var | Default | Description |
|------|---------|---------|-------------|
| `--port` | `AWSIM_PORT` | `4566` | Listen port |
| `--region` | `AWSIM_REGION` | `us-east-1` | Default region |
| `--account-id` | `AWSIM_ACCOUNT_ID` | `000000000000` | Default account |
| `--data-dir` | `AWSIM_DATA_DIR` | (none) | Persistence directory; omit for in-memory only |
| `--log-level` | `AWSIM_LOG_LEVEL` | `info` | Log verbosity |
| `--no-gc` | `AWSIM_NO_GC` | `false` | Disable startup orphan-blob GC for body stores |
| `--gc-interval-secs` | `AWSIM_GC_INTERVAL_SECS` | (startup-only) | Re-run orphan GC every N seconds |
| `--max-blob-bytes` | `AWSIM_MAX_BLOB_BYTES` | (unbounded) | Per-service body store cap (FIFO eviction); applies to S3, Lambda, SQS, ECR |
| `--max-concurrent-requests` | `AWSIM_MAX_CONCURRENT_REQUESTS` | `5000` | In-flight request cap; excess returns 503 |
| (env-only) | `AWSIM_IAM_ENFORCE` | `false` | Turn on IAM policy enforcement on the gateway |

## Admin console

```bash
cd ui && bun run dev
```

Open `http://localhost:5173`.

## Documentation

- [What is AWSim](docs/guide/what-is-awsim.md)
- [Getting started](docs/guide/getting-started.md)
- [Configuration](docs/guide/configuration.md)
- [Persistence](docs/guide/persistence.md)
- [IAM enforcement](docs/guide/iam-enforcement.md)
- [Lambda execution](docs/guide/lambda-execution.md)
- [API Gateway](docs/guide/api-gateway.md)
- [Cognito OAuth](docs/guide/cognito-oauth.md)
- [Estimated billing](docs/guide/billing.md)
- [Service-by-service operation lists](docs/services/index.md)

## License

MIT OR Apache-2.0
