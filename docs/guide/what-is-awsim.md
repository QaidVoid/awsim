# What is AWSim?

AWSim is a fully offline AWS emulator written in Rust. It runs 37 AWS services in a single binary, starts in under 500ms, and requires no internet connection, no AWS account, and no license keys.

## Why AWSim?

LocalStack Community Edition was effectively discontinued as a free tool — most useful features moved behind a paid tier. AWSim fills that gap: a permissively licensed, fully offline alternative that you can run anywhere without registering.

AWSim is dual-licensed under **MIT** and **Apache-2.0**. You can use it in commercial projects.

## Architecture

AWSim is built on [Axum](https://github.com/tokio-rs/axum), a Rust async web framework. When a request arrives:

1. **Protocol detection** — the gateway inspects the `Content-Type`, `X-Amz-Target`, and path to determine which AWS wire protocol is in use (JSON, Query, REST-XML, REST-JSON).
2. **Service routing** — the `x-amz-target` header or URL path is used to identify the target service and operation.
3. **Operation dispatch** — the request is forwarded to the appropriate service handler, which reads/writes in-memory state protected by `DashMap` (a concurrent hash map).
4. **Event bus** — cross-service integrations (SNS fan-out, SQS→Lambda polling, etc.) use an internal async event bus.
5. **Persistence** — if `--data-dir` is set, service state is serialized to JSON snapshots on a 30-second timer and on shutdown.

All state lives in memory. Snapshots are written atomically (write to a temp file, then rename).

## What AWSim is not

- It does not make any network calls to AWS.
- It does not implement IAM policy evaluation for authorization — credentials are accepted but not validated.
- It is not 100% API-compatible with AWS. Edge cases and rarely-used parameters may behave differently or be unimplemented.
