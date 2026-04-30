# Configuration

AWSim is configured through CLI flags and environment variables. All flags have corresponding env vars.

## CLI Flags

### Core

| Flag | Short | Default | Env Var | Description |
|------|-------|---------|---------|-------------|
| `--port` | `-p` | `4566` | `AWSIM_PORT` | Port to listen on |
| `--region` | `-r` | `us-east-1` | `AWSIM_REGION` | Default AWS region |
| `--account-id` | | `000000000000` | `AWSIM_ACCOUNT_ID` | Simulated AWS account ID |
| `--data-dir` | | _(none)_ | `AWSIM_DATA_DIR` | Directory for persistence + per-service SQLite stores |
| `--log-level` | `-v` | `info` | `AWSIM_LOG_LEVEL` | Log verbosity |
| _(env only)_ | | `false` | `AWSIM_IAM_ENFORCE` | Enable IAM policy evaluation (see [IAM Enforcement](/guide/iam-enforcement)) |

### Memory + concurrency tuning

These knobs cap how much RAM AWSim can use under burst load. The defaults are chosen for development on a typical workstation; lower them when you're running in a memory-constrained container, raise them for higher throughput on a beefier box.

| Flag | Default | Env Var | Description |
|------|---------|---------|-------------|
| `--max-concurrent-requests` | `256` | `AWSIM_MAX_CONCURRENT_REQUESTS` | Hard cap on in-flight requests. Excess requests get an immediate `503` instead of queueing. Each request can hold up to `--max-body-bytes` of buffered body plus a parsed `serde_json::Value` ~3-5× the body size, so this directly bounds peak RSS during request bursts. |
| `--max-body-bytes` | `100 MiB` (`104857600`) | `AWSIM_MAX_BODY_BYTES` | Per-request body size cap. Lower this when hammering DDB with large `BatchWriteItem` payloads. |
| `--max-blocking-threads` | `32` | `AWSIM_MAX_BLOCKING_THREADS` | Cap on the tokio blocking thread pool — used for SQLite IO and other `spawn_blocking` work. Each thread reserves ~2 MiB of stack, so this directly bounds RSS contribution from blocking work. Drop to `8` to clamp memory during bulk imports; raise for higher write throughput. |

### Service-specific

| Flag | Default | Env Var | Description |
|------|---------|---------|-------------|
| `--ses-retention-hours` | `720` (30 days) | `AWSIM_SES_RETENTION_HOURS` | Hours to retain captured SES outbound emails. The hourly sweep deletes anything older. Set to `0` to disable. See the [SES service doc](/services/ses). |

## Examples

```bash
# Custom port and region
./awsim --port 4000 --region eu-west-1

# Enable persistence + per-service SQLite stores
./awsim --data-dir /var/lib/awsim

# Tight memory profile (e.g. 1-2 GiB container)
./awsim --max-concurrent-requests 64 --max-blocking-threads 8

# Verbose logging
./awsim --log-level debug

# Via environment variables
AWSIM_PORT=4000 AWSIM_REGION=eu-west-1 AWSIM_DATA_DIR=/data ./awsim
```

## Log Levels

Valid values for `--log-level` / `AWSIM_LOG_LEVEL`:

- `error`
- `warn`
- `info` (default)
- `debug`
- `trace`

## Allocator

AWSim ships with `tikv-jemallocator` as the global allocator on Linux + macOS (MSVC builds keep the system allocator). Jemalloc returns memory to the OS more aggressively than glibc malloc, so idle RSS stays flat after burst workloads instead of ratcheting upward.

If you need to tune jemalloc's page-decay behaviour — useful when investigating per-second memory cycling — set `MALLOC_CONF` in the environment:

```bash
# Aggressive page return: drop dirty/muzzy pages after 1s + 0s
MALLOC_CONF="dirty_decay_ms:1000,muzzy_decay_ms:0,narenas:2" ./awsim
```

See the [memory + observability guide](/guide/admin-console#observability) for diagnosing growth.

## Startup Output

When AWSim starts, it prints the active configuration:

```
INFO awsim: Listening on 0.0.0.0:4566
INFO awsim: Region: us-east-1
INFO awsim: Account ID: 000000000000
INFO awsim: Inflight-request cap enabled max_concurrent_requests=256
```

If `--data-dir` is set, the startup output also shows the snapshot directory and confirms persistence is enabled.
