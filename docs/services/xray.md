# AWS X-Ray

In-memory trace store. Accepts segment documents from the AWS X-Ray daemon (or any client using the SDK), aggregates them per-trace, and serves the standard listing/aggregation operations the AWS console hits.

**Endpoint:** `http://localhost:4566`
**Signing name:** `xray`
**Protocol:** REST-JSON

## Operations

| Operation | Method / Path |
|-----------|--------------|
| `PutTraceSegments` | `POST /TraceSegments` |
| `BatchGetTraces` | `POST /Traces` |
| `GetTraceSummaries` | `POST /TraceSummaries` |
| `GetServiceGraph` | `POST /ServiceGraph` |
| `GetSamplingRules` / `CreateSamplingRule` / `DeleteSamplingRule` | `POST /GetSamplingRules` etc. |
| `GetSamplingTargets` | `POST /SamplingTargets` |
| `CreateGroup` / `DeleteGroup` / `GetGroups` | `POST /CreateGroup` etc. |

## Behavior notes

- `PutTraceSegments` parses each `TraceSegmentDocuments` entry, appends it to the trace identified by `trace_id`, and updates the trace's start/end/duration plus error/fault/throttle flags.
- Bad segments come back in `UnprocessedTraceSegments` with an `ErrorCode` and `Message`.
- `GetServiceGraph` aggregates per-`name` over all stored traces. Each unique service name becomes a node; edges are not currently inferred.
- `GetSamplingTargets` echoes whatever statistics the client sent so the local reservoir keeps working.
