# SSM Parameter Store

AWS Systems Manager Parameter Store for managing configuration data and secrets as hierarchical parameters.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `ssm` |
| Target Prefix | `AmazonSSM` |
| Persistence | No |

## Quick Start

Store configuration parameters and retrieve them by path:

```bash
# Store a plain string parameter
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonSSM.PutParameter" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ssm/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"/myapp/config/db-host","Value":"db.example.com","Type":"String","Description":"Database hostname"}'

# Store a secret value
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonSSM.PutParameter" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ssm/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"/myapp/secrets/api-key","Value":"sk-prod-abc123","Type":"SecureString"}'

# Retrieve a parameter
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonSSM.GetParameter" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ssm/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"/myapp/config/db-host"}'

# Get all parameters under a path
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonSSM.GetParametersByPath" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ssm/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Path":"/myapp/config","Recursive":true}'
```

## Operations

### Parameters
- `PutParameter` — create or update a parameter
  - Input: `Name` (required, e.g., `/myapp/config/db-host`), `Value` (required), `Type` (`String`, `StringList`, `SecureString`), `Description`, `KeyId` (KMS key for SecureString), `Overwrite` (boolean, required to update existing), `Tags`
  - Returns: `Version` (integer, incremented on each update), `Tier` (`Standard` or `Advanced`)
  - `StringList` values are comma-separated strings

- `GetParameter` — retrieve a single parameter by name
  - Input: `Name`, optional `WithDecryption` (boolean, for SecureString)
  - Returns: `Parameter` with `Name`, `Value`, `Type`, `Version`, `LastModifiedDate`, `ARN`

- `GetParameters` — retrieve multiple parameters by name in a single call
  - Input: `Names` (list), optional `WithDecryption`
  - Returns: `Parameters` (found), `InvalidParameters` (names not found)

- `GetParametersByPath` — retrieve all parameters under a path prefix
  - Input: `Path` (e.g., `/myapp`), `Recursive` (boolean, traverse sub-paths), optional `WithDecryption`, `MaxResults`, `NextToken`, `ParameterFilters`
  - Returns: paginated `Parameters` list

- `DeleteParameter` — delete a single parameter
  - Input: `Name`

- `DeleteParameters` — delete multiple parameters by name
  - Input: `Names` (list)
  - Returns: `DeletedParameters`, `InvalidParameters`

- `DescribeParameters` — list parameters with optional filters
  - Input: optional `Filters` (by name, type, key ID), `MaxResults`, `NextToken`
  - Returns: paginated `Parameters` list (values not included — use GetParameter)

- `GetParameterHistory` — retrieve version history for a parameter
  - Input: `Name`, optional `WithDecryption`, `MaxResults`, `NextToken`
  - Returns: paginated `Parameters` list with all historical versions including `Labels`

- `LabelParameterVersion` — attach human-readable labels to a specific parameter version
  - Input: `Name`, `Labels` (list of strings), optional `ParameterVersion` (defaults to current)
  - Returns: `InvalidLabels` (always empty), `ParameterVersion`

### Tags
- `AddTagsToResource` — add tags to a parameter
  - Input: `ResourceType` (`Parameter`), `ResourceId` (parameter name), `Tags`

- `RemoveTagsFromResource` — remove tags from a parameter
  - Input: `ResourceType`, `ResourceId`, `TagKeys`

- `ListTagsForResource` — list tags on a parameter
  - Input: `ResourceType`, `ResourceId`

### Inventory (Systems Manager Agent stubs)
- `PutInventory` — accept inventory data from SSM agents; always returns success
- `GetInventory` — returns `Entities: []`
- `GetInventorySchema` — returns `Schemas: []`

### Run Command
- `SendCommand` — create a command record with a generated `CommandId`
  - Input: `DocumentName` (required), optional `Targets`, `Parameters`, `TimeoutSeconds`
  - Returns: `Command` object with `CommandId`, `Status: "Pending"`, `CreatedDate`

- `ListCommands` — list stored commands
  - Input: optional `CommandId` filter, `MaxResults`
  - Returns: `Commands` list

