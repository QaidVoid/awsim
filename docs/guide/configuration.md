# Configuration

AWSim is configured through CLI flags and environment variables. All flags have corresponding env vars.

## CLI Flags

| Flag | Short | Default | Env Var | Description |
|------|-------|---------|---------|-------------|
| `--port` | `-p` | `4566` | `AWSIM_PORT` | Port to listen on |
| `--region` | `-r` | `us-east-1` | `AWSIM_REGION` | Default AWS region |
| `--account-id` | | `000000000000` | `AWSIM_ACCOUNT_ID` | Simulated AWS account ID |
| `--data-dir` | | _(none)_ | `AWSIM_DATA_DIR` | Directory for persistence snapshots |
| `--log-level` | `-v` | `info` | `AWSIM_LOG_LEVEL` | Log verbosity |

## Examples

```bash
# Custom port and region
./awsim --port 4000 --region eu-west-1

# Enable persistence
./awsim --data-dir /var/lib/awsim

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

## Body Limit

The request body limit is **100 MB**. This applies to all service endpoints including S3 object uploads.

## Startup Output

When AWSim starts, it prints the active configuration:

```
INFO awsim: Listening on 0.0.0.0:4566
INFO awsim: Region: us-east-1
INFO awsim: Account ID: 000000000000
```

If `--data-dir` is set, the startup output also shows the snapshot directory and confirms persistence is enabled.
