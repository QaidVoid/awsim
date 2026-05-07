# Docker

Published images live at [`ghcr.io/qaidvoid/awsim`](https://github.com/QaidVoid/awsim/pkgs/container/awsim). Both `linux/amd64` and `linux/arm64` are built natively.

## Tags

| tag | what |
| --- | --- |
| `:latest` | most recent stable release |
| `:<version>` | specific release (e.g. `:0.3.0`) |
| `:nightly` | rolling pre-release rebuilt from `main` every night |
| `:nightly-<short-sha>` | pinned nightly so you don't get silently bumped |

## Quick start

```bash
docker run --rm -p 4566:4566 ghcr.io/qaidvoid/awsim:latest
```

Open the admin UI at <http://localhost:4566/_awsim/ui/> or hit `localhost:4566` in a browser and it'll redirect for you.

For a green-padlock HTTPS endpoint with no client trust setup, also publish 4567 and turn the listener on:

```bash
docker run --rm -p 4566:4566 -p 4567:4567 \
  -e AWSIM_HTTPS_PORT=4567 \
  ghcr.io/qaidvoid/awsim:latest
```

That serves a publicly-trusted Let's Encrypt cert for `aws.qaidvoid.dev` (and `*.aws.qaidvoid.dev`) - DNS for those names points to `127.0.0.1`, so traffic never leaves your machine. See [tls.md](./tls.md) for the full story.

## Persisting data

`AWSIM_DATA_DIR=/data` is set by default in the image and `/data` is declared as a volume. Bind-mount or named-volume to keep state across restarts:

```bash
docker run --rm -p 4566:4566 \
  -v awsim-data:/data \
  ghcr.io/qaidvoid/awsim:latest
```

To run fully in-memory, override the env var:

```bash
docker run --rm -p 4566:4566 \
  -e AWSIM_DATA_DIR= \
  ghcr.io/qaidvoid/awsim:latest
```

## Common flags as env vars

Every CLI flag has a matching `AWSIM_*` env var:

```bash
docker run --rm -p 4566:4566 \
  -e AWSIM_REGION=eu-west-1 \
  -e AWSIM_ACCOUNT_ID=123456789012 \
  -e AWSIM_LOG_LEVEL=debug \
  -e AWSIM_ENFORCE_IAM=true \
  -v awsim-data:/data \
  ghcr.io/qaidvoid/awsim:latest
```

`AWSIM_ENFORCE_IAM=true` locks IAM enforcement on at boot regardless of any persisted runtime config. Useful when you want policy enforcement before anyone opens the UI.

## docker compose

A `docker-compose.yml` ships in the repo with both the HTTP and HTTPS listeners enabled:

```yaml
services:
  awsim:
    image: ghcr.io/qaidvoid/awsim:latest
    ports:
      - "4566:4566"
      - "4567:4567"
    environment:
      - AWSIM_REGION=us-east-1
      - AWSIM_ACCOUNT_ID=000000000000
      - AWSIM_LOG_LEVEL=info
      - AWSIM_HTTPS_PORT=4567
    volumes:
      - awsim-data:/data

volumes:
  awsim-data:
```

Drop the `AWSIM_HTTPS_PORT` env var and the `4567:4567` mapping if you only want the plain-HTTP listener.

```bash
docker compose up        # foreground
docker compose up -d     # detached
```

Apps that want to wait for awsim to be ready should use `depends_on: { awsim: { condition: service_started } }`. There's no `service_healthy` healthcheck because the runtime is distroless (no shell, no curl). See the note below.

## Health checks

The `/_awsim/health` HTTP endpoint exists, but the runtime image is `gcr.io/distroless/static-debian12:nonroot` which has no shell, curl, or wget. Standard `HEALTHCHECK CMD curl ...` won't work in-container.

Options if you need a strict ready-gate:

- **External probe**: let Kubernetes or your orchestrator hit `/_awsim/health` on the published port.
- **Sidecar**: add a tiny `curlimages/curl` container with `depends_on` and use `service_completed_successfully` after a one-shot probe.
- **Sleep + start**: `service_started` is usually fine for local dev; awsim binds its port within ~250ms of process start.

## Building from source

If you need a custom image (e.g. with extra patches), build from the repo's `Dockerfile`:

```bash
docker build -t awsim:local .
```

Multi-arch local build:

```bash
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  --tag ghcr.io/<you>/awsim:dev \
  --push .
```

The Dockerfile is a three-stage pipeline: a Bun stage builds the SvelteKit admin UI, a Rust+musl stage statically links the binary with the UI embedded, and the runtime stage is distroless/static-debian12.
