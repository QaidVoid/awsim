# AGENTS.md — AWSim Development Guidelines

## Project Overview

AWSim is a fully offline, free, open-source AWS emulator built in Rust with a SvelteKit admin console. It provides a single-binary local AWS development environment with per-service embeddable crates.

## Architecture at a Glance

```
Request → Gateway (Axum) → Protocol Detection → Service Router → Service Crate → Response
                ↓                                       ↓
          Auth (SigV4)                          State Store (in-memory)
```

- **awsim-core**: Gateway, routing, protocol parsers/serializers, state management, error types, `ServiceHandler` trait
- **awsim-auth**: SigV4 parsing and validation
- **awsim-{service}**: One crate per AWS service, each implementing `ServiceHandler`
- **awsim**: Main binary that composes all service crates into a running server
- **ui/**: SvelteKit admin console

## Commit Guidelines

### Commit Eagerly

- **One logical change per commit.** Don't accumulate multiple features or fixes.
- Commit after each meaningful step: new file scaffold, trait implementation, passing test, etc.
- A commit that compiles but has incomplete features is fine. A commit that doesn't compile is not.
- Prefer many small commits over few large ones. This makes review, bisect, and revert trivial.

### Commit Messages

Use conventional commit format:

```
feat(s3): implement CreateBucket and DeleteBucket operations
fix(core): handle missing Content-Type header in protocol detection
refactor(dynamodb): extract expression parser into separate module
test(sqs): add integration tests for FIFO queue deduplication
docs: update service compatibility matrix in README
chore: update dependencies
```

Prefixes: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`, `ci`

Scope is the crate name without `awsim-` prefix: `s3`, `sqs`, `core`, `auth`, `dynamodb`, `ui`, etc.

## Rust Conventions

### General

- **Edition 2024** (or latest stable)
- Format with `rustfmt` (default settings)
- No warnings allowed — treat warnings as errors (`#![deny(warnings)]` in lib.rs)
- Use `clippy` with default lints
- Prefer `thiserror` for error types, `anyhow` only in the binary crate
- Async runtime: **Tokio** (multi-threaded)
- HTTP framework: **Axum 0.8+**

### Crate Structure

Each service crate follows this structure:

```
crates/awsim-{service}/
├── Cargo.toml
├── src/
│   ├── lib.rs          # Public API, ServiceHandler impl, re-exports
│   ├── operations/     # One file per operation or logical group
│   │   ├── mod.rs
│   │   ├── buckets.rs  # (e.g., for S3: CreateBucket, DeleteBucket, ListBuckets)
│   │   ├── objects.rs  # (e.g., for S3: PutObject, GetObject, DeleteObject)
│   │   └── ...
│   ├── state.rs        # Service-specific state/store types
│   ├── types.rs        # Request/response types (or re-export from awsim-models)
│   └── errors.rs       # Service-specific error codes
└── tests/
    └── integration.rs  # Integration tests using awsim-test utilities
```

### ServiceHandler Pattern

Every service crate must implement the `ServiceHandler` trait from `awsim-core`:

```rust
use awsim_core::{ServiceHandler, RequestContext, AwsError, Protocol};
use serde_json::Value;

pub struct S3Service {
    state: S3State,
}

#[async_trait::async_trait]
impl ServiceHandler for S3Service {
    fn service_name(&self) -> &str { "s3" }
    fn protocol(&self) -> Protocol { Protocol::RestXml }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        match operation {
            "CreateBucket" => self.create_bucket(input, ctx).await,
            "DeleteBucket" => self.delete_bucket(input, ctx).await,
            // ...
            _ => Err(AwsError::not_implemented(operation)),
        }
    }
}
```

### Error Handling

- Return `AwsError` with the correct AWS error code, HTTP status, and message.
- Look up the real AWS error codes for each operation — don't guess.
- Example: S3 `NoSuchBucket` returns HTTP 404 with code `NoSuchBucket`.
- Use `AwsError::not_implemented(op)` for operations that aren't implemented yet — this returns a clear error to the caller rather than silently failing.

### State Management

- Use `awsim_core::AccountRegionStore<T>` for state that is namespaced by account + region.
- State types should be `Send + Sync` (use `Arc<DashMap<...>>` or `Arc<RwLock<...>>`).
- Never use `std::sync::Mutex` — use `tokio::sync::Mutex` or `DashMap` for async-safe access.
- Keep state types in `state.rs` within each service crate.

### Dependencies

- Minimize external dependencies per service crate.
- All service crates depend on: `awsim-core`, `serde`, `serde_json`, `async-trait`, `tracing`
- Service-specific deps are fine (e.g., `awsim-s3` may use `md5`, `sha2` for ETags/checksums)
- **Do not** add deps to the workspace root unless truly shared by all crates.

## Protocol Implementation Notes

### awsJson (1.0 / 1.1)

- Request: POST to `/`, body is JSON
- Operation identified by `X-Amz-Target: ServicePrefix.OperationName`
- Response: JSON body with `x-amzn-RequestId` header
- Errors: `{"__type": "ErrorCode", "message": "..."}`

### restJson1

- Request: HTTP method + path pattern, body is JSON
- Each operation has a unique URI pattern (e.g., `POST /2015-03-31/functions`)
- Response: JSON body, status code varies per operation
- Errors: `{"__type": "ErrorCode", "message": "..."}`

### restXml

- Request: HTTP method + path pattern, body is XML (or empty)
- Used primarily by S3 — many operations use headers extensively (`x-amz-*`)
- Response: XML body
- Errors: `<Error><Code>NoSuchBucket</Code><Message>...</Message></Error>`

### awsQuery / ec2Query

- Request: POST to `/`, body is form-urlencoded (`Action=CreateUser&UserName=foo`)
- Response: XML wrapped in `<{Action}Response><{Action}Result>...</{Action}Result><ResponseMetadata>...</ResponseMetadata></{Action}Response>`
- Complex types use dot-notation: `Tags.member.1.Key=Name&Tags.member.1.Value=foo`

## Testing

### Test Categories

1. **Unit tests** — In each crate's `src/` files. Test individual operations, expression parsing, state logic.
2. **Integration tests** — In `tests/` directory. Spin up the server, hit it with real AWS SDK calls.
3. **Conformance tests** — Generated from Smithy models. Validate request/response shapes.

### Writing Tests

```rust
#[tokio::test]
async fn test_create_and_list_buckets() {
    let server = awsim_test::start_server(&[awsim_s3::service()]).await;
    let client = server.s3_client();

    client.create_bucket()
        .bucket("test-bucket")
        .send()
        .await
        .unwrap();

    let buckets = client.list_buckets().send().await.unwrap();
    assert_eq!(buckets.buckets().len(), 1);
    assert_eq!(buckets.buckets()[0].name().unwrap(), "test-bucket");
}
```

### Test Conventions

- Test the AWS SDK interface, not internal functions — this ensures wire compatibility.
- Each test should be self-contained (create its own resources, clean up is implicit since state is per-test).
- Use `awsim-test` helpers to start an in-process server and get pre-configured SDK clients.
- Name tests descriptively: `test_{operation}_{scenario}` (e.g., `test_put_object_with_metadata`)

## SvelteKit UI Conventions

### Stack
- SvelteKit 2 with Svelte 5 (runes)
- Tailwind CSS v4+
- shadcn-svelte for UI components
- TypeScript throughout

### Structure

```
ui/
├── src/
│   ├── routes/
│   │   ├── +layout.svelte       # App shell (sidebar, topbar)
│   │   ├── +page.svelte         # Dashboard
│   │   ├── s3/
│   │   │   ├── +page.svelte     # Bucket list
│   │   │   └── [bucket]/
│   │   │       └── +page.svelte # Object browser
│   │   ├── dynamodb/
│   │   ├── sqs/
│   │   └── ...
│   ├── lib/
│   │   ├── api/                 # API client for AWSim admin endpoints
│   │   ├── components/          # Shared components
│   │   └── stores/              # Svelte stores for global state
│   └── app.html
├── static/
├── package.json
├── svelte.config.js
├── tailwind.config.js
└── vite.config.ts
```

### UI Conventions
- Use Svelte 5 runes (`$state`, `$derived`, `$effect`) — no legacy `$:` or stores API.
- All API calls go through `src/lib/api/` — never call fetch directly from components.
- Dark mode by default, light mode toggle in topbar.
- Responsive: sidebar collapses on mobile.
- No page reloads — use SvelteKit's client-side navigation.

## Adding a New AWS Service

Step-by-step checklist for implementing a new service:

1. **Create the crate:**
   ```bash
   mkdir -p crates/awsim-{service}/src/operations
   ```

2. **Add to workspace** in root `Cargo.toml`

3. **Vendor the Smithy model** into `models/{service}/` (from https://github.com/aws/api-models-aws)

4. **Implement `ServiceHandler`** in `src/lib.rs`

5. **Implement operations** in `src/operations/` — start with the most common ones:
   - Create/Delete/List/Describe for the primary resource
   - The one operation people use most (e.g., `PutItem` for DynamoDB, `SendMessage` for SQS)

6. **Add state types** in `src/state.rs`

7. **Add error codes** in `src/errors.rs` (look up the real AWS error codes)

8. **Register the service** in the main `awsim` binary crate

9. **Write integration tests** using `awsim-test`

10. **Add UI page** in `ui/src/routes/{service}/`

11. **Update the service matrix** in `PLAN.md`

12. **Commit each step separately** — don't batch steps 1-11 into one commit

## Code Quality Checklist

Before submitting work on any service:

- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy` passes with no warnings
- [ ] `cargo test` passes (both unit and integration)
- [ ] All implemented operations return correct AWS error codes for invalid input
- [ ] State is properly namespaced by account + region
- [ ] No `unwrap()` in library code (use `?` or return `AwsError`)
- [ ] No `println!` — use `tracing::{info, debug, warn, error}`
- [ ] Public API is documented with `///` doc comments
- [ ] Operation handler matches AWS SDK behavior (test with real SDK)

## Performance Targets

- Cold start: <500ms
- Idle memory: <10 MiB (base server, no resources created)
- Throughput: >10,000 req/s for simple operations (GetItem, SendMessage)
- Binary size: <30 MiB (release build with all services)
- Per-service crate compile time: <10s incremental

## Out of Scope (For Now)

- Multi-region replication (cross-region state sync)
- IAM policy evaluation (we store policies but don't enforce them in bypass mode)
- Real EC2 instance launching
- Redshift
- CodeBuild, CodePipeline
