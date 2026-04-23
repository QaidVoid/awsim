# CloudWatch Metrics

Amazon CloudWatch for publishing custom metrics, querying metric statistics, managing alarms, and dashboards.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsQuery` |
| Signing Name | `monitoring` |
| Persistence | No |

CloudWatch Metrics uses the `AwsQuery` protocol: `POST` requests with `Content-Type: application/x-www-form-urlencoded` and an `Action=` parameter.

## Quick Start

Publish a custom metric and create an alarm on it:

```bash
# Publish a custom metric
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/monitoring/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=PutMetricData' \
  --data-urlencode 'Namespace=MyApp' \
  --data-urlencode 'MetricData.member.1.MetricName=RequestCount' \
  --data-urlencode 'MetricData.member.1.Value=42' \
  --data-urlencode 'MetricData.member.1.Unit=Count'

# Create an alarm
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/monitoring/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=PutMetricAlarm' \
  --data-urlencode 'AlarmName=high-error-rate' \
  --data-urlencode 'Namespace=MyApp' \
  --data-urlencode 'MetricName=ErrorCount' \
  --data-urlencode 'Threshold=100' \
  --data-urlencode 'ComparisonOperator=GreaterThanThreshold' \
  --data-urlencode 'EvaluationPeriods=1' \
  --data-urlencode 'Period=60' \
  --data-urlencode 'Statistic=Sum' \
  --data-urlencode 'AlarmActions.member.1=arn:aws:sns:us-east-1:000000000000:alerts'
```

## Operations

### Metrics
- `PutMetricData` — publish custom metric data points to CloudWatch
  - Input: `Namespace` (required, e.g., `MyApp`), `MetricData` (list of `{MetricName, Value, Unit, Timestamp, Dimensions}`)
  - `Unit` options: `Seconds`, `Microseconds`, `Milliseconds`, `Bytes`, `Kilobytes`, `Megabytes`, `Gigabytes`, `Terabytes`, `Bits`, `Kilobits`, `Megabits`, `Gigabits`, `Terabits`, `Percent`, `Count`, `Bytes/Second`, `Kilobytes/Second`, `Megabytes/Second`, `Gigabytes/Second`, `Terabytes/Second`, `Bits/Second`, `Kilobits/Second`, `Megabits/Second`, `Gigabits/Second`, `Terabits/Second`, `Count/Second`, `None`
  - Returns: empty response (HTTP 200)

- `ListMetrics` — list metrics with optional namespace and name filters
  - Input: optional `Namespace`, `MetricName`, `Dimensions`, `NextToken`
  - Returns: paginated `Metrics` list with `Namespace`, `MetricName`, `Dimensions`

- `GetMetricStatistics` — get aggregated statistics for a metric over a time range
  - Input: `Namespace`, `MetricName`, `StartTime` (ISO 8601), `EndTime` (ISO 8601), `Period` (seconds), `Statistics` (list: `Average`, `Sum`, `SampleCount`, `Maximum`, `Minimum`)
  - Returns: `Datapoints` list with `Timestamp`, the requested statistic values, `Unit`

- `GetMetricData` — query one or more metrics with optional metric math expressions
  - Input: `MetricDataQueries` (list of `{Id, MetricStat, Expression}`), `StartTime`, `EndTime`
  - Returns: `MetricDataResults` per query

### Alarms
- `PutMetricAlarm` — create or update an alarm on a metric threshold
  - Input: `AlarmName` (required), `Namespace`, `MetricName`, `Threshold`, `ComparisonOperator` (`GreaterThanThreshold`, `LessThanThreshold`, `GreaterThanOrEqualToThreshold`, `LessThanOrEqualToThreshold`), `EvaluationPeriods`, `Period` (seconds), `Statistic`, optional `AlarmActions`, `OKActions`, `InsufficientDataActions`
  - Returns: empty response; alarm is created in `OK` state

- `DescribeAlarms` — list alarms with optional filters
  - Input: optional `AlarmNames`, `StateValue` (`OK`, `ALARM`, `INSUFFICIENT_DATA`), `MaxRecords`, `NextToken`
  - Returns: `MetricAlarms` list with full alarm configuration

- `DeleteAlarms` — delete one or more alarms
  - Input: `AlarmNames` (list)

- `SetAlarmState` — manually override the state of an alarm (useful for testing alarm-triggered workflows)
  - Input: `AlarmName`, `StateValue` (`OK`, `ALARM`, `INSUFFICIENT_DATA`), `StateReason` (required string), optional `StateReasonData` (JSON)

### Dashboards
- `PutDashboard` — create or update a CloudWatch dashboard
  - Input: `DashboardName`, `DashboardBody` (JSON string with widget configurations)
  - Returns: `DashboardValidationMessages` (empty if valid)

- `GetDashboard` — retrieve a dashboard by name
  - Input: `DashboardName`
  - Returns: `DashboardBody` (JSON), `DashboardArn`, `DashboardName`

- `ListDashboards` — list all dashboards in the account/region
  - Input: optional `DashboardNamePrefix`, `NextToken`
  - Returns: `DashboardEntries` list

- `DeleteDashboards` — delete one or more dashboards
  - Input: `DashboardNames` (list)

## Curl Examples

```bash
# 1. Publish metric with dimensions
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/monitoring/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=PutMetricData' \
  --data-urlencode 'Namespace=MyApp' \
  --data-urlencode 'MetricData.member.1.MetricName=Latency' \
  --data-urlencode 'MetricData.member.1.Value=250' \
  --data-urlencode 'MetricData.member.1.Unit=Milliseconds' \
  --data-urlencode 'MetricData.member.1.Dimensions.member.1.Name=Service' \
  --data-urlencode 'MetricData.member.1.Dimensions.member.1.Value=UserService'

