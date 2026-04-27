# Admin Console

AWSim includes a SvelteKit-based web UI for browsing and managing emulated resources.

## Running the UI

The UI lives in the `ui/` directory of the repository:

```bash
cd ui
bun install
bun run dev
```

The dev server starts on `http://localhost:5173` by default. It proxies `/_awsim` requests to AWSim running on `http://localhost:4566`, so make sure AWSim is running before opening the UI.

## Admin API Endpoints

AWSim exposes a lightweight admin API independent of the AWS wire protocol:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/_awsim/health` | GET | Health check — returns `{"status":"ok"}` |
| `/_awsim/services` | GET | List all registered services with their signing names and protocols |
| `/_awsim/config` | GET | Active configuration (port, region, account ID, data-dir) |
| `/_awsim/stats` | GET | Runtime statistics |
| `/_awsim/storage` | GET | Per-service `BodyStore` disk usage (when `--data-dir` is set) |

Example:

```bash
curl http://localhost:4566/_awsim/health
curl http://localhost:4566/_awsim/services
curl http://localhost:4566/_awsim/config
curl http://localhost:4566/_awsim/stats
curl http://localhost:4566/_awsim/storage
```

### `/_awsim/storage`

Reports on-disk byte counts for each service that has a `BodyStore` enabled.
When AWSim is started without `--data-dir`, `data_dir` is `null` and the
`services` array is empty.

```json
{
  "data_dir": "/var/awsim/data",
  "snapshots": {
    "path": "/var/awsim/data/snapshots",
    "size_bytes": 12345
  },
  "services": [
    {
      "name": "s3",
      "groups": ["objects", "multipart"],
      "size_bytes": 1048576,
      "blob_count": 42
    },
    {
      "name": "lambda",
      "groups": ["lambda"],
      "size_bytes": 512000,
      "blob_count": 3
    },
    {
      "name": "ecr",
      "groups": ["ecr"],
      "size_bytes": 0,
      "blob_count": 0
    },
    {
      "name": "sqs",
      "groups": ["sqs"],
      "size_bytes": 256,
      "blob_count": 12
    }
  ],
  "total_size_bytes": 1573577
}
```

The handler walks each group directory using `metadata()` only (no file reads),
so it returns quickly even with thousands of files.

## Dashboard

The main dashboard shows:

- All registered services
- Active resource counts per service (where available)
- Quick links to individual service pages

## Service Pages

The UI has 33 service-specific pages. Each page lets you view, create, and manage resources for that service — for example:

- **S3**: Browse buckets, list objects, upload/download
- **DynamoDB**: View tables, scan items, run queries
- **SQS**: List queues, send and receive messages
- **Lambda**: List functions, invoke them
- **Cognito**: Manage user pools, users, groups
- **IAM**: Manage users, roles, policies, access keys
- **Step Functions**: View state machines, executions, execution history (ASL viewer included)

## Notes

- The UI is a development tool only — it is not packaged inside the AWSim binary.
- The UI connects to whichever AWSim instance is running on `localhost:4566`. To point it at a different host/port, set the `VITE_AWSIM_URL` environment variable before running `bun run dev`.
