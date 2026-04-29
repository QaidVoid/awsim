# Application Auto Scaling

Stores scalable targets, scaling policies, and scheduled actions for the resource types Terraform/CDK templates wire up most often (ECS services, Lambda provisioned concurrency, DynamoDB read/write capacity, AppStream fleets, etc.). The emulator never actually evaluates policies or executes scaling decisions — `DescribeScalingActivities` always returns an empty list.

**Endpoint:** `http://localhost:4566`
**Signing name:** `application-autoscaling`
**Protocol:** AWS-JSON 1.1 (X-Amz-Target prefix: `AnyScaleFrontendService`)

## Operations

| Operation | Notes |
|-----------|-------|
| `RegisterScalableTarget` | Idempotent on `(ServiceNamespace, ResourceId, ScalableDimension)` — re-register fills in only the fields you supply, keeping prior values for the rest. |
| `DeregisterScalableTarget` | Cascades to delete every policy and scheduled action attached to the target. |
| `DescribeScalableTargets` | Filter by `ResourceIds` and `ScalableDimension`. |
| `PutScalingPolicy` | Rejects with `ObjectNotFoundException` if no scalable target exists for the supplied `(ServiceNamespace, ResourceId, ScalableDimension)`. Returns `Alarms: []`. |
| `DeleteScalingPolicy` / `DescribeScalingPolicies` | Standard CRUD. |
| `PutScheduledAction` / `DeleteScheduledAction` / `DescribeScheduledActions` | Schedule string is stored verbatim — no cron parsing. |
| `DescribeScalingActivities` | Always `{ "ScalingActivities": [] }`. |

## Behavior notes

- All resources are keyed by `{ServiceNamespace}|{ResourceId}|{ScalableDimension}`.
- The default `RoleARN` for new scalable targets points at the standard service-linked role ARN.
- `PolicyType` defaults to `TargetTrackingScaling` when omitted on `PutScalingPolicy`.
