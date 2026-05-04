# Lightweight multi-stage Dockerfile for AWSim.
#
# - Builder uses Rust 1.91 + musl so the final binary is fully static.
# - BuildKit cache mounts (`--mount=type=cache`) reuse the cargo registry +
#   target dir between builds — first run is slow, subsequent rebuilds skip
#   downloading + recompiling unchanged crates.
# - Runtime is distroless/static (~2MB base) — only the AWSim binary, no shell,
#   no package manager, runs as a non-root user.
#
# Local build:
#     docker buildx build -t awsim:dev --load .
#
# Multi-arch (amd64 + arm64), publish to GHCR:
#     docker buildx build \
#       --platform linux/amd64,linux/arm64 \
#       --tag ghcr.io/qaidvoid/awsim:0.2.0 \
#       --push .

# syntax=docker/dockerfile:1.7

ARG RUST_VERSION=1.91

# ---------- Builder stage ----------
FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-slim AS builder
ARG TARGETARCH

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
        musl-tools \
        clang \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Map docker target arch → rustup target triple.
RUN case "${TARGETARCH}" in \
        amd64) echo "x86_64-unknown-linux-musl"  > /tmp/rust-target ;; \
        arm64) echo "aarch64-unknown-linux-musl" > /tmp/rust-target ;; \
        *) echo "unsupported TARGETARCH: ${TARGETARCH}" >&2; exit 1 ;; \
    esac && rustup target add "$(cat /tmp/rust-target)"

COPY . .

# BuildKit cache mounts: persist cargo registry/git + the target dir between
# builds. First build pulls + compiles everything; subsequent builds only
# rebuild what changed.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release --locked \
        --target "$(cat /tmp/rust-target)" \
        --bin awsim \
    && cp "target/$(cat /tmp/rust-target)/release/awsim" /usr/local/bin/awsim

# ---------- Runtime stage ----------
FROM gcr.io/distroless/static-debian12:nonroot

LABEL org.opencontainers.image.title="AWSim"
LABEL org.opencontainers.image.description="Fully offline AWS development environment"
LABEL org.opencontainers.image.source="https://github.com/QaidVoid/awsim"
LABEL org.opencontainers.image.licenses="MIT OR Apache-2.0"

# /data is the default persistence directory (use --data-dir or AWSIM_DATA_DIR
# to enable). Declared as a volume so docker run --volume mounts cleanly.
ENV AWSIM_PORT=4566
EXPOSE 4566
VOLUME ["/data"]

COPY --from=builder /usr/local/bin/awsim /usr/local/bin/awsim

ENTRYPOINT ["/usr/local/bin/awsim"]
