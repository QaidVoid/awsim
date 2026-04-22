# Docker

## Dockerfile

The repository includes a multi-stage Dockerfile:

```dockerfile
FROM rust:1.86-slim AS builder

WORKDIR /app
COPY . .
RUN cargo build --release --bin awsim

FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/awsim /usr/local/bin/awsim

EXPOSE 4566

HEALTHCHECK --interval=10s --timeout=3s --start-period=5s \
    CMD curl -f http://localhost:4566/_awsim/health || exit 1

ENTRYPOINT ["awsim"]
CMD ["--port", "4566"]
```

## Build the Image

```bash
docker build -t awsim .
```

## Run the Container

```bash
docker run -p 4566:4566 awsim
```

With persistence:

```bash
docker run -p 4566:4566 -v awsim-data:/data \
  -e AWSIM_DATA_DIR=/data \
  awsim
```

With custom region and account:

```bash
docker run -p 4566:4566 \
  -e AWSIM_REGION=eu-west-1 \
  -e AWSIM_ACCOUNT_ID=123456789012 \
  awsim
```

## Docker Compose

The repository includes a `docker-compose.yml`:

```yaml
services:
  awsim:
    build: .
    ports:
      - "4566:4566"
    environment:
      - AWSIM_REGION=us-east-1
      - AWSIM_ACCOUNT_ID=000000000000
      - AWSIM_LOG_LEVEL=info
    volumes:
      - awsim-data:/data
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:4566/_awsim/health"]
      interval: 10s
      timeout: 3s
      start-period: 5s

volumes:
  awsim-data:
```

Start with:

```bash
docker compose up
```

Start in the background:

```bash
docker compose up -d
```

## Health Check

Docker's built-in health check polls `/_awsim/health`. The container is marked as `healthy` once it responds with HTTP 200. Your other services can use `depends_on: {awsim: {condition: service_healthy}}`.

```yaml
services:
  my-app:
    image: my-app
    depends_on:
      awsim:
        condition: service_healthy
  awsim:
    build: .
    ports:
      - "4566:4566"
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:4566/_awsim/health"]
      interval: 10s
      timeout: 3s
      start-period: 5s
```
