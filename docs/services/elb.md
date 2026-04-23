# ELB

Elastic Load Balancing v2 (ALB/NLB) for distributing traffic across targets with listener rules.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsQuery` |
| Signing Name | `elasticloadbalancing` |
| Persistence | No |

ELB uses the `AwsQuery` protocol: `POST` requests with `Content-Type: application/x-www-form-urlencoded` and an `Action=` parameter. The CLI command is `elbv2`.

## Quick Start

Create a load balancer, target group, and listener:

```bash
# Create an Application Load Balancer
aws --endpoint-url http://localhost:4566 \
  elbv2 create-load-balancer \
  --name my-alb \
  --type application \
  --scheme internet-facing \
  --subnets subnet-abc12345 subnet-def67890

# Create a target group
aws --endpoint-url http://localhost:4566 \
  elbv2 create-target-group \
  --name my-targets \
  --protocol HTTP \
  --port 8080 \
  --vpc-id vpc-abc12345 \
  --target-type ip \
  --health-check-path /health

# Create an HTTP listener
aws --endpoint-url http://localhost:4566 \
  elbv2 create-listener \
  --load-balancer-arn arn:aws:elasticloadbalancing:us-east-1:000000000000:loadbalancer/app/my-alb/abc123 \
  --protocol HTTP \
  --port 80 \
  --default-actions Type=forward,TargetGroupArn=arn:aws:elasticloadbalancing:us-east-1:000000000000:targetgroup/my-targets/abc123
```

## Operations

### Load Balancers
- `CreateLoadBalancer` — create an Application or Network Load Balancer
  - Input: `Names.member.1` (required), `Type` (`application` or `network`), `Scheme` (`internet-facing` or `internal`), `Subnets.member.N` (list of subnet IDs), `SecurityGroups.member.N` (list of SG IDs), `IpAddressType` (`ipv4` or `dualstack`), `Tags`
  - Returns: `LoadBalancers.member` list with `LoadBalancerArn`, `DNSName`, `State.Code` (`active`)

- `DeleteLoadBalancer` — delete a load balancer
  - Input: `LoadBalancerArn`

- `DescribeLoadBalancers` — list load balancers with optional ARN filter
  - Input: optional `LoadBalancerArns.member.N`, `Names.member.N`, `Marker`, `PageSize`

- `ModifyLoadBalancerAttributes` — update attributes (deletion protection, idle timeout, access logs, etc.)
  - Input: `LoadBalancerArn`, `Attributes.member.N` (list of `{Key, Value}`)

### Target Groups
- `CreateTargetGroup` — create a target group for load balancer routing
  - Input: `Name` (required), `Protocol` (`HTTP`, `HTTPS`, `TCP`, `TLS`, `UDP`), `Port`, `VpcId`, `TargetType` (`instance`, `ip`, `lambda`), `HealthCheckPath`, `HealthCheckProtocol`, `HealthCheckIntervalSeconds`, `HealthyThresholdCount`
  - Returns: `TargetGroups.member` list with `TargetGroupArn`

- `DeleteTargetGroup` — delete a target group
- `DescribeTargetGroups` — list target groups with optional filters

- `RegisterTargets` — register EC2 instances or IP addresses as targets
  - Input: `TargetGroupArn`, `Targets.member.N` (list of `{Id, Port}` where `Id` is an instance ID or IP)

- `DeregisterTargets` — remove targets from a target group
  - Input: `TargetGroupArn`, `Targets.member.N`

- `DescribeTargetHealth` — check the health status of targets in a group
  - Input: `TargetGroupArn`, optional `Targets.member.N`
  - Returns: `TargetHealthDescriptions.member` with `Target.Id`, `TargetHealth.State` (always `healthy` in AWSim)

### Listeners
- `CreateListener` — create a listener on a load balancer
  - Input: `LoadBalancerArn`, `Protocol`, `Port`, `DefaultActions.member.1` (e.g., `Type=forward,TargetGroupArn=...`)
  - Returns: `Listeners.member` with `ListenerArn`

- `DeleteListener` — delete a listener
- `DescribeListeners` — list listeners for a load balancer

### Rules
- `CreateRule` — create a routing rule with conditions and actions
  - Input: `ListenerArn`, `Conditions.member.N` (path patterns, host headers, etc.), `Actions.member.N`, `Priority` (integer)
  - Returns: `Rules.member` with `RuleArn`, `Priority`

- `DeleteRule` — delete a routing rule
- `DescribeRules` — list rules for a listener

### Tags
- `AddTags` — add tags to a load balancer, target group, or listener
  - Input: `ResourceArns.member.N`, `Tags.member.N`

- `RemoveTags` — remove tags from ELB resources
- `DescribeTags` — list tags for ELB resources

## Curl Examples

```bash
# 1. Create a load balancer via curl
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/elasticloadbalancing/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=CreateLoadBalancer' \
  --data-urlencode 'Name=my-alb' \
  --data-urlencode 'Type=application' \
  --data-urlencode 'Subnets.member.1=subnet-abc12345' \
  --data-urlencode 'Subnets.member.2=subnet-def67890'

