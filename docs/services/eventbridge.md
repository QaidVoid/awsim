# EventBridge

Amazon EventBridge event bus and rule engine for routing events between AWS services and applications.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `events` |
| Persistence | No |

## Operations

### Event Buses
- `CreateEventBus` — create a custom event bus
- `DeleteEventBus` — delete an event bus
- `DescribeEventBus` — get details of an event bus
- `ListEventBuses` — list all event buses

### Rules
- `PutRule` — create or update an event rule with a schedule or event pattern
- `DeleteRule` — delete a rule
- `DescribeRule` — get details of a rule
- `ListRules` — list rules optionally filtered by event bus
- `EnableRule` — enable a disabled rule
- `DisableRule` — disable an active rule

### Targets
- `PutTargets` — add targets to a rule
- `RemoveTargets` — remove targets from a rule
- `ListTargetsByRule` — list targets associated with a rule

### Events
- `PutEvents` — publish events to an event bus

### Tags
- `TagResource` — add tags to an event bus or rule
- `UntagResource` — remove tags from a resource
- `ListTagsForResource` — list tags on a resource

## Example

```bash
# Create a custom event bus
aws --endpoint-url http://localhost:4567 \
  events create-event-bus \
  --name my-app-events

# Put a scheduled rule (every 5 minutes)
aws --endpoint-url http://localhost:4567 \
  events put-rule \
  --name every-5-minutes \
  --schedule-expression "rate(5 minutes)" \
  --state ENABLED

# Put a target on the rule
aws --endpoint-url http://localhost:4567 \
  events put-targets \
  --rule every-5-minutes \
  --targets '[{"Id":"1","Arn":"arn:aws:lambda:us-east-1:000000000000:function:my-fn"}]'

# Publish a custom event
aws --endpoint-url http://localhost:4567 \
  events put-events \
  --entries '[{"Source":"myapp","DetailType":"OrderPlaced","Detail":"{\"orderId\":\"123\"}","EventBusName":"my-app-events"}]'
```

## Notes

- Events published via `PutEvents` are accepted and stored but no actual event routing or invocation of targets occurs in AWSim.
- The default event bus is automatically available as `default`.
- Schedule expressions and event patterns are stored but not evaluated.
