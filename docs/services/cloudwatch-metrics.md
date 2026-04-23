# CloudWatch Metrics

Amazon CloudWatch for publishing custom metrics, querying metric statistics, managing alarms, and dashboards.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsQuery` |
| Signing Name | `monitoring` |
| Persistence | No |

## Operations

### Metrics
- `PutMetricData` — publish custom metric data points to CloudWatch
- `ListMetrics` — list metrics with optional namespace and name filters
- `GetMetricStatistics` — get aggregated statistics (Average, Sum, Min, Max, Count) for a metric
- `GetMetricData` — query one or more metrics with metric math expressions

### Alarms
- `PutMetricAlarm` — create or update an alarm on a metric threshold
- `DescribeAlarms` — list alarms with optional filters
- `DeleteAlarms` — delete one or more alarms
- `SetAlarmState` — manually set the state of an alarm (OK, ALARM, INSUFFICIENT_DATA)

### Dashboards
- `PutDashboard` — create or update a CloudWatch dashboard
- `GetDashboard` — retrieve a dashboard by name
- `ListDashboards` — list all dashboards in the account/region
- `DeleteDashboards` — delete one or more dashboards

## Example

```bash
# Publish a custom metric
aws --endpoint-url http://localhost:4567 \
  cloudwatch put-metric-data \
  --namespace MyApp \
  --metric-name RequestCount \
  --value 42 \
  --unit Count

# Create an alarm
aws --endpoint-url http://localhost:4567 \
  cloudwatch put-metric-alarm \
  --alarm-name high-error-rate \
  --namespace MyApp \
  --metric-name ErrorCount \
  --threshold 100 \
  --comparison-operator GreaterThanThreshold \
  --evaluation-periods 1 \
  --period 60 \
  --statistic Sum \
  --alarm-actions arn:aws:sns:us-east-1:000000000000:alerts

# Get metric statistics
aws --endpoint-url http://localhost:4567 \
  cloudwatch get-metric-statistics \
  --namespace MyApp \
  --metric-name RequestCount \
  --start-time 2024-01-01T00:00:00Z \
  --end-time 2024-01-02T00:00:00Z \
  --period 3600 \
  --statistics Average
```

## Notes

- CloudWatch Metrics uses the `AwsQuery` protocol (form-encoded POST with `Action=` parameter) with service name `monitoring`.
- Alarm actions (SNS notifications, Auto Scaling, etc.) are recorded but not executed.
- `SetAlarmState` allows testing alarm-driven workflows without waiting for thresholds to be breached.
- State is in-memory only and lost on restart.