- `GetCommandInvocation` — retrieve the result of a command for a specific instance
  - Input: `CommandId`, optional `InstanceId`
  - Returns: stub with `Status: "Success"`, empty `StandardOutputContent`

## Curl Examples

```bash
# 1. Store multiple config values
for name in db-host db-port db-name; do
  curl -s http://localhost:4566 \
    -H "Content-Type: application/x-amz-json-1.1" \
    -H "X-Amz-Target: AmazonSSM.PutParameter" \
    -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ssm/aws4_request, SignedHeaders=host, Signature=fake" \
    -d "{\"Name\":\"/myapp/config/$name\",\"Value\":\"value-for-$name\",\"Type\":\"String\"}"
done

# 2. Get multiple parameters at once
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonSSM.GetParameters" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ssm/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Names":["/myapp/config/db-host","/myapp/config/db-port","/myapp/secrets/api-key"],"WithDecryption":true}'

# 3. Update a parameter (requires Overwrite: true)
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonSSM.PutParameter" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ssm/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"/myapp/config/db-host","Value":"new-db.example.com","Type":"String","Overwrite":true}'
```

## SDK Example

```typescript
import {
  SSMClient,
  PutParameterCommand,
  GetParameterCommand,
  GetParametersByPathCommand,
  DeleteParameterCommand,
} from '@aws-sdk/client-ssm';

const ssm = new SSMClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Store configuration
await ssm.send(new PutParameterCommand({
  Name: '/myapp/prod/db-host',
  Value: 'db.internal.example.com',
  Type: 'String',
  Description: 'Production database host',
}));

await ssm.send(new PutParameterCommand({
  Name: '/myapp/prod/db-password',
  Value: 'SuperSecret456!',
  Type: 'SecureString',  // stored as plain text in AWSim
}));

await ssm.send(new PutParameterCommand({
  Name: '/myapp/prod/allowed-regions',
  Value: 'us-east-1,us-west-2,eu-west-1',
  Type: 'StringList',
}));

// Retrieve a single parameter
const { Parameter } = await ssm.send(new GetParameterCommand({
  Name: '/myapp/prod/db-host',
}));
console.log('DB Host:', Parameter?.Value);
console.log('Version:', Parameter?.Version);

// Get all parameters under /myapp/prod/
const allParams: Record<string, string> = {};
let nextToken: string | undefined;

do {
  const { Parameters, NextToken } = await ssm.send(new GetParametersByPathCommand({
    Path: '/myapp/prod',
    Recursive: true,
    WithDecryption: true,
    MaxResults: 10,
    NextToken: nextToken,
  }));

  for (const param of Parameters ?? []) {
    allParams[param.Name!] = param.Value!;
  }

  nextToken = NextToken;
} while (nextToken);

console.log('All config:', allParams);

// Update an existing parameter
await ssm.send(new PutParameterCommand({
  Name: '/myapp/prod/db-host',
  Value: 'db-primary.internal.example.com',
  Type: 'String',
  Overwrite: true,
}));

// Cleanup
await ssm.send(new DeleteParameterCommand({
  Name: '/myapp/prod/db-host',
}));
```

## Behavior Notes

- AWSim stores all parameter types (`String`, `StringList`, `SecureString`) as plain text — `SecureString` values are **not** actually encrypted.
- `WithDecryption: true` is accepted but has no effect since values aren't encrypted.
- Parameter versioning is tracked: each `PutParameter` with `Overwrite: true` increments the version number and creates a new history entry accessible via `GetParameterHistory`.
- Path-based retrieval supports recursive listing under any `/prefix` using `GetParametersByPath` with `Recursive: true`.
- `Overwrite: true` is required to update an existing parameter; omitting it returns a `ParameterAlreadyExists` error.
- Labels attached via `LabelParameterVersion` are stored on each version and returned in `GetParameterHistory`.
- `SendCommand` creates an in-memory command record but does not execute anything. `GetCommandInvocation` always returns `Status: "Success"`.
- Inventory operations (`PutInventory`, `GetInventory`, `GetInventorySchema`) are stubs that always return empty results.
- State is in-memory only and lost on restart.
