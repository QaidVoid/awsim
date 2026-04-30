# What is AWSim?

AWSim is a fully offline AWS emulator written in Rust. It runs 60+ AWS services in a single binary, starts in under 500ms, and requires no internet connection, no AWS account, and no license keys. A built-in billing dashboard projects what your workload would cost on real AWS so you can spot expensive patterns before they hit your real account.

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

## IAM Policy Enforcement

AWSim includes a real IAM policy evaluation engine that implements AWS authorization semantics — identity policies, resource policies, permissions boundaries, SCPs, and session policies with all 26 condition operators. Enforcement is **opt-in** via `AWSIM_IAM_ENFORCE=true` so existing tests remain unaffected. Enabling it lets you unit-test policy documents end-to-end against S3, DynamoDB, KMS, SQS, SNS, Secrets Manager, Lambda, and IAM. See the [IAM Enforcement guide](/guide/iam-enforcement).

## Estimated Billing

AWSim ships with a billing meter that subscribes to every request that flows through the gateway, multiplies usage by canonical AWS pricing (vendored from the public AWS Pricing Bulk JSON), and surfaces a rolling estimated monthly bill at `/billing` in the admin console. Currently models per-request, byte-ingest, data-transfer-out, GB-month storage, GB-second compute, instance-hours, state-transition and per-character billing axes across 22+ services. See the [Billing guide](/guide/billing) for what's metered and how the rates were sourced.

## What AWSim is not

- It does not make any network calls to AWS at runtime.
- By default it does not enforce IAM policies — you must opt in with `AWSIM_IAM_ENFORCE=true`.
- It is not 100% API-compatible with AWS. Edge cases and rarely-used parameters may behave differently or be unimplemented.
- The billing dashboard is a *projection*, not a contract. A few services' billing axes (Lambda's per-function memory, Step Functions' transition counts, EC2/RDS instance types) round to safe defaults; consult the [Billing guide](/guide/billing) for known approximations.
