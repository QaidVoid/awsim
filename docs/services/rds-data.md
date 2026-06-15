# RDS Data API (`rds-data`)

The Amazon RDS Data API is the HTTP SQL endpoint for Aurora: clients POST a SQL
statement and receive rows back, without holding a persistent database
connection. AWSim implements it as a separate service from RDS, backed by a
real PostgreSQL so statements run with genuine PostgreSQL semantics rather than
a simulated dialect.

## Opt-in feature

Unlike the rest of AWSim, the Data API depends on Docker at runtime: it starts a
real PostgreSQL container per cluster on demand. It is therefore compiled out of
the default build and enabled with a Cargo feature:

```bash
cargo run -p awsim --features rds-data
```

With the feature disabled (the default), AWSim stays fully offline and in-memory
and the `rds-data` service is simply not registered.

## Configuration

- `AWSIM_RDS_DATA_CONTAINER_RUNTIME`: the container runtime executable. Defaults
  to `docker`. Podman is drop-in compatible for the commands used here (`run`,
  `rm`), so set this to `podman` to use it instead.
- `AWSIM_RDS_DATA_PG_IMAGE`: the PostgreSQL image to run. Any `postgres:NN` tag
  works (14 through 18), since the Data API only uses wire-protocol features
  that are stable across those releases. Defaults to `postgres:16-alpine`.
- `AWSIM_RDS_DATA_PG_HOST`: the host AWSim connects to in order to reach a
  container's published port. Defaults to `127.0.0.1` for the common case where
  AWSim runs directly on the host.

### Using Podman

Set `AWSIM_RDS_DATA_CONTAINER_RUNTIME=podman`. The `run -d --rm -e -p` and
`rm -f` commands AWSim issues are all Podman-compatible, and because AWSim shells
out to the CLI rather than the Docker socket, Podman's different socket path does
not matter. When AWSim itself runs in a rootless Podman container reaching the
host, use `host.containers.internal` (Podman's equivalent of
`host.docker.internal`) for `AWSIM_RDS_DATA_PG_HOST`.

### Running AWSim itself inside Docker

When AWSim runs in a container and talks to the host's Docker socket (the
Docker-out-of-Docker pattern, mounting `/var/run/docker.sock`), the PostgreSQL
containers it starts are siblings on the host daemon, not nested. Their
published ports live on the host, not on AWSim's own loopback. Set
`AWSIM_RDS_DATA_PG_HOST` to a host the sibling is reachable through (for example
`host.docker.internal`); AWSim then publishes the container port on all
interfaces so the connection succeeds.

## Operations

- `ExecuteStatement` runs a single SQL statement
  - Input: `resourceArn`, `sql`, optional `parameters`, `transactionId`, `includeResultMetadata`
  - Returns: `numberOfRecordsUpdated` for data-modifying statements, or `records` (and `columnMetadata` when requested) for row-returning statements
- `BatchExecuteStatement` runs one statement once per parameter set
  - Input: `resourceArn`, `sql`, optional `parameterSets`, `transactionId`
  - Returns: `updateResults`
- `BeginTransaction` opens a transaction
  - Input: `resourceArn`
  - Returns: `transactionId`
- `CommitTransaction` / `RollbackTransaction` close a transaction
  - Input: `transactionId`
  - Returns: `transactionStatus`

### Parameters

Named parameters use the `:name` syntax in the SQL and a `parameters` list of
`{name, value}` entries, where `value` is a Data API field such as
`{ "longValue": 1 }` or `{ "stringValue": "abc" }`. AWSim substitutes them as
escaped SQL literals before execution; the `::type` cast operator and string
literals are left untouched.

## Scope and limitations

- The Data API operates independently of the RDS control plane. It does not read
  the cluster's `HttpEndpointEnabled` flag (that lives in the separate `rds`
  service), and it manages its own PostgreSQL containers keyed by `resourceArn`.
- Each cluster maps to a single database inside its container; the request
  `database` and `schema` fields are not used to switch databases.
- Result values are mapped to typed fields where the column type is known
  (integers, floats, booleans, byte arrays) and to string values otherwise.
