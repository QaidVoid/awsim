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
| `/_awsim/events` | GET | Server-Sent Events stream of every gateway request |

Example:

```bash
curl http://localhost:4566/_awsim/health
curl http://localhost:4566/_awsim/services
curl http://localhost:4566/_awsim/config
curl http://localhost:4566/_awsim/stats
curl http://localhost:4566/_awsim/storage
curl -N http://localhost:4566/_awsim/events
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

### `/_awsim/events`

Opens a [Server-Sent Events](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events)
stream that emits one JSON-encoded event for every AWS API request that
flows through the gateway. The connection stays open and pushes events
in real time until the client disconnects.

The broadcast channel has a capacity of 256 events; if a slow consumer
falls behind, the oldest events are dropped.

Event shape:

```json
{
  "id": "req-uuid",
  "ts": 1735041600.123,
  "method": "POST",
  "path": "/",
  "service": "s3",
  "operation": "PutObject",
  "account_id": "000000000000",
  "region": "us-east-1",
  "principal_arn": "arn:aws:iam::000000000000:access-key/AKIA...",
  "status_code": 200,
  "duration_ms": 12.5,
  "request_size": 1024,
  "response_size": 256,
  "error_code": null
}
```

`error_code` is `null` for `2xx`/`3xx` responses and the AWS error code
(e.g., `"AccessDenied"`, `"NoSuchBucket"`) when `status_code >= 400`.
`principal_arn` is `null` for unauthenticated requests; `operation` is
`null` only when the request failed before the operation could be
parsed.

Each event arrives over the wire as:

```
data: {"id":"...","ts":1735041600.123,"method":"POST","path":"/","service":"s3","operation":"PutObject","status_code":200,...}

```

The UI consumes this stream to power the dashboard activity feed and
the live request log.

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
