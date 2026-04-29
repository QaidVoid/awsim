# Estimated Billing

AWSim ships with a billing meter that watches every request flowing through the gateway, multiplies usage by canonical AWS pricing, and surfaces a rolling estimated monthly bill at `/billing` in the admin console.

The dashboard answers questions like:

- How much would this workload cost on real AWS?
- Which service is dominating my bill?
- At my current burn rate, how long until I hit my budget?
- Is my cost trajectory accelerating, flat, or trending down?

It's a projection, not a contract — see [Approximations](#approximations) below.

## Quick start

Open the admin console (`cd ui && bun run dev`, then `http://localhost:5173/billing`). Hit some AWS APIs and watch the bill tick up:

```bash
export AWS_ENDPOINT_URL=http://localhost:4566
aws s3 mb s3://test-bucket
for i in $(seq 1 1000); do
  aws s3api put-object --bucket test-bucket --key obj-$i --body /etc/hostname
done
```

The Cost Trajectory panel shows the running bill over the last 30 minutes, the Per-Service Breakdown lists each service's contribution, and the Time to Bankruptcy widget projects when your set budget would be exhausted at the current rate.

## What's metered

| Axis | Description | Services |
|---|---|---|
| Per-request | Flat $/call rate (PUT, GET, etc.) | S3, DynamoDB, SQS, SNS, KMS, Secrets Manager, EventBridge, API Gateway, SES, CloudWatch Metrics, Route 53, Kinesis, CloudFront, Cognito |
| Per state-transition | $0.000025 / transition | Step Functions |
| Per character | $/character of input text | Polly, Comprehend |
| Data transfer out | $0.09/GB egress to internet | All metered services |
| Data ingest | $/GB of incoming payload | Firehose, CloudWatch Logs |
| Storage (GB-month) | $/GB stored, sampled point-in-time | S3, DynamoDB, ECR, CloudWatch Logs |
| Compute (GB-second) | duration × memory × rate | Lambda |
| Instance-hours | Running instances × hourly rate | EC2, RDS |

Pricing data lives in `crates/awsim-billing/pricing/<service>.json`, vendored from the AWS Pricing Bulk JSON. Most files are populated by a refresh tool (see [Refreshing pricing](#refreshing-pricing)); a few (EC2, RDS, Polly, Comprehend) are hand-written because their AWS rates are per-instance-type or per-100-character units that need translation.

## How the meter works

The gateway publishes a `RequestEvent` for every request it handles. A spawned tokio task in `awsim-billing` subscribes to the broadcast channel and routes each event to the right counter:

1. **Per-request services** — `OpCounter::record(units, bytes_in, bytes_out, error)` increments the operation counter. `units` is normally 1, but services can override it via metadata response headers (see [Per-call metadata](#per-call-metadata)).
2. **Storage / instance-hour services** — a separate 30-second poll loop in the awsim binary samples on-disk byte counts (S3 BodyStore, DynamoDB SQLite file) and running instance counts (EC2/RDS) and feeds them into trapezoidal-integration accumulators.
3. **Compute (GB-seconds)** — Lambda emits `X-Awsim-Memory-MB` on its response headers; the meter multiplies that × `event.duration_ms` × the rate.

All accumulated cost is stored in pico-USD (1e-12 USD) integer atomics — coarse-grained units truncated tiny per-sample accruals to zero.

## Per-call metadata

Services can attach `X-Awsim-*` response headers that the gateway peels off before the response leaves the building. The billing meter reads them off the resulting `RequestEvent`:

| Header | Type | Used by |
|---|---|---|
| `X-Awsim-Memory-MB` | `u32` | Lambda compute (GB-seconds) |
| `X-Awsim-State-Transitions` | `u32` | Step Functions per-transition billing |
| `X-Awsim-Char-Count` | `u64` | Polly / Comprehend per-character billing |

If a service grows a new billable axis, the typical fix is: have the handler emit a header with the right count, optionally extend `RequestEvent`, then read it off in the meter.

## Persistence

When `--data-dir` is set, the billing state survives restarts via `data/snapshots/billing.json`. The poll loop also fires inside this block (storage / instance counts can't be sampled without `data-dir`). In-memory mode still meters per-request charges but skips storage / instance-hour billing.

## Refreshing pricing

AWS publishes pricing as huge per-service JSON files at `pricing.us-east-1.amazonaws.com`. The billing crate ships a feature-gated refresh binary that pulls the relevant SKUs and writes the slim per-service files we vendor:

```bash
cargo run -p awsim-billing --bin refresh-pricing --features refresh
```

Run it after AWS bumps a rate, then commit the diff under `crates/awsim-billing/pricing/`. The tool handles the per-region offer files (most services), the global bulk file (CloudFront), and the AWSDataTransfer offer (egress rates). Hand-written files for EC2 / RDS / Polly / Comprehend stay untouched.

## Approximations

The bill is honest about what it does *not* model precisely:

- **Lambda memory** defaults to 128 MB when an invocation didn't carry the function's configured memory through. Functions on bigger memory tiers underbill in proportion.
- **Step Functions transitions** are counted per `StateEntered` event in the ASL interpreter's history. Parallel/Map state branches may slightly over- or under-count vs. real AWS.
- **EC2/RDS instance-hours** charge at a baseline rate (`t3.micro` / `db.t3.micro`) regardless of the actual instance type. AWSim's request event doesn't carry the instance type through. Workloads on bigger types underbill in proportion.
- **Polly** charges at the Standard voice tier ($4 / million chars). Neural ($16 / M), Long-form ($100 / M), and Generative ($30 / M) tiers underbill.
- **Cognito MAU** is informational — the per-MAU rate appears on the dashboard but `count` stays at 0 because AWSim doesn't track unique-principal activity over time yet.
- **Athena per-TB scanned** is not metered — AWSim's Athena emulator returns `SUCCEEDED` immediately without scanning any data.

## Admin endpoint

`GET /_awsim/billing` returns the JSON the dashboard renders:

```json
{
  "currency": "USD",
  "elapsed_secs": 423,
  "running_cost_usd": 0.00128,
  "projected_monthly_cost_usd": 7.85,
  "services": [
    {
      "service": "s3",
      "display_name": "Amazon S3",
      "region": "us-east-1",
      "total_cost_usd": 0.00075,
      "request_count": 150,
      "storage_cost_usd": 0.00012,
      "storage_bytes": 5_242_880,
      "dimensions": [
        {
          "description": "PUT/COPY/POST/LIST requests",
          "price_per_request": 0.000005,
          "request_count": 150,
          "cost_usd": 0.00075
        },
        ...
      ]
    },
    ...
  ]
}
```

Useful for piping into other dashboards, alerting, or scripted budget checks.
