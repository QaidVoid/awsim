# Chaos Engine

AWSim's chaos engine injects synthetic AWS errors and latency into the
gateway so application code can be exercised against realistic failure
modes — throttling, regional outages, slow networks — without leaving
your offline emulator.

## When to use it

Chaos rules fire **after** authentication and authorisation but
**before** the request hits the service handler. That makes the engine
a good fit for testing:

- **Retry & backoff** — does your SDK back off correctly when S3 returns
  `503 SlowDown`?
- **Throttle handling** — does your DynamoDB code break the requested
  work into smaller chunks when it sees
  `ProvisionedThroughputExceededException`?
- **Circuit breakers** — does your code open the breaker when KMS goes
  dark?
- **Timeouts** — does your code time out cleanly when Lambda invokes
  spike to 2 seconds?
- **Graceful degradation** — does your service return cached data when
  every backend call fails?

## Anatomy of a rule

A chaos rule is a **match predicate** plus an **effect** plus a
**probability**:

```jsonc
{
  "service": { "kind": "exact", "value": "s3" },
  "operation": { "kind": "any" },
  "probability": 0.05,
  "effect": {
    "kind": "error",
    "status": 503,
    "code": "SlowDown",
    "message": "Please reduce your request rate."
  },
  "enabled": true,
  "label": "preset: flaky-s3"
}
```

- `service.kind` is `"any"` (matches every service) or `"exact"` (one
  signing name, e.g. `s3`).
- `operation.kind` is the same — `"any"` or an exact AWS API call name.
- `probability` ∈ `[0.0, 1.0]`. `1.0` always fires; `0.0` never does.
- `effect.kind` is `"error"`, `"latency"`, or `"both"`.

Rules are evaluated in registration order. The **first** rule whose
predicate matches *and* whose probability roll succeeds wins —
subsequent rules don't fire even if they'd also match.

## Schedules

Rules can be gated by an optional `schedule` containing two
independent components, combined with AND:

- **`window`** — `{ start_ts, end_ts }` unix-seconds, either bound
  optional. Useful for "fire only during this maintenance window" or
  "auto-stop after N seconds".
- **`flap`** — `{ period_secs, active_secs, anchor_ts }`. Active for
  `active_secs` out of every `period_secs`, anchored at
  `anchor_ts`. Useful for "flap on / off every 30s" workloads that
  test flapping connections.

A rule with no schedule is always active (subject to the `enabled`
flag and probability roll).

The CLI exposes three convenience flags on `awsim chaos add`:

```sh
# Fire for the next 5 min then auto-stop.
awsim chaos add --service kms --error '503,KMSInternalException,boom' \
  --ttl-secs 300

# Wait 30s, then start firing.
awsim chaos add --service s3 --latency 200-500 --start-in-secs 30

# Flap: 30s on / 30s off, indefinitely.
awsim chaos add --service '*' --error '503,ServiceUnavailable,...' \
  --flap '30/60'

# Compose: starts in 1 min, lasts 10 min total, flaps 20s on / 40s off.
awsim chaos add --service lambda --operation Invoke --latency 1000-3000 \
  --start-in-secs 60 --ttl-secs 600 --flap '20/60'
```

Schedule fields can also be set directly via the HTTP API or the
"Add rule" dialog in the dashboard.

## Built-in presets

Six common failure-mode bundles ship out of the box:

| Preset | What it does |
| --- | --- |
| `flaky-s3` | 5% of S3 requests return `503 SlowDown`. |
| `ddb-throttle` | 10% of DynamoDB requests return `ProvisionedThroughputExceededException`. |
| `slow-lambda` | All Lambda `Invoke` calls get +500-2000ms latency. |
| `kms-outage` | Every KMS call returns `503 KMSInternalException`. |
| `regional-failover` | 50% of all calls return `503 ServiceUnavailable`. |
| `network-jitter` | Every call gets +50-300ms latency. |

## Three ways to drive it

### 1. Dashboard — `/chaos`

The dashboard at `http://localhost:4566/chaos` (or whatever port awsim
is bound to) gives you:

- preset cards (one click to apply),
- a sortable rules table with a kill-switch toggle on every row,
- an "Add rule" form for ad-hoc scenarios,
- a sparkline of recent injection rate.

### 2. CLI — `awsim chaos`

```sh
# List built-in presets
awsim chaos preset list

# Apply one
awsim chaos preset apply flaky-s3

# Add a custom rule
awsim chaos add \
  --service dynamodb \
  --operation PutItem \
  --probability 0.2 \
  --error '400,ProvisionedThroughputExceededException,backoff'

# Inspect what's running
awsim chaos list
awsim chaos stats

# Reset everything
awsim chaos clear
```

The `--endpoint` flag (or `AWSIM_ENDPOINT` env var) points at a running
awsim instance — defaults to `http://localhost:4566`.

### 3. HTTP API

All admin endpoints live under `/_awsim/chaos/*`:

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/_awsim/chaos/rules` | List active rules + total injections. |
| POST | `/_awsim/chaos/rules` | Add a rule (JSON body). |
| PATCH | `/_awsim/chaos/rules/{id}` | Toggle `enabled` flag. |
| DELETE | `/_awsim/chaos/rules/{id}` | Remove a rule. |
| POST | `/_awsim/chaos/clear` | Drop every rule + reset counters. |
| GET | `/_awsim/chaos/stats` | Total injections + recent ring buffer. |
| GET | `/_awsim/chaos/presets` | List preset names + descriptions. |
| POST | `/_awsim/chaos/presets/{name}` | Apply a preset by name. |

## Worked example: testing exponential backoff

Suppose you've added retry-with-backoff to a service that talks to S3
and you want to confirm it actually retries instead of crashing on
the first error:

```sh
# Make 30% of S3 calls fail with a retryable 503.
awsim chaos add --service s3 --operation '*' \
  --probability 0.3 \
  --error '503,SlowDown,please retry' \
  --label "test: retry-on-slowdown"

# Run your application's S3 integration test suite.
bun test integration/s3-retry.test.ts

# Inspect what fired:
awsim chaos stats
# → Total injections: 17
# → Recent (newest last): ... s3/PutObject ... s3/GetObject ...

# Tear down.
awsim chaos clear
```

If your test passes, you've proved the retry logic survives a 30%
failure rate end-to-end. If it fails, the chaos engine has done its
job — you've found a bug that production would have eventually
exposed.
