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
| `/_awsim/requests` | GET | Most recent captured request ids (newest first) |
| `/_awsim/requests/{id}` | GET | Full captured detail for one request — headers + bodies |
| `/_awsim/requests/{id}/replay` | POST | Re-issue the captured request through the gateway |

Example:

```bash
curl http://localhost:4566/_awsim/health
curl http://localhost:4566/_awsim/services
curl http://localhost:4566/_awsim/config
curl http://localhost:4566/_awsim/stats
curl http://localhost:4566/_awsim/storage
curl -N http://localhost:4566/_awsim/events
curl http://localhost:4566/_awsim/requests
curl http://localhost:4566/_awsim/requests/<id>
curl -X POST http://localhost:4566/_awsim/requests/<id>/replay
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

### `/_awsim/requests`, `/_awsim/requests/{id}`, `/_awsim/requests/{id}/replay`

Every request that flows through the gateway is also captured into a
bounded ring buffer (default 200 entries) keyed by request id. The
buffer stores method, path, query, status, both header sets and both
bodies. Bodies are size-capped at 64 KiB each direction and stored
base64-encoded so binary payloads (S3 objects, ECR layers) survive.

```bash
# List the 50 most recent captured ids (newest first)
curl http://localhost:4566/_awsim/requests
# {"ids": ["a1b2...", "c3d4...", ...]}

# Fetch full detail for one request
curl http://localhost:4566/_awsim/requests/a1b2...
```

`POST /_awsim/requests/{id}/replay` reconstructs the original request
from the captured detail and dispatches it through the gateway again.
Returns the freshly minted request id so callers can pull the new
detail:

```bash
curl -X POST http://localhost:4566/_awsim/requests/a1b2.../replay
# {"new_id": "z9y8...", "status_code": 200, "original_id": "a1b2..."}
```

Replay returns `409 RequestBodyTruncated` when the original request
body exceeded the capture cap — partial-body replay would silently lie
about the result.

## Dashboard

The main dashboard composes a live overview of the running emulator:

- **KPI strip** — total requests since boot, live RPS over a trailing 5s window, on-disk usage across BodyStores, and uptime.
- **Live request stream** — auto-tailing table of recent requests, filterable by 4xx / 5xx. Click any row to open the inspect drawer.
- **Service status list** — per-service blob counts and disk usage from `/_awsim/storage`.
- **Insights panel** — config + storage breakdown.

## Service Pages

The UI ships a service-specific page for every backend that AWSim emulates. Each one is built on a shared `ServicePage` shell with viewport-bound scrolling, a typed API client, and decomposed sub-components (list, detail-sheet, create dialogs). Examples:

- **S3**: Browse buckets, list objects, upload/download
- **DynamoDB**: View tables, scan items, run queries
- **SQS**: List queues, send and receive messages
- **Lambda**: List functions, invoke them
- **Cognito**: Manage user pools, users, groups
- **IAM**: Manage users, roles, policies, access keys
- **Step Functions**: View state machines, executions, execution history (ASL viewer included)

## Keyboard Shortcuts

The console is keyboard-first. Press `?` any time to bring up the cheat sheet.

**General**

| Keys | Action |
|------|--------|
| `?` | Open the shortcut cheat sheet |
| `/` | Open the command palette (also `⌘K` / `Ctrl+K`) |
| `t` | Toggle dark / light theme (preserves the active dark variant) |
| `[` | Collapse / expand the sidebar |
| `i` | Inspect the most recent captured request |

**Navigation** — type the leader key `g`, then a target letter:

| Sequence | Page |
|----------|------|
| `g d` | Dashboard |
| `g r` | Request log |
| `g s` | S3 |
| `g f` | Lambda (function) |
| `g t` | DynamoDB (table) |
| `g i` | IAM |
| `g q` | SQS (queue) |
| `g n` | SNS |
| `g k` | KMS (key) |
| `g e` | EC2 |
| `g c` | Cognito |
| `g m` | CloudWatch metrics |
| `g x` | CloudTrail |
| `g w` | CloudWatch logs |
| `g b` | Bedrock |
| `g p` | API Gateway |

A small "leader" chip appears at the bottom of the screen while a sequence is in flight. `Esc` cancels.

Shortcuts are ignored while typing into inputs, textareas, selects, contenteditable elements, and the command palette input — so they never compete with normal typing.

## Inspect Drawer

The Inspect drawer is a global side-panel that loads the captured detail for any request and shows it in a tabbed view:

- **Request** — every captured header plus the request body, decoded by content-type. JSON is pretty-printed; XML and form payloads are shown verbatim; non-UTF-8 binary bodies fall back to a hex dump of the first 256 bytes.
- **Response** — same treatment for the response side.
- **curl** — a runnable `curl` invocation that reproduces the request against the local emulator, headers and body included.

The drawer header shows the HTTP method, the captured URL, and a **Replay** button. Clicking Replay re-issues the request through the gateway, swaps the drawer to the freshly captured detail, and toasts the new status code. The button is disabled with an explanatory tooltip when the original body was truncated during capture.

The drawer can be opened from:

- Any row in the live request stream or request log.
- The `i` keyboard shortcut (inspects the most recent request).
- The "Tools → Inspect last request" entry in the command palette.

## Themes

The theme picker on the topbar (and in the command palette's "Theme" group) offers five built-in variants:

| Variant | Notes |
|---------|-------|
| Default Dark | Neutral charcoal with the warm AWS-amber accent. |
| Midnight | Deep blue-purple ground with an electric violet accent. |
| Slate | Cooler, less saturated dark with a muted blue accent. |
| Solarized Dark | Ethan Schoonover's classic palette — warm dark cyan ground, yellow accent. |
| Light | The default light scheme — full token coverage, usable but not the primary mode. |

Each variant is applied as a CSS class composed on top of the existing Tailwind `dark` class, so any component that uses `dark:` utilities continues to work. The active theme is persisted in `localStorage` (`awsim-theme`), and the pre-paint script in `app.html` applies it before first paint to avoid a flash of the wrong palette.

Pressing `t` toggles between dark and light while remembering the most recent dark variant — so a quick `t t` round-trip never costs you your customised dark.

## Notes

- The UI is a development tool only — it is not packaged inside the AWSim binary.
- The UI connects to whichever AWSim instance is running on `localhost:4566`. To point it at a different host/port, set the `VITE_AWSIM_URL` environment variable before running `bun run dev`.
