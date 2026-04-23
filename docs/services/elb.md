# ELB

Elastic Load Balancing v2 (ALB/NLB) for distributing traffic across targets with listener rules.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsQuery` |
| Signing Name | `elasticloadbalancing` |
| Persistence | No |

## Operations

### Load Balancers
- `CreateLoadBalancer` — create an Application or Network Load Balancer
- `DeleteLoadBalancer` — delete a load balancer
- `DescribeLoadBalancers` — list load balancers with optional ARN filter
- `ModifyLoadBalancerAttributes` — update load balancer attributes (deletion protection, idle timeout, etc.)

### Target Groups
- `CreateTargetGroup` — create a target group for load balancer routing
- `DeleteTargetGroup` — delete a target group
- `DescribeTargetGroups` — list target groups with optional filters
- `RegisterTargets` — register EC2 instances or IP addresses as targets
- `DeregisterTargets` — remove targets from a target group
- `DescribeTargetHealth` — check the health status of targets in a group

### Listeners
- `CreateListener` — create a listener on a load balancer (port, protocol, default action)
- `DeleteListener` — delete a listener
- `DescribeListeners` — list listeners for a load balancer

### Rules
- `CreateRule` — create a routing rule with conditions and actions
- `DeleteRule` — delete a routing rule
- `DescribeRules` — list rules for a listener

### Tags
- `AddTags` — add tags to a load balancer, target group, or listener
- `RemoveTags` — remove tags from ELB resources
- `DescribeTags` — list tags for ELB resources

## Example

```bash
# Create an Application Load Balancer
aws --endpoint-url http://localhost:4567 \
  elbv2 create-load-balancer \
  --name my-alb \
  --type application \
  --subnets subnet-111 subnet-222

# Create a target group
aws --endpoint-url http://localhost:4567 \
  elbv2 create-target-group \
  --name my-targets \
  --protocol HTTP \
  --port 8080 \
  --vpc-id vpc-123 \
  --target-type instance

# Create a listener
aws --endpoint-url http://localhost:4567 \
  elbv2 create-listener \
  --load-balancer-arn <lb-arn> \
  --protocol HTTP \
  --port 80 \
  --default-actions Type=forward,TargetGroupArn=<tg-arn>
```

## Notes

- ELB uses the `AwsQuery` protocol (form-encoded POST with `Action=` parameter).
- Load balancers are registered in AWSim but no actual traffic routing or health checking occurs.
- Target health status is returned as `healthy` for all registered targets.
- State is in-memory only and lost on restart.
