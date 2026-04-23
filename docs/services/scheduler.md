# Scheduler

Amazon EventBridge Scheduler for creating managed schedules that invoke targets on a recurring or one-time basis.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `RestJson1` |
| Signing Name | `scheduler` |
| Persistence | Yes |

## Operations

### Schedules
- `CreateSchedule` — create a schedule with a cron/rate expression or one-time date
- `GetSchedule` — get a specific schedule by name
- `ListSchedules` — list schedules with optional group filter
- `DeleteSchedule` — delete a schedule
- `UpdateSchedule` — update a schedule's expression, target, or state

### Schedule Groups
- `CreateScheduleGroup` — create a group to organize schedules
- `GetScheduleGroup` — get a specific schedule group by name
- `ListScheduleGroups` — list all schedule groups
- `DeleteScheduleGroup` — delete a schedule group and all its schedules

## Example

```bash
# Create a rate-based schedule
aws --endpoint-url http://localhost:4567 \
  scheduler create-schedule \
  --name every-hour \
  --schedule-expression "rate(1 hour)" \
  --target '{
    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:my-fn",
    "RoleArn": "arn:aws:iam::000000000000:role/SchedulerRole"
  }' \
  --flexible-time-window '{"Mode":"OFF"}'

# Create a cron schedule
aws --endpoint-url http://localhost:4567 \
  scheduler create-schedule \
  --name daily-report \
  --schedule-expression "cron(0 9 * * ? *)" \
  --target '{
    "Arn": "arn:aws:lambda:us-east-1:000000000000:function:report-fn",
    "RoleArn": "arn:aws:iam::000000000000:role/SchedulerRole"
  }' \
  --flexible-time-window '{"Mode":"OFF"}'

# List schedules
aws --endpoint-url http://localhost:4567 \
  scheduler list-schedules
```

## Notes

- EventBridge Scheduler uses the `RestJson1` protocol with REST routing (`/schedules/{Name}`).
- Schedules are stored and returned correctly but are not actually executed — no Lambda invocations or target calls occur on schedule.
- Persistence is enabled: schedules and schedule groups survive AWSim restarts.
- A `default` schedule group is automatically available.
