# Secrets Manager

AWS Secrets Manager for storing, rotating, and retrieving secrets such as database credentials and API keys.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `secretsmanager` |
| Persistence | No |

## Operations

- `CreateSecret` — create a new secret with a string or binary value
- `GetSecretValue` — retrieve the current or a specific version of a secret
- `PutSecretValue` — create a new version of an existing secret
- `DescribeSecret` — get metadata about a secret (no value returned)
- `ListSecrets` — list all secrets in the account/region
- `UpdateSecret` — update secret metadata (description, KMS key) or value
- `DeleteSecret` — mark a secret for deletion with an optional recovery window
- `RestoreSecret` — cancel a pending deletion and restore the secret
- `TagResource` — add tags to a secret
- `UntagResource` — remove tags from a secret

## Example

```bash
# Create a secret
aws --endpoint-url http://localhost:4567 \
  secretsmanager create-secret \
  --name my-db-password \
  --description "Production DB password" \
  --secret-string "s3cur3p@ss"

# Retrieve the secret value
aws --endpoint-url http://localhost:4567 \
  secretsmanager get-secret-value \
  --secret-id my-db-password

# Rotate the secret value
aws --endpoint-url http://localhost:4567 \
  secretsmanager put-secret-value \
  --secret-id my-db-password \
  --secret-string "n3wp@ss!"

# Retrieve the previous version
aws --endpoint-url http://localhost:4567 \
  secretsmanager get-secret-value \
  --secret-id my-db-password \
  --version-stage AWSPREVIOUS
```

## Notes

- Version stages `AWSCURRENT` and `AWSPREVIOUS` are tracked automatically when `PutSecretValue` is called.
- Deleted secrets with a recovery window remain accessible via their ARN for restoration but return `InvalidRequestException` on `GetSecretValue`.
- `ForceDeleteWithoutRecovery: true` immediately removes the secret permanently.
- No automatic rotation is performed — rotation is a stub only.
