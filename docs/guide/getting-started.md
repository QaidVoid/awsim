# Getting Started

## Prerequisites

- **Rust 1.86+** — required to build from source. Install via [rustup](https://rustup.rs/).
- Or **Nix** — see the [Nix guide](/guide/nix) for a fully reproducible setup.

## Build from Source

```bash
git clone https://github.com/QaidVoid/awsim
cd awsim
cargo build --release
```

The binary is at `target/release/awsim`.

## Run

```bash
./target/release/awsim
```

By default, AWSim listens on port `4566` with region `us-east-1` and account ID `000000000000`.

## Verify

```bash
curl http://localhost:4566/_awsim/health
```

Expected response:

```json
{"status":"ok"}
```

## Use with AWS CLI

Set `--endpoint-url` on every command:

```bash
aws --endpoint-url http://localhost:4566 s3 ls
aws --endpoint-url http://localhost:4566 s3 mb s3://my-bucket
aws --endpoint-url http://localhost:4566 s3 cp file.txt s3://my-bucket/
```

You can also set the endpoint via the AWS CLI profile:

```ini
# ~/.aws/config
[profile awsim]
region = us-east-1
endpoint_url = http://localhost:4566
```

Then use `--profile awsim` or `AWS_PROFILE=awsim` instead of repeating `--endpoint-url`.

Credentials can be any non-empty value — AWSim does not validate them:

```bash
AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test aws --endpoint-url http://localhost:4566 s3 ls
```

## SDK Configuration

### JavaScript / TypeScript

```typescript
import { S3Client } from "@aws-sdk/client-s3";

const s3 = new S3Client({
  region: "us-east-1",
  endpoint: "http://localhost:4566",
  credentials: {
    accessKeyId: "test",
    secretAccessKey: "test",
  },
  forcePathStyle: true, // required for S3
});
```

### Python (boto3)

```python
import boto3

s3 = boto3.client(
    "s3",
    region_name="us-east-1",
    endpoint_url="http://localhost:4566",
    aws_access_key_id="test",
    aws_secret_access_key="test",
)
```

### Rust (aws-sdk-rust)

```rust
use aws_config::BehaviorVersion;
use aws_sdk_s3::config::Builder;

let config = aws_config::defaults(BehaviorVersion::latest())
    .endpoint_url("http://localhost:4566")
    .region(aws_types::region::Region::new("us-east-1"))
    .credentials_provider(aws_credential_types::Credentials::new(
        "test", "test", None, None, "awsim",
    ))
    .load()
    .await;

let s3 = aws_sdk_s3::Client::from_conf(
    Builder::from(&config)
        .force_path_style(true)
        .build(),
);
```

## Quick Example

```bash
# Create a bucket
aws --endpoint-url http://localhost:4566 s3 mb s3://my-bucket

# Upload a file
echo "hello" > test.txt
aws --endpoint-url http://localhost:4566 s3 cp test.txt s3://my-bucket/test.txt

# List objects
aws --endpoint-url http://localhost:4566 s3 ls s3://my-bucket/

# Download the file
aws --endpoint-url http://localhost:4566 s3 cp s3://my-bucket/test.txt downloaded.txt
cat downloaded.txt
```
