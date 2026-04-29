# AWS Cloud Map (Service Discovery)

Namespace + service + instance registry, used by ECS service discovery and any code that wires up `aws-sdk-servicediscovery` directly. Async operations (CreateNamespace, RegisterInstance, DeregisterInstance) collapse to `SUCCESS` immediately so callers can poll `GetOperation` once and move on.

**Endpoint:** `http://localhost:4566`
**Signing name:** `servicediscovery`
**Protocol:** AWS-JSON 1.1 (X-Amz-Target prefix: `Route53AutoNaming_v20170314`)

## Operations

| Group | Operations |
|-------|-----------|
| Namespaces | `CreateHttpNamespace`, `CreatePrivateDnsNamespace`, `CreatePublicDnsNamespace`, `DeleteNamespace`, `GetNamespace`, `ListNamespaces` |
| Services | `CreateService`, `DeleteService`, `GetService`, `ListServices` (filterable on `NAMESPACE_ID`) |
| Instances | `RegisterInstance`, `DeregisterInstance`, `GetInstance`, `ListInstances`, `DiscoverInstances` |
| Operations | `GetOperation`, `ListOperations` |

## Behavior notes

- `DeleteNamespace` and `DeleteService` reject with `ResourceInUse` when child resources still exist.
- Counts on parent resources (`NamespaceServiceCount`, `ServiceInstanceCount`) are kept in sync as instances/services come and go.
- `DiscoverInstances` returns matching instances with `HealthStatus: HEALTHY` and the full attribute map (the `AWS_INSTANCE_IPV4` / `AWS_INSTANCE_PORT` pair ECS task discovery writes).
- The service supports both id-based and name-based identifiers in resolver paths the data plane needs.
