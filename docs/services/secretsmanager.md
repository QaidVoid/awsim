# Secrets Manager

AWS Secrets Manager for storing, rotating, and retrieving secrets such as database credentials and API keys.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `secretsmanager` |
| Target Prefix | `secretsmanager` |
| Persistence | No |

## Quick Start

Create a secret, retrieve it, and rotate its value:

```bash
# Create a secret with a string value
SECRET_ARN=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: secretsmanager.CreateSecret" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/secretsmanager/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"my-db-password","Description":"Production DB password","SecretString":"s3cur3p@ss"}' \
  | jq -r '.ARN')

echo "Secret ARN: $SECRET_ARN"

# Retrieve the secret value
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: secretsmanager.GetSecretValue" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/secretsmanager/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"SecretId":"my-db-password"}'
```

## Operations

- `CreateSecret` — create a new secret with a string or binary value
  - Input: `Name` (required, the secret name), `SecretString` (string value, e.g., JSON or plain text) or `SecretBinary` (base64-encoded binary), optional `Description`, `KmsKeyId`, `Tags`
  - Returns: `ARN` (e.g., `arn:aws:secretsmanager:us-east-1:000000000000:secret:my-db-password-AbCdEf`), `Name`, `VersionId`
  - Initial version is staged as `AWSCURRENT`

- `GetSecretValue` — retrieve the current or a specific version of a secret
  - Input: `SecretId` (name or ARN), optional `VersionId` or `VersionStage` (`AWSCURRENT`, `AWSPREVIOUS`)
  - Returns: `SecretString` or `SecretBinary`, `VersionId`, `VersionStages`, `Name`, `ARN`, `CreatedDate`
  - Returns `ResourceNotFoundException` if the secret doesn't exist
  - Returns `InvalidRequestException` if the secret is pending deletion

- `PutSecretValue` — create a new version of an existing secret
  - Input: `SecretId`, `SecretString` or `SecretBinary`, optional `ClientRequestToken`
  - Automatically promotes the new version to `AWSCURRENT` and demotes the previous to `AWSPREVIOUS`
  - Returns: `ARN`, `Name`, `VersionId`, `VersionStages`

- `DescribeSecret` — get metadata about a secret (value is NOT returned)
  - Input: `SecretId`
  - Returns: `Name`, `ARN`, `Description`, `CreatedDate`, `LastAccessedDate`, `LastChangedDate`, `VersionIdsToStages` (map of version ID to stage list), `Tags`

- `ListSecrets` — list all secrets in the account/region
  - Input: optional `MaxResults`, `NextToken`, `Filters`
  - Returns: paginated `SecretList` with `Name`, `ARN`, `Description`, `LastChangedDate`

- `UpdateSecret` — update secret metadata (description, KMS key) or value
  - Input: `SecretId`, optional `Description`, `KmsKeyId`, `SecretString`, `SecretBinary`

- `DeleteSecret` — mark a secret for deletion with an optional recovery window
  - Input: `SecretId`, optional `RecoveryWindowInDays` (7–30, default 30), `ForceDeleteWithoutRecovery` (boolean, immediate deletion)
  - Returns: `ARN`, `Name`, `DeletionDate`

- `RestoreSecret` — cancel a pending deletion and restore the secret
  - Input: `SecretId`
  - Returns: `ARN`, `Name`

- `TagResource` — add tags to a secret
  - Input: `SecretId`, `Tags` (list of `{Key, Value}`)

- `UntagResource` — remove tags from a secret
  - Input: `SecretId`, `TagKeys` (list of keys)

## Curl Examples

```bash
# 1. Create a JSON secret (common pattern for DB credentials)
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: secretsmanager.CreateSecret" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/secretsmanager/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"/myapp/prod/db","Description":"Production database credentials","SecretString":"{\"host\":\"db.example.com\",\"port\":5432,\"username\":\"app_user\",\"password\":\"SuperSecret123!\",\"dbname\":\"myapp\"}"}'

# 2. Retrieve a secret by name
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: secretsmanager.GetSecretValue" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/secretsmanager/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"SecretId":"/myapp/prod/db"}'

# 3. Rotate the secret value (creates new version)
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: secretsmanager.PutSecretValue" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/secretsmanager/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"SecretId":"/myapp/prod/db","SecretString":"{\"host\":\"db.example.com\",\"port\":5432,\"username\":\"app_user\",\"password\":\"NewPassword456!\",\"dbname\":\"myapp\"}"}'

# 4. List all secrets
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: secretsmanager.ListSecrets" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/secretsmanager/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{}'
```

## SDK Example

```typescript
import {
  SecretsManagerClient,
  CreateSecretCommand,
  GetSecretValueCommand,
  PutSecretValueCommand,
  DescribeSecretCommand,
  DeleteSecretCommand,
} from '@aws-sdk/client-secrets-manager';

const sm = new SecretsManagerClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create a structured JSON secret
const dbCreds = {
  host: 'db.example.com',
  port: 5432,
  username: 'app_user',
  password: 'SuperSecret123!',
  dbname: 'myapp',
};

const { ARN } = await sm.send(new CreateSecretCommand({
  Name: '/myapp/prod/database',
  Description: 'Production database credentials',
  SecretString: JSON.stringify(dbCreds),
  Tags: [
    { Key: 'environment', Value: 'prod' },
    { Key: 'service', Value: 'api' },
  ],
}));

console.log('Secret ARN:', ARN);

// Retrieve and parse the secret
const { SecretString } = await sm.send(new GetSecretValueCommand({
  SecretId: '/myapp/prod/database',
}));

const credentials = JSON.parse(SecretString!);
console.log('DB Host:', credentials.host);

// Rotate the password (creates AWSCURRENT, old becomes AWSPREVIOUS)
await sm.send(new PutSecretValueCommand({
  SecretId: '/myapp/prod/database',
  SecretString: JSON.stringify({ ...credentials, password: 'NewPassword456!' }),
}));

// Retrieve the previous version
const { SecretString: prevSecret } = await sm.send(new GetSecretValueCommand({
  SecretId: '/myapp/prod/database',
  VersionStage: 'AWSPREVIOUS',
}));
console.log('Previous password:', JSON.parse(prevSecret!).password);

// Describe (metadata only, no value)
const description = await sm.send(new DescribeSecretCommand({
  SecretId: '/myapp/prod/database',
}));
console.log('Versions:', description.VersionIdsToStages);

// Delete with 7-day recovery window
await sm.send(new DeleteSecretCommand({
  SecretId: '/myapp/prod/database',
  RecoveryWindowInDays: 7,
}));
```

## Behavior Notes

- Version stages `AWSCURRENT` and `AWSPREVIOUS` are tracked automatically when `PutSecretValue` is called.
- Deleted secrets with a recovery window remain accessible for restoration via their ARN but return `InvalidRequestException` on `GetSecretValue`.
- `ForceDeleteWithoutRecovery: true` immediately and permanently removes the secret.
- Automatic rotation (`RotateSecret`) is not implemented — rotation is a stub only.
- `SecretBinary` values are stored as base64-encoded strings and returned as base64 in `GetSecretValue`.
- Secret ARNs include a 6-character random suffix: `arn:aws:secretsmanager:us-east-1:000000000000:secret:{name}-AbCdEf`.
- State is in-memory only and lost on restart (no persistence even though real Secrets Manager persists).
