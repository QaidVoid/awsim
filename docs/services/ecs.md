# ECS

Amazon Elastic Container Service for running and managing containerized applications.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `ecs` |
| Persistence | No |

## Operations

### Clusters
- `CreateCluster` — create an ECS cluster
- `DeleteCluster` — delete a cluster
- `DescribeClusters` — get details of one or more clusters
- `ListClusters` — list all clusters in the account/region

### Task Definitions
- `RegisterTaskDefinition` — register a new task definition (or new revision of existing)
- `DeregisterTaskDefinition` — deregister a task definition revision
- `DescribeTaskDefinition` — get the full task definition including container configs
- `ListTaskDefinitions` — list task definition ARNs with optional family filter
- `ListTaskDefinitionFamilies` — list task definition family names

### Services
- `CreateService` — create a long-running service backed by a task definition
- `DeleteService` — delete a service
- `DescribeServices` — get details of one or more services in a cluster
- `ListServices` — list services in a cluster
- `UpdateService` — update service configuration (desired count, task definition, etc.)

### Tasks
- `RunTask` — launch one or more tasks from a task definition
- `StopTask` — stop a running task
- `DescribeTasks` — get details of one or more tasks
- `ListTasks` — list tasks in a cluster with optional filters

## Example

```bash
# Create a cluster
aws --endpoint-url http://localhost:4567 \
  ecs create-cluster \
  --cluster-name my-cluster

# Register a task definition
aws --endpoint-url http://localhost:4567 \
  ecs register-task-definition \
  --family my-task \
  --container-definitions '[{"name":"app","image":"nginx:latest","cpu":256,"memory":512,"essential":true}]'

# Create a service
aws --endpoint-url http://localhost:4567 \
  ecs create-service \
  --cluster my-cluster \
  --service-name my-service \
  --task-definition my-task \
  --desired-count 2

# Run a one-off task
aws --endpoint-url http://localhost:4567 \
  ecs run-task \
  --cluster my-cluster \
  --task-definition my-task
```

## Notes

- ECS in AWSim tracks task definitions, services, and task metadata but does not actually launch containers.
- Task revisions are auto-incremented each time `RegisterTaskDefinition` is called for the same family.
- Services report a desired count but no actual scaling or health checking occurs.
- State is in-memory only and lost on restart.
