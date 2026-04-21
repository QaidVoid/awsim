# AWSim

A fully offline, free AWS development environment. Single binary, sub-second startup, 20 services.

## Quick Start

### From Source

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

## Supported Services

| Service | Protocol | Key Operations |
|---------|----------|----------------|
| S3 | REST XML | Buckets, objects, multipart uploads, versioning |
| SQS | JSON 1.0 | Queues, messages, FIFO, DLQ, long polling |
| DynamoDB | JSON 1.0 | Tables, items, queries, scans, expressions, transactions |
| SNS | JSON 1.1 | Topics, subscriptions, publishing, filtering |
| IAM | Query | Users, groups, roles, policies, instance profiles |
| STS | Query | GetCallerIdentity, AssumeRole, session tokens |
| Lambda | REST JSON | Functions, invocation, aliases, versions, layers, event source mappings |
| EventBridge | JSON 1.1 | Event buses, rules, targets, event pattern matching |
| CloudWatch Logs | JSON 1.1 | Log groups, streams, events, filtering |
| Step Functions | JSON 1.0 | State machines, executions, ASL interpreter |
| KMS | JSON 1.1 | Keys, aliases, encrypt/decrypt, data keys |
| Secrets Manager | JSON 1.1 | Secrets, versions, version stages |
| SSM Parameter Store | JSON 1.1 | Hierarchical parameters, history, types |
| Kinesis | JSON 1.1 | Streams, shards, records, iterators |
| SES v2 | REST JSON | Email sending, identities, templates |
| Cognito | JSON 1.1 | User pools, auth flows, JWT tokens, groups |
| EC2 | Query | VPCs, subnets, security groups, gateways, route tables |
| CloudFormation | Query | Stacks, change sets, template validation, intrinsic functions |
| ECS | JSON 1.1 | Clusters, task definitions, services, tasks |
| ECR | JSON 1.1 | Repositories, images, authorization |

## Configuration

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--port` | `AWSIM_PORT` | 4566 | Listen port |
| `--region` | `AWSIM_REGION` | us-east-1 | Default region |
| `--account-id` | `AWSIM_ACCOUNT_ID` | 000000000000 | Default account |
| `--data-dir` | `AWSIM_DATA_DIR` | (none) | Persistence directory |
| `--log-level` | `AWSIM_LOG_LEVEL` | info | Log verbosity |

## Admin Console

Start the dev server:

```bash
cd ui && bun run dev
```

Open http://localhost:5173 for the management dashboard.

## License

MIT OR Apache-2.0
