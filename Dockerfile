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
