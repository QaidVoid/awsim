# ECS

Amazon Elastic Container Service for running and managing containerized applications.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `ecs` |
| Target Prefix | `AmazonEC2ContainerServiceV20141113` |
| Persistence | No |

## Quick Start

Create a cluster, register a task definition, and launch a task:

```bash
# Create a cluster
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonEC2ContainerServiceV20141113.CreateCluster" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ecs/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"clusterName":"my-cluster"}'

# Register a task definition
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonEC2ContainerServiceV20141113.RegisterTaskDefinition" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ecs/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"family":"my-task","containerDefinitions":[{"name":"app","image":"nginx:latest","cpu":256,"memory":512,"essential":true,"portMappings":[{"containerPort":80,"protocol":"tcp"}]}],"requiresCompatibilities":["FARGATE"],"cpu":"256","memory":"512","networkMode":"awsvpc"}'

# Run a task
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonEC2ContainerServiceV20141113.RunTask" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ecs/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"cluster":"my-cluster","taskDefinition":"my-task","count":1,"launchType":"FARGATE","networkConfiguration":{"awsvpcConfiguration":{"subnets":["subnet-abc12345"],"assignPublicIp":"ENABLED"}}}'
```

## Operations

### Clusters
- `CreateCluster` — create an ECS cluster
  - Input: `clusterName` (required), optional `tags`, `settings`, `configuration`
  - Returns: `cluster` with `clusterArn` (e.g., `arn:aws:ecs:us-east-1:000000000000:cluster/my-cluster`), `status` (`ACTIVE`), `registeredContainerInstancesCount`, `runningTasksCount`

- `DeleteCluster` — delete a cluster (must have no running tasks or services)
  - Input: `cluster` (name or ARN)

- `DescribeClusters` — get details of one or more clusters
  - Input: `clusters` (list of names or ARNs), optional `include` (list: `ATTACHMENTS`, `CONFIGURATIONS`, `SETTINGS`, `STATISTICS`, `TAGS`)
  - Returns: `clusters` list, `failures` for missing clusters

- `ListClusters` — list all clusters in the account/region
  - Returns: paginated `clusterArns` list

### Task Definitions
- `RegisterTaskDefinition` — register a new task definition or create a new revision
  - Input: `family` (required), `containerDefinitions` (list with `name`, `image`, `cpu`, `memory`, `essential`, `portMappings`, `environment`, `logConfiguration`), `taskRoleArn`, `executionRoleArn`, `networkMode` (`bridge`, `host`, `awsvpc`), `requiresCompatibilities` (`EC2` or `FARGATE`), `cpu`, `memory`
  - Returns: `taskDefinition` with `taskDefinitionArn` (e.g., `arn:aws:ecs:...:task-definition/my-task:1`), `revision` (auto-incremented)

- `DeregisterTaskDefinition` — deregister a specific task definition revision
  - Input: `taskDefinition` (family:revision or ARN)

- `DescribeTaskDefinition` — get the full task definition including container configs
  - Input: `taskDefinition` (family, family:revision, or ARN)

- `ListTaskDefinitions` — list task definition ARNs with optional family filter
  - Input: optional `familyPrefix`, `status` (`ACTIVE` or `INACTIVE`), `maxResults`, `nextToken`

- `ListTaskDefinitionFamilies` — list task definition family names
  - Input: optional `familyPrefix`, `status`, `maxResults`, `nextToken`

### Services
- `CreateService` — create a long-running service backed by a task definition
  - Input: `cluster`, `serviceName` (required), `taskDefinition`, `desiredCount`, `launchType` (`FARGATE` or `EC2`), `networkConfiguration`, `loadBalancers`, `serviceRegistries`
  - Returns: `service` with `serviceArn`, `status` (`ACTIVE`), `runningCount`, `desiredCount`

- `DeleteService` — delete a service
  - Input: `cluster`, `service`, optional `force` (set to true to delete even if running tasks exist)

- `DescribeServices` — get details of one or more services
  - Input: `cluster`, `services` (list of names or ARNs), optional `include`

