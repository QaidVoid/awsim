# Cross-service events

AWSim runs a single in-process event bus that every service can publish
to and that subscribers consume off the request thread. Today the bus
carries two kinds of payloads:

- **Per-API-call records** built by the gateway after every dispatched
  request. These are the foundation for CloudTrail-style audit, AWS
  Config drift snapshots, and the EventBridge `aws.<service>`
  auto-emission catalog.
- **Service-specific notifications** that don't map to an API call but
  still need cross-service fan-out — SQS visibility changes, S3
  `ObjectCreated:Put`, SNS deliveries, DynamoDB streams, and the like.

This page covers when each shape applies and how to wire a new
producer or consumer.

## The bus

`awsim_core::EventBus` is a `tokio::sync::broadcast::Sender<InternalEvent>`
wrapper. The gateway holds a single instance on `AppState::event_bus`
and hands it to every service that needs it. Subscribers call
`bus.subscribe()` to get a `Receiver` and drain it from a background
task.

Every event is an `InternalEvent`:

```rust
pub struct InternalEvent {
    pub source: String,         // "s3", "sns", "eventbridge", ...
    pub event_type: String,     // "s3:ObjectCreated:Put" / awsim:ApiCall / ...
    pub region: String,
    pub account_id: String,
    pub detail: serde_json::Value,
}
```

The bus is **best-effort, in-process, lossy on lag**. A subscriber that
falls behind by more than 1 024 events is told it lagged via
`broadcast::error::RecvError::Lagged(skipped)` and resumes from the
current head. Producers never block waiting for slow consumers.

## Per-API-call events (`awsim:ApiCall`)

The gateway builds an `ApiCallDetail` after every dispatched request,
whether it succeeded or failed, and publishes it via
`EventBus::publish_api_call`. The detail mirrors CloudTrail's event
shape so subscribers can render or persist it directly:

```rust
pub struct ApiCallDetail {
    pub event_id: String,
    pub event_source: String,           // "s3.amazonaws.com"
    pub event_name: String,             // "PutObject"
    pub event_time_epoch: f64,
    pub source_ip: Option<String>,
    pub user_agent: Option<String>,
    pub user_identity_arn: Option<String>,
    pub user_identity_account: Option<String>,
    pub request_parameters: Option<serde_json::Value>,
    pub response_elements: Option<serde_json::Value>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub http_status: u16,
}
```

Sensitive parameters (passwords, KMS ciphertexts, signed-URL bodies)
are scrubbed before publishing. Subscribers should treat
`request_parameters` and `response_elements` as already redacted and
must not re-derive them from the raw request body.

The reserved `event_type` constant is `API_CALL_EVENT_TYPE =
"awsim:ApiCall"`. Subscribers filter by that constant before parsing
`detail` as `ApiCallDetail`.

### Subscribing as a sink

Today three services tap the bus this way: CloudTrail (records every
event for `LookupEvents`), EventBridge (synthesizes the documented
`aws.<service>` event for matching catalog entries), and
ResourceGroupsTaggingApi (watches `Tag*` / `Untag*` for the
cross-service tag store).

The CloudTrail subscriber is the canonical pattern: a free
`spawn_event_subscriber(bus, store)` function that detaches a tokio
task and drains the receiver until the bus is dropped. The gateway
calls it once at startup with `state.event_bus` and the service's
per-account/region store handle.

```rust
pub fn spawn_event_subscriber(
    bus: &EventBus,
    store: AccountRegionStore<MyServiceState>,
) -> tokio::task::JoinHandle<()> {
    let mut rx = bus.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) if event.event_type == API_CALL_EVENT_TYPE => {
                    let detail: ApiCallDetail =
                        match serde_json::from_value(event.detail) {
                            Ok(d) => d,
                            Err(_) => continue,
                        };
                    let state = store.get(&event.account_id, &event.region);
                    state.record(detail);
                }
                Ok(_) => {} // not for us
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(skipped, "subscriber lagged; events dropped");
                }
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    })
}
```

Add a sink when the new behaviour needs to react to *every* API call,
not just one service's. Single-service reactions (S3 trigger -> Lambda
ESM, SQS visibility expiry, etc.) belong in the publishing service's
tick loop or worker pool, not in a bus subscriber.

## Service-specific notifications

The same bus also carries non-API events services publish directly.
Examples:

- **S3** publishes `s3:ObjectCreated:Put` / `Copy` / `Delete` so Lambda
  ESM, SNS notification configs, and SQS notification configs can
  fan out.
- **SNS** publishes `sns:Publish` so SQS / Lambda subscribers see the
  message envelope.
- **DynamoDB** publishes stream-change events for downstream consumers.
- **EventBridge** publishes `eventbridge:TargetInvocation` per matched
  rule so the cross-service router can deliver to the target.

The `event_type` for these is service-defined, not the reserved
`awsim:ApiCall`. Producers call `EventBus::publish` (not
`publish_api_call`) with a service-shaped `detail` payload.

## Producer rules of thumb

- **API-call records belong in the gateway, not in services.** Don't
  call `publish_api_call` from inside a handler — the gateway already
  does it for every dispatched request and a second emission produces
  duplicates that subscribers can't deduplicate.
- **Service notifications belong in the service.** When SQS expires a
  visibility timeout or S3 finalises an upload, the originating
  service publishes; the bus does fan-out.
- **Never block the producer.** Publishing is non-blocking and the
  bus drops on lag rather than back-pressuring producers.

## Consumer rules of thumb

- **One task per subscriber.** Each `bus.subscribe()` returns an
  independent receiver; share state between subscribers via your
  service's own store, not by cloning receivers.
- **Filter by `event_type` before deserialising.** The bus is
  multi-tenant — a subscriber that parses every event into its own
  schema will spend CPU rejecting unrelated payloads.
- **Treat lag as a warning, not a fatal.** Log `skipped` and continue;
  the bus has already advanced.
- **Use the per-event `(account_id, region)`, not your own context.**
  The bus carries events from many tenants and routing must respect
  the publisher's scope.
