# Memory + Diagnostics

AWSim is built to keep RSS bounded under burst workloads — DDB query loops, bulk imports, 10k-user pools — without leaking. This page is a runbook for "is awsim using too much memory?" and the knobs you have when the answer is yes.

## What's already in place

The default profile aims for a few hundred MiB resident with idle baseline ≤ 10 MiB:

- **jemalloc allocator** on Linux + macOS — returns memory to the OS more aggressively than glibc, so idle RSS doesn't ratchet upward after each burst.
- **Per-service SQLite stores** for the high-volume services (DynamoDB, CloudWatch Logs, CloudWatch Metrics, Kinesis, SES). Items + log events live on disk, not in memory.
- **AWS-defined response caps** on DynamoDB Query / Scan (1 MiB), BatchGetItem (16 MB), TransactGetItems (4 MB), BatchWriteItem (25 items / 400 KB / item), TransactWriteItems (100 actions), PutItem / UpdateItem (400 KB) — clients paginate via `LastEvaluatedKey` / `UnprocessedKeys` exactly like real AWS.
- **Tokio runtime caps** — `--max-blocking-threads 32`, `--max-concurrent-requests 256` ([see Configuration](/guide/configuration)).
- **Lazy SQLite connection pools** — `min_idle=1, max_size=4` per store, tight `cache_size` + `mmap_size` PRAGMAs.
- **SES retention sweep** — hourly, configurable via `--ses-retention-hours` (default 30 days, `0` to disable).
- **Hourly chaos rule sweep**, request-detail ring capped at 200 entries, broadcast channels at 256 / 1024.

If you still see growth, the diagnostics below tell you _which_ subsystem is responsible.

## Linux: tracking RSS over time

```bash
# One-shot RSS reading
ps -o pid,rss,vsz,comm -p $(pgrep awsim)
# RSS is in KiB.

# Sampled time-series (5s tick)
while true; do
  printf '%s %s KiB\n' "$(date +%T)" \
    "$(awk '/^VmRSS:/{print $2}' /proc/$(pgrep awsim)/status)"
  sleep 5
done | tee rss.log

# Detailed breakdown — heap vs stack vs anon vs files
cat /proc/$(pgrep awsim)/status \
  | grep -E '^Vm(Peak|Size|RSS|Data|Stk|Exe|Lib|HWM)'
```

Gotcha: if multiple `awsim` processes are running, `pgrep awsim` returns more than one PID. Pin to a specific PID instead.

## `/_awsim/debug/objects`

The most important endpoint when investigating growth. Walks every major in-memory store and reports counts, plus the process's RSS / VmHWM / VmSize / VmData / VmPeak.

```bash
# Baseline before workload
curl -s http://localhost:4566/_awsim/debug/objects > /tmp/before.json

# ...do whatever's leaking...

# After
curl -s http://localhost:4566/_awsim/debug/objects > /tmp/after.json

# Diff to see what grew
diff <(jq -S . /tmp/before.json) <(jq -S . /tmp/after.json)
```

The full payload covers:

- **`process`** — RSS / VmSize / VmHWM / VmPeak / VmData (Linux only — null elsewhere)
- **`app`** — request count, request_details ring size, registered services, broadcast subscriber counts (catches SSE leaks), chaos rules + recent injections, uptime
- **`cognito`** — user pools count, mfa sessions, totals, plus per-pool breakdown (users / groups / clients / auth-events / devices / revoked-refresh-tokens)
- **`billing`** — account-region buckets + op-counter / storage / compute / resource row totals
- **`sqlite`** — row counts per persistent service, DynamoDB DB file size

What to scan first when diffing:

- `process.rss_bytes` — confirms RSS actually grew between snapshots.
- `app.request_event_subscribers` / `internal_event_subscribers` — if either climbs forever, a tab/client never released its SSE subscriber.
- `app.request_details` — should cap at 200; if it's higher the ring eviction broke.
- `cognito.totals.auth_events` — capped per user; runaway means the cap broke.
- `billing.op_counters_total` — only grows when new (service, operation) combos appear; flat under steady-state.
- `sqlite.*_rows` — if these explode and the DB file grows, retention sweep isn't running.

## `/observability` UI page

Open **Admin → Observability**. Polls `/_awsim/debug/objects` every 5 s, renders an RSS sparkline and tables of every section above. **Snapshot baseline** captures the current values; subsequent renders show signed deltas next to every cell so a leak shows up as a stream of orange `+N` annotations against the structure that's growing.

Keep the page closed if you're investigating per-second RSS cycling — its own polling is one of the most common sources of small periodic allocations (12 MiB cycles aligning with jemalloc's 10 s decay window).

## When the diagnostic shows nothing growing

If every counter is flat but RSS still creeps up between bursts, that's **not a leak** — it's allocator behaviour. glibc-style allocators hold freed pages in fragmented free lists; the larger the burst, the larger the residue. Three options in increasing aggressiveness:

```bash
# Option 1: tighter jemalloc decay (Linux + macOS only)
MALLOC_CONF="dirty_decay_ms:1000,muzzy_decay_ms:0,narenas:2" ./awsim

# Option 2: lower concurrency cap so per-op spikes stay smaller
./awsim --max-concurrent-requests 64

# Option 3: lower blocking pool — caps SQLite IO parallelism
./awsim --max-blocking-threads 8
```

Tradeoffs: option 1 costs nothing functional. Options 2 + 3 reduce throughput in exchange for flatter RSS curves, useful in tight containers.

## When a single op spikes RSS hard

The DDB caps stop unbounded queries from materialising entire partitions, but a *single* op that allocates a lot at once (a 10k-item Query before the cap landed, a multi-megabyte BatchWriteItem) can still spike to 1+ GiB transiently. Memory drops back, but jemalloc holds the dirty pages for ~10 s before unmapping them.

Fix at the workload layer: stick to the AWS-defined limits (1 MiB Query/Scan responses, 100 keys per BatchGetItem, etc.). Or further reduce `--max-concurrent-requests` so 256 simultaneous bursts don't compound.

## Glossary

| Field | What it is |
|-------|------------|
| `RSS` (`VmRSS`) | Resident set size — bytes mapped into RAM right now. |
| `VmHWM` | Peak RSS since the process started. |
| `VmSize` | Total virtual address space (RSS + swapped + reserved). |
| `VmData` | Data + heap + stack — the chunk allocators carve from. |
| `VmPeak` | Peak `VmSize` since process start. |
| Dirty pages | Pages allocator freed but kept mapped, ready to reuse. |
| Muzzy pages | Pages `madvise(MADV_FREE)`'d — kernel may reclaim. |