- `ListServices` — list services in a cluster
  - Input: `cluster`, optional `maxResults`, `nextToken`, `launchType`

- `UpdateService` — update service configuration
  - Input: `cluster`, `service`, optional `desiredCount`, `taskDefinition`, `networkConfiguration`, `forceNewDeployment`

### Tasks
- `RunTask` — launch one or more tasks from a task definition
  - Input: `cluster`, `taskDefinition`, `count` (default 1), `launchType`, `networkConfiguration`, `overrides`, `tags`
  - Returns: `tasks` list with `taskArn`, `lastStatus` (`PENDING`), `containers`

- `StopTask` — stop a running task
  - Input: `cluster`, `task` (task ARN or ID), `reason` (optional string)

- `DescribeTasks` — get details of one or more tasks
  - Input: `cluster`, `tasks` (list of ARNs), optional `include`

- `ListTasks` — list tasks in a cluster with optional filters
  - Input: `cluster`, optional `family`, `serviceName`, `desiredStatus` (`RUNNING`, `STOPPED`, `PENDING`)

## Curl Examples

```bash
# 1. Create service with 2 replicas
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonEC2ContainerServiceV20141113.CreateService" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ecs/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"cluster":"my-cluster","serviceName":"my-service","taskDefinition":"my-task:1","desiredCount":2,"launchType":"FARGATE","networkConfiguration":{"awsvpcConfiguration":{"subnets":["subnet-abc12345"],"securityGroups":["sg-abc12345"],"assignPublicIp":"ENABLED"}}}'

# 2. List task definitions
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonEC2ContainerServiceV20141113.ListTaskDefinitions" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ecs/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"familyPrefix":"my-task"}'

# 3. Update service desired count
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonEC2ContainerServiceV20141113.UpdateService" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ecs/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"cluster":"my-cluster","service":"my-service","desiredCount":4}'
```

## SDK Example

```typescript
import {
  ECSClient,
  CreateClusterCommand,
  RegisterTaskDefinitionCommand,
  CreateServiceCommand,
  RunTaskCommand,
  ListTasksCommand,
} from '@aws-sdk/client-ecs';

const ecs = new ECSClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create cluster
await ecs.send(new CreateClusterCommand({ clusterName: 'my-cluster' }));

// Register task definition
const { taskDefinition } = await ecs.send(new RegisterTaskDefinitionCommand({
  family: 'my-task',
  containerDefinitions: [{
    name: 'app',
    image: 'nginx:latest',
    cpu: 256,
    memory: 512,
    essential: true,
    portMappings: [{ containerPort: 80, protocol: 'tcp' }],
    environment: [{ name: 'NODE_ENV', value: 'production' }],
  }],
  requiresCompatibilities: ['FARGATE'],
  cpu: '256',
  memory: '512',
  networkMode: 'awsvpc',
}));

console.log('Task Def ARN:', taskDefinition?.taskDefinitionArn);
console.log('Revision:', taskDefinition?.revision);

// Create a service
await ecs.send(new CreateServiceCommand({
  cluster: 'my-cluster',
  serviceName: 'api-service',
  taskDefinition: 'my-task',
  desiredCount: 2,
  launchType: 'FARGATE',
  networkConfiguration: {
    awsvpcConfiguration: {
      subnets: ['subnet-abc12345'],
      securityGroups: ['sg-abc12345'],
      assignPublicIp: 'ENABLED',
    },
  },
}));

// Run a one-off task
const { tasks } = await ecs.send(new RunTaskCommand({
  cluster: 'my-cluster',
  taskDefinition: 'my-task',
  count: 1,
  launchType: 'FARGATE',
}));

console.log('Task ARN:', tasks?.[0]?.taskArn);
```

## Behavior Notes

- ECS in AWSim tracks task definitions, services, and task metadata but does **not** actually launch containers.
- Task revisions are auto-incremented each time `RegisterTaskDefinition` is called for the same `family`.
- Services report `runningCount` equal to `desiredCount` immediately — no actual container health checking or scaling occurs.
- Task `lastStatus` starts as `PENDING` and transitions to `RUNNING` quickly (simulated).
- State is in-memory only and lost on restart.