# 2. Force alarm into ALARM state for testing
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/monitoring/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=SetAlarmState' \
  --data-urlencode 'AlarmName=high-error-rate' \
  --data-urlencode 'StateValue=ALARM' \
  --data-urlencode 'StateReason=Testing alarm notification'

# 3. Get metric statistics
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/monitoring/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=GetMetricStatistics' \
  --data-urlencode 'Namespace=MyApp' \
  --data-urlencode 'MetricName=RequestCount' \
  --data-urlencode 'StartTime=2026-01-01T00:00:00Z' \
  --data-urlencode 'EndTime=2026-01-02T00:00:00Z' \
  --data-urlencode 'Period=3600' \
  --data-urlencode 'Statistics.member.1=Sum'
```

## SDK Example

```typescript
import {
  CloudWatchClient,
  PutMetricDataCommand,
  PutMetricAlarmCommand,
  SetAlarmStateCommand,
  DescribeAlarmsCommand,
} from '@aws-sdk/client-cloudwatch';

const cw = new CloudWatchClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Publish metrics
await cw.send(new PutMetricDataCommand({
  Namespace: 'MyApp',
  MetricData: [
    { MetricName: 'RequestCount', Value: 42, Unit: 'Count' },
    { MetricName: 'ErrorCount', Value: 3, Unit: 'Count' },
  ],
}));

// Create alarm
await cw.send(new PutMetricAlarmCommand({
  AlarmName: 'too-many-errors',
  Namespace: 'MyApp',
  MetricName: 'ErrorCount',
  Threshold: 10,
  ComparisonOperator: 'GreaterThanThreshold',
  EvaluationPeriods: 1,
  Period: 60,
  Statistic: 'Sum',
  AlarmActions: ['arn:aws:sns:us-east-1:000000000000:alerts'],
}));

// Manually trigger alarm for testing
await cw.send(new SetAlarmStateCommand({
  AlarmName: 'too-many-errors',
  StateValue: 'ALARM',
  StateReason: 'Manual trigger for testing',
}));

// List alarms in ALARM state
const { MetricAlarms } = await cw.send(new DescribeAlarmsCommand({
  StateValue: 'ALARM',
}));
console.log('Active alarms:', MetricAlarms?.map(a => a.AlarmName));
```

## Behavior Notes

- CloudWatch Metrics uses the `AwsQuery` protocol with service name `monitoring` (not `cloudwatch`).
- Alarm actions (SNS notifications, Auto Scaling policies, etc.) are recorded but not executed when an alarm state changes.
- `SetAlarmState` is useful for testing alarm-driven workflows (e.g., SNS fan-out) without waiting for metric thresholds to be crossed.
- `GetMetricStatistics` returns simulated datapoints based on what was published via `PutMetricData`.
- State is in-memory only and lost on restart.