# 2. Describe load balancers
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/elasticloadbalancing/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=DescribeLoadBalancers'

# 3. Register targets in a target group
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/elasticloadbalancing/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=RegisterTargets' \
  --data-urlencode 'TargetGroupArn=arn:aws:elasticloadbalancing:us-east-1:000000000000:targetgroup/my-targets/abc123' \
  --data-urlencode 'Targets.member.1.Id=10.0.1.100' \
  --data-urlencode 'Targets.member.1.Port=8080'
```

## SDK Example

```typescript
import {
  ElasticLoadBalancingV2Client,
  CreateLoadBalancerCommand,
  CreateTargetGroupCommand,
  CreateListenerCommand,
  DescribeTargetHealthCommand,
} from '@aws-sdk/client-elastic-load-balancing-v2';

const elb = new ElasticLoadBalancingV2Client({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create load balancer
const { LoadBalancers } = await elb.send(new CreateLoadBalancerCommand({
  Name: 'my-alb',
  Type: 'application',
  Scheme: 'internet-facing',
  Subnets: ['subnet-abc12345', 'subnet-def67890'],
}));
const lbArn = LoadBalancers?.[0]?.LoadBalancerArn!;
console.log('LB ARN:', lbArn);
console.log('DNS:', LoadBalancers?.[0]?.DNSName);

// Create target group
const { TargetGroups } = await elb.send(new CreateTargetGroupCommand({
  Name: 'my-targets',
  Protocol: 'HTTP',
  Port: 8080,
  VpcId: 'vpc-abc12345',
  TargetType: 'ip',
  HealthCheckPath: '/health',
}));
const tgArn = TargetGroups?.[0]?.TargetGroupArn!;

// Create listener
await elb.send(new CreateListenerCommand({
  LoadBalancerArn: lbArn,
  Protocol: 'HTTP',
  Port: 80,
  DefaultActions: [{ Type: 'forward', TargetGroupArn: tgArn }],
}));

// Check target health
const { TargetHealthDescriptions } = await elb.send(new DescribeTargetHealthCommand({
  TargetGroupArn: tgArn,
}));
console.log('Targets:', TargetHealthDescriptions?.map(t => ({
  id: t.Target?.Id,
  state: t.TargetHealth?.State,
})));
```

## Behavior Notes

- ELB uses the `AwsQuery` protocol — the AWS CLI uses `elbv2` (not `elb`) for ALB/NLB resources.
- Load balancers are registered in AWSim but no actual traffic routing or health checking occurs.
- `DescribeTargetHealth` always returns `healthy` for all registered targets.
- `DNSName` for load balancers follows the pattern `{name}-{id}.{region}.elb.amazonaws.com`.
- State is in-memory only and lost on restart.
