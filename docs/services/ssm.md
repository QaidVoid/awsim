# SSM Parameter Store

AWS Systems Manager Parameter Store for managing configuration data and secrets as hierarchical parameters.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `ssm` |
| Persistence | No |

## Operations

### Parameters
- `PutParameter` — create or update a parameter (String, StringList, SecureString)
- `GetParameter` — retrieve a single parameter by name
- `GetParameters` — retrieve multiple parameters by name in a single call
- `GetParametersByPath` — retrieve all parameters under a path prefix
- `DeleteParameter` — delete a single parameter
- `DeleteParameters` — delete multiple parameters by name
- `DescribeParameters` — list parameters with optional filters
- `GetParameterHistory` — retrieve version history for a parameter

### Tags
- `AddTagsToResource` — add tags to a parameter
- `RemoveTagsFromResource` — remove tags from a parameter
- `ListTagsForResource` — list tags on a parameter

## Example

```bash
# Store a plain string parameter
aws --endpoint-url http://localhost:4567 \
  ssm put-parameter \
  --name /myapp/config/db-host \
  --value "localhost" \
  --type String

# Store a secret (SecureString)
aws --endpoint-url http://localhost:4567 \
  ssm put-parameter \
  --name /myapp/secrets/api-key \
  --value "my-secret-key" \
  --type SecureString

# Retrieve a parameter
aws --endpoint-url http://localhost:4567 \
  ssm get-parameter \
  --name /myapp/config/db-host

# Retrieve all parameters under a path
aws --endpoint-url http://localhost:4567 \
  ssm get-parameters-by-path \
  --path /myapp/config \
  --recursive
```

## Notes

- AWSim stores all parameter types as plain text — `SecureString` values are not actually encrypted.
- Parameter versioning is tracked; each `PutParameter` with `Overwrite: true` creates a new version.
- Path-based retrieval supports recursive listing under any `/prefix`.
