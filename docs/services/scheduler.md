# Scheduler

Amazon EventBridge Scheduler for creating managed schedules that invoke targets on a recurring or one-time basis.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `RestJson1` |
| Signing Name | `scheduler` |
| Persistence | Yes |

EventBridge Scheduler uses REST-style routing with JSON bodies. Paths follow `/schedules/{Name}` and `/schedule-groups/{Name}`.

## Quick Start

Create a schedule group, add a rate-based schedule, and list all schedules:

```bash
# Create a schedule group
curl -s -X POST http://localhost:4566/schedule-groups/my-group \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/scheduler/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Tags":{"team":"platform"}}'

# Create a rate-based schedule in the group
curl -s -X POST http://localhost:4566/schedules/every-hour \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/scheduler/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{
    "ScheduleExpression": "rate(1 hour)",
    "GroupName": "my-group",
    "Target": {
      "Arn": "arn:aws:lambda:us-east-1:000000000000:function:hourly-job",
      "RoleArn": "arn:aws:iam::000000000000:role/SchedulerRole"
    },
    "FlexibleTimeWindow": {"Mode": "OFF"},
    "State": "ENABLED"
  }'

# List all schedules
curl -s http://localhost:4566/schedules \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/scheduler/aws4_request, SignedHeaders=host, Signature=fake"
```

## Operations

### Schedules
- `CreateSchedule` — create a schedule with a cron/rate expression or one-time date
  - Path: `POST /schedules/{Name}`
  - Input: `ScheduleExpression` (e.g., `rate(5 minutes)`, `cron(0 9 * * ? *)`, `at(2026-12-31T23:59:59)`), `Target` (`{Arn, RoleArn, Input, SqsParameters, EventBridgeParameters, ...}`), `FlexibleTimeWindow` (`{Mode: "OFF"/"FLEXIBLE", MaximumWindowInMinutes}`), `GroupName` (default: `default`), `State` (`ENABLED` or `DISABLED`), `Description`, `StartDate`, `EndDate`
  - Returns: `ScheduleArn`

- `GetSchedule` — get a specific schedule by name
  - Path: `GET /schedules/{Name}`
  - Input: optional `GroupName` (default: `default`)
  - Returns: full schedule configuration including `ScheduleExpression`, `Target`, `State`, `CreationDate`

- `ListSchedules` — list schedules with optional group filter
  - Path: `GET /schedules`
  - Input: optional `GroupName`, `MaxResults`, `NextToken`, `NamePrefix`, `State`
  - Returns: paginated `Schedules` list

- `DeleteSchedule` — delete a schedule
  - Path: `DELETE /schedules/{Name}`
  - Input: optional `GroupName`

- `UpdateSchedule` — update a schedule's expression, target, or state
  - Path: `PUT /schedules/{Name}`
  - Input: same as `CreateSchedule` — include all fields (full replacement)
  - Returns: `ScheduleArn`

### Schedule Groups
- `CreateScheduleGroup` — create a group to organize schedules
  - Path: `POST /schedule-groups/{Name}`
  - Input: optional `Tags`
  - Returns: `ScheduleGroupArn`

- `GetScheduleGroup` — get a specific schedule group by name
  - Path: `GET /schedule-groups/{Name}`
  - Returns: `Name`, `Arn`, `State`, `CreationDate`, `LastModificationDate`

- `ListScheduleGroups` — list all schedule groups
  - Path: `GET /schedule-groups`
  - Input: optional `MaxResults`, `NextToken`, `NamePrefix`
  - Returns: paginated `ScheduleGroups` list

- `DeleteScheduleGroup` — delete a schedule group and all its schedules
  - Path: `DELETE /schedule-groups/{Name}`

## Curl Examples

```bash
# 1. Create a cron-based schedule (every day at 9 AM UTC)
curl -s -X POST http://localhost:4566/schedules/daily-report \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/scheduler/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{
    "ScheduleExpression": "cron(0 9 * * ? *)",
    "ScheduleExpressionTimezone": "America/New_York",
    "Target": {
      "Arn": "arn:aws:lambda:us-east-1:000000000000:function:generate-report",
      "RoleArn": "arn:aws:iam::000000000000:role/SchedulerRole",
      "Input": "{\"reportType\":\"daily\",\"format\":\"pdf\"}"
    },
    "FlexibleTimeWindow": {"Mode": "FLEXIBLE","MaximumWindowInMinutes": 15},
    "State": "ENABLED",
    "Description": "Daily report generation"
  }'

# 2. Create a one-time schedule (future date)
curl -s -X POST http://localhost:4566/schedules/migration-job \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/scheduler/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{
    "ScheduleExpression": "at(2026-12-01T02:00:00)",
    "Target": {
      "Arn": "arn:aws:lambda:us-east-1:000000000000:function:run-migration",
      "RoleArn": "arn:aws:iam::000000000000:role/SchedulerRole"
    },
    "FlexibleTimeWindow": {"Mode": "OFF"}
  }'

# 3. Get a specific schedule
curl -s http://localhost:4566/schedules/daily-report \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/scheduler/aws4_request, SignedHeaders=host, Signature=fake"
```

## SDK Example

```typescript
import {
  SchedulerClient,
  CreateScheduleCommand,
  CreateScheduleGroupCommand,
  ListSchedulesCommand,
  UpdateScheduleCommand,
} from '@aws-sdk/client-scheduler';

const scheduler = new SchedulerClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create a schedule group
await scheduler.send(new CreateScheduleGroupCommand({
  Name: 'data-jobs',
}));

// Create a rate-based schedule targeting Lambda
const { ScheduleArn } = await scheduler.send(new CreateScheduleCommand({
  Name: 'hourly-cleanup',
  GroupName: 'data-jobs',
  ScheduleExpression: 'rate(1 hour)',
  Target: {
    Arn: 'arn:aws:lambda:us-east-1:000000000000:function:cleanup-job',
    RoleArn: 'arn:aws:iam::000000000000:role/SchedulerRole',
    Input: JSON.stringify({ dryRun: false }),
  },
  FlexibleTimeWindow: { Mode: 'OFF' },
  State: 'ENABLED',
  Description: 'Run cleanup every hour',
}));

console.log('Schedule ARN:', ScheduleArn);

// Create a schedule targeting SQS
await scheduler.send(new CreateScheduleCommand({
  Name: 'queue-processor',
  ScheduleExpression: 'rate(5 minutes)',
  Target: {
    Arn: 'arn:aws:sqs:us-east-1:000000000000:processing-queue',
    RoleArn: 'arn:aws:iam::000000000000:role/SchedulerRole',
    SqsParameters: { MessageGroupId: 'scheduler' },
  },
  FlexibleTimeWindow: { Mode: 'OFF' },
  State: 'ENABLED',
}));

// List all schedules in a group
const { Schedules } = await scheduler.send(new ListSchedulesCommand({
  GroupName: 'data-jobs',
}));

console.log('Schedules:', Schedules?.map(s => `${s.Name} (${s.State})`));
```

## Behavior Notes

- Schedules are stored and returned correctly but are **not actually executed** — no Lambda invocations or target calls occur on schedule triggers.
- Persistence is enabled: schedules and schedule groups survive AWSim restarts.
- A `default` schedule group is automatically available and cannot be deleted.
- Schedule expressions follow the same syntax as real EventBridge Scheduler: `rate(N unit)`, `cron(...)`, or `at(...)`.
- `ScheduleExpressionTimezone` is stored but not evaluated (schedules don't run at all in AWSim).
- Targets can be Lambda, SQS, SNS, Step Functions, ECS tasks, EventBridge event buses, and more — all accepted but none invoked.
