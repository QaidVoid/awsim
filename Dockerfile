# Lightweight multi-stage Dockerfile for AWSim.
#
# - The first stage uses Bun to build the SvelteKit static UI under `ui/build/`.
# - The Rust builder uses musl so the final binary is fully static. The UI
#   build is copied in before `cargo build` so `rust-embed` picks it up.
# - BuildKit cache mounts (`--mount=type=cache`) reuse the cargo registry +
#   target dir + bun cache between builds. First run is slow, subsequent
#   rebuilds skip downloading + recompiling unchanged crates.
# - Runtime is distroless/static (~2 MB base) — only the AWSim binary, no
#   shell, no package manager, runs as a non-root user. AWSIM_DATA_DIR is
#   pre-set to `/data` so containers persist by default; bind-mount or
#   override the env var to redirect.
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
ARG BUN_VERSION=1

# ---------- UI build stage ----------
FROM --platform=$BUILDPLATFORM oven/bun:${BUN_VERSION}-alpine AS ui-builder
WORKDIR /ui

# Install deps first against just the manifest so layer caching works when
# UI sources change but lockfile doesn't.
COPY ui/package.json ui/bun.lock ./
RUN --mount=type=cache,target=/root/.bun/install/cache \
    bun install --frozen-lockfile

COPY ui/ ./
# vite.config.ts reads ../Cargo.toml for the version stamp.
COPY Cargo.toml /Cargo.toml
RUN --mount=type=cache,target=/root/.bun/install/cache \
    NODE_ENV=production bun run build

# ---------- Rust builder stage ----------
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
# Pull in the freshly-built UI assets so `rust-embed` finds them at compile time.
COPY --from=ui-builder /ui/build ui/build

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

# `/data` is the default persistence directory inside the container. Set
# `AWSIM_DATA_DIR=/data` so plain `docker run` persists by default; bind-mount
# the volume (or set `AWSIM_DATA_DIR=` empty) to override.
ENV AWSIM_PORT=4566 \
    AWSIM_DATA_DIR=/data
EXPOSE 4566
VOLUME ["/data"]

COPY --from=builder /usr/local/bin/awsim /usr/local/bin/awsim

ENTRYPOINT ["/usr/local/bin/awsim"]
