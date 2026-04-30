# Seeding

AWSim ships a bulk-seeder that fills services with realistic fake data. It bypasses SigV4 + the gateway and writes straight into each service's internal state, so a 10k-user / 1k-table seed completes in well under a second instead of taking the full per-request hit.

Three entry points sit on top of the same admin endpoints:

1. **`/seed` UI page** — service cards with count inputs and Run buttons. Quick + interactive.
2. **`/_awsim/seed/<service>` admin endpoints** — script directly with `curl` or your favourite client.
3. **`awsim seed --file seed.toml`** — reproducible scenarios for CI / dev fixtures.

## UI

Open **Admin → Seed data** in the sidebar. Each service has its own card with sensible defaults:

| Card | Inputs |
|------|--------|
| Cognito users | Pool (dropdown of existing pools) + count |
| DynamoDB | Tables + items per table |
| S3 | Buckets + objects per bucket + body bytes |
| Secrets Manager | Count |
| SQS | Queues + messages per queue |

Cards show a status badge (`idle` / `running` / `done` / `error`) and a result line with what was actually created. Run buttons are disabled while the request is in flight so double-clicks don't kick off duplicate seeds.

## Admin endpoints

All endpoints accept JSON `POST` bodies and return a counts summary. Defaults: account from `--account-id`, region from `--region`.

### `POST /_awsim/seed/cognito-users`

```json
{
  "pool_id":  "us-east-1_abcdef",
  "count":    10000,
  "prefix":   "seed-",          // optional, default "seed-"
  "password": "Seed-Pass-1234!" // optional, default meets typical pool policy
}
```

Each user gets a random name + email, status biased ~80% `CONFIRMED` / ~15% `FORCE_CHANGE_PASSWORD` / ~5% `UNCONFIRMED`, ~95% `enabled`, `email_verified` flipped on for ~90% of confirmed users. Capped at **100 000 / call**.

Returns: `{ "created": N, "skipped": M, "pool_id": "..." }`.

### `POST /_awsim/seed/dynamodb`

```json
{
  "tables":          5,
  "items_per_table": 1000,
  "prefix":          "seed",   // optional
  "account":         "...",    // optional
  "region":          "..."     // optional
}
```

Each table gets a single `id` (String) hash key and `items_per_table` items with random `name`, `email`, `score`, and `active` attributes. Items go through the same SQLite store the gateway path uses, so they're immediately queryable. Capped at **1 000 tables / 100 000 items per table**.

Returns: `{ "tables_created": N, "items_created": M, "errors": [] }`.

### `POST /_awsim/seed/s3`

```json
{
  "buckets":            5,
  "objects_per_bucket": 100,
  "body_bytes":         256,   // capped at 64 KiB
  "prefix":             "seed",
  "account":            "...",
  "region":             "..."
}
```

Each object body is `body_bytes` of deterministic ASCII so the seed is fast and repeatable; ETags match the body content. Capped at **500 buckets / 10 000 objects per bucket**.

Returns: `{ "buckets_created": N, "objects_created": M }`.

### `POST /_awsim/seed/secrets`

```json
{
  "count":   20,
  "prefix":  "seed",
  "account": "...",
  "region":  "..."
}
```

Each secret carries a JSON body shaped like a typical credential blob: `{ username, password, host, port }`. Capped at **50 000 / call**.

Returns: `{ "created": N }`.

### `POST /_awsim/seed/sqs`

```json
{
  "queues":             5,
  "messages_per_queue": 50,
  "prefix":             "seed",
  "account":            "...",
  "region":             "..."
}
```

Each message body is a random fake sentence with the correct `md5_of_body` hash. Queues are created as standard (non-FIFO). URLs are built against awsim's own port so the seeded queues are immediately usable by SDK clients. Capped at **1 000 queues / 100 000 messages per queue**.

Returns: `{ "queues_created": N, "messages_created": M }`.

## CLI: `awsim seed`

Use the CLI subcommand for reproducible scenarios — commit a `seed.toml` to your repo and any teammate (or CI job) can recreate the same fixture with one command:

```bash
awsim seed --file fixtures/dev.toml

# Override the endpoint:
awsim seed --file fixtures/dev.toml --endpoint http://staging-awsim.local:4566

# Or via env var:
AWSIM_ENDPOINT=http://staging-awsim.local:4566 awsim seed --file fixtures/dev.toml
```

Endpoint resolution: `--endpoint` flag → `endpoint` in the TOML file → `AWSIM_ENDPOINT` env var → `http://localhost:4566`.

Output is one ✓-prefixed line per service with the JSON response, plus a final summary line:

```
✓ Cognito users: {"created":1000,"skipped":0,"pool_id":"us-east-1_abc"}
✓ DynamoDB: {"tables_created":5,"items_created":5000,"errors":[]}
✓ S3: {"buckets_created":5,"objects_created":500}
✓ Secrets Manager: {"created":20}
✓ SQS: {"queues_created":5,"messages_created":250}
✓ Seed complete.
```

A failing service stops the run and surfaces the error message.

### TOML shape

Each section is optional; omit a service to skip it. `cognito_users` is an array (one entry per pool); the other sections are single tables.

```toml
endpoint = "http://localhost:4566"   # optional

[[cognito_users]]
pool_id = "us-east-1_abcdef"
count   = 1000

# Multiple pools? Just add another [[cognito_users]] block.
[[cognito_users]]
pool_id = "us-east-1_xyz789"
count   = 500
prefix  = "load-"

[dynamodb]
tables          = 5
items_per_table = 1000

[s3]
buckets             = 5
objects_per_bucket  = 100
body_bytes          = 256

[secrets]
count = 20

[sqs]
queues             = 5
messages_per_queue = 50
```

## Cleanup

The seeders use a `seed-` prefix by default for resource names so the synthetic data is easy to spot. Drop a [named snapshot](/guide/persistence#named-snapshots) before seeding if you want to roll back cleanly:

```bash
curl -XPOST http://localhost:4566/_awsim/snapshots/pre-seed
# ...seed and use...
curl -XPOST http://localhost:4566/_awsim/snapshots/pre-seed/load
```

Or just restart with a fresh `--data-dir` (or no data-dir) to nuke everything.

## When not to use seed

The seeders prioritise speed over realism — they don't fire Lambda triggers, don't generate stream records, and don't go through SigV4 / IAM. If you need any of that, write a small script that uses the AWS SDK against AWSim instead. The seed endpoints are meant for "I just need 10k rows in a table" workloads.
