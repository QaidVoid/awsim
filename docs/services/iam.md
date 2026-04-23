# IAM & STS

AWS Identity and Access Management for managing users, groups, roles, and policies. STS provides temporary credentials.

---

## IAM

**Protocol:** `AwsQuery` (`Action=` parameter in form body)
**Signing name:** `iam`
**Persistent:** Yes

IAM is a **global** service — resources are not region-specific.

## Quick Start (IAM)

Create a role, attach a managed policy, and create access keys:

```bash
# Create a role with a Lambda trust policy
aws --endpoint-url http://localhost:4566 iam create-role \
  --role-name my-lambda-role \
  --assume-role-policy-document '{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"lambda.amazonaws.com"},"Action":"sts:AssumeRole"}]}'

# Attach AWSLambdaBasicExecutionRole
aws --endpoint-url http://localhost:4566 iam attach-role-policy \
  --role-name my-lambda-role \
  --policy-arn arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole

# Create a user and access key
aws --endpoint-url http://localhost:4566 iam create-user --user-name bob
aws --endpoint-url http://localhost:4566 iam create-access-key --user-name bob
```

## Quick Start (STS)

```bash
# Get caller identity
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/sts/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=GetCallerIdentity' \
  --data-urlencode 'Version=2011-06-15'
```

## IAM Operations

### Users

| Operation | Description |
|-----------|-------------|
| `CreateUser` | Create an IAM user. Input: `UserName`, optional `Path`, `Tags`. Returns: `User` with `UserId`, `Arn`, `CreateDate` |
| `GetUser` | Get user details. Input: optional `UserName` (defaults to caller) |
| `UpdateUser` | Update user name or path. Input: `UserName`, optional `NewUserName`, `NewPath` |
| `DeleteUser` | Delete a user. Must detach all policies first |
| `ListUsers` | List all users. Supports `PathPrefix`, `MaxItems`, `Marker` pagination |
| `CreateAccessKey` | Generate access key ID + secret for a user |
| `DeleteAccessKey` | Delete an access key by ID |
| `ListAccessKeys` | List access keys for a user (secret is not returned after creation) |
| `TagUser` / `UntagUser` / `ListUserTags` | Tag management |

### Groups

| Operation | Description |
|-----------|-------------|
| `CreateGroup` | Create a group. Input: `GroupName`, optional `Path` |
| `GetGroup` | Get group with member list |
| `DeleteGroup` | Delete a group |
| `ListGroups` | List all groups |
| `AddUserToGroup` | Add a user to a group. Input: `GroupName`, `UserName` |
| `RemoveUserFromGroup` | Remove a user from a group |

### Roles

| Operation | Description |
|-----------|-------------|
| `CreateRole` | Create a role with a trust policy. Input: `RoleName`, `AssumeRolePolicyDocument` (JSON), optional `Description`, `MaxSessionDuration`, `Path`, `Tags`. Returns: `Role` with `RoleId`, `Arn` |
| `GetRole` | Get role details including trust policy and attached policies |
| `UpdateRole` | Update role description or max session duration |
| `DeleteRole` | Delete a role. Must detach all policies first |
| `ListRoles` | List all roles |
| `UpdateAssumeRolePolicy` | Update the role's trust policy document |
| `TagRole` / `UntagRole` / `ListRoleTags` | Tag management |

### Managed Policies

| Operation | Description |
|-----------|-------------|
| `CreatePolicy` | Create a managed policy. Input: `PolicyName`, `PolicyDocument` (JSON), optional `Description`, `Path`. Returns: `Policy` with `PolicyArn` |
| `GetPolicy` | Get policy metadata (not document — use `GetPolicyVersion`) |
| `DeletePolicy` | Delete a policy |
| `ListPolicies` | List managed policies. Use `Scope=Local` for custom policies |
| `CreatePolicyVersion` | Create a new policy version. Input: `PolicyArn`, `PolicyDocument`, optional `SetAsDefault` |
| `GetPolicyVersion` | Get a specific version's document. Input: `PolicyArn`, `VersionId` |
| `DeletePolicyVersion` | Delete a non-default policy version |
| `ListPolicyVersions` | List all versions of a policy |
| `SetDefaultPolicyVersion` | Set the default active version |

### Attaching Policies

| Operation | Description |
|-----------|-------------|
| `AttachUserPolicy` | Attach a managed policy to a user |
| `DetachUserPolicy` | Detach a managed policy from a user |
| `AttachRolePolicy` | Attach a managed policy to a role |
| `DetachRolePolicy` | Detach a managed policy from a role |
| `AttachGroupPolicy` | Attach a managed policy to a group |
| `DetachGroupPolicy` | Detach a managed policy from a group |
| `ListAttachedUserPolicies` | List managed policies attached to a user |
| `ListAttachedRolePolicies` | List managed policies attached to a role |
| `ListAttachedGroupPolicies` | List managed policies attached to a group |

### Inline Policies

| Operation | Description |
|-----------|-------------|
| `PutUserPolicy` | Create/replace an inline policy on a user |
| `GetUserPolicy` | Get an inline policy document |
| `DeleteUserPolicy` | Delete an inline policy |
| `ListUserPolicies` | List inline policy names for a user |
| `PutRolePolicy` | Create/replace an inline policy on a role |
| `GetRolePolicy` | Get an inline policy document on a role |
| `DeleteRolePolicy` | Delete an inline policy on a role |
| `ListRolePolicies` | List inline policy names for a role |
| `PutGroupPolicy` | Create/replace an inline policy on a group |

### Instance Profiles

| Operation | Description |
|-----------|-------------|
| `CreateInstanceProfile` | Create an instance profile |
| `DeleteInstanceProfile` | Delete an instance profile |
| `GetInstanceProfile` | Get instance profile details |
| `AddRoleToInstanceProfile` | Attach a role to an instance profile |
| `RemoveRoleFromInstanceProfile` | Detach a role |
| `TagInstanceProfile` / `UntagInstanceProfile` / `ListInstanceProfileTags` | Tag management |

### Account

| Operation | Description |
|-----------|-------------|
| `CreateAccountAlias` | Set account alias |
| `DeleteAccountAlias` | Remove account alias |
| `ListAccountAliases` | List aliases |
| `GetAccountPasswordPolicy` | Get password policy |
| `UpdateAccountPasswordPolicy` | Update password policy |
| `GetAccountSummary` | Account-level summary (user/role/policy counts) |
| `GetAccountAuthorizationDetails` | Full account auth details for all users, groups, and roles |

### OIDC Providers

| Operation | Description |
|-----------|-------------|
| `CreateOpenIDConnectProvider` | Register an OIDC IdP with thumbprint |
| `GetOpenIDConnectProvider` | Get OIDC provider |
| `ListOpenIDConnectProviders` | List OIDC providers |
| `DeleteOpenIDConnectProvider` | Delete an OIDC provider |
| `AddClientIDToOpenIDConnectProvider` | Add a client ID |
| `RemoveClientIDFromOpenIDConnectProvider` | Remove a client ID |
| `UpdateOpenIDConnectProviderThumbprint` | Update thumbprint |

### SAML Providers

| Operation | Description |
|-----------|-------------|
| `CreateSAMLProvider` | Create a SAML IdP. Input: `Name`, `SAMLMetadataDocument` |
| `GetSAMLProvider` | Get SAML provider |
| `ListSAMLProviders` | List SAML providers |
| `DeleteSAMLProvider` | Delete a SAML provider |
| `UpdateSAMLProvider` | Update SAML metadata |

### MFA Devices

| Operation | Description |
|-----------|-------------|
| `CreateVirtualMFADevice` | Create a virtual MFA device (returns QR code seed) |
| `ListVirtualMFADevices` | List virtual MFA devices |
| `DeleteVirtualMFADevice` | Delete a virtual MFA device |
| `EnableMFADevice` | Enable MFA for a user |
| `DeactivateMFADevice` | Disable MFA for a user |
| `ListMFADevices` | List MFA devices for a user |

---

## STS

**Protocol:** `AwsQuery`
**Signing name:** `sts`
**Persistent:** No

| Operation | Description |
|-----------|-------------|
| `GetCallerIdentity` | Return the `Account`, `UserId`, and `Arn` of the caller |
| `AssumeRole` | Get temporary credentials for a role. Input: `RoleArn`, `RoleSessionName`, optional `DurationSeconds` (900–43200), `ExternalId` |
| `GetSessionToken` | Get temporary credentials for a user. Input: optional `DurationSeconds`, `SerialNumber`, `TokenCode` |
| `AssumeRoleWithWebIdentity` | Exchange a web identity token (OIDC) for credentials. Input: `RoleArn`, `RoleSessionName`, `WebIdentityToken` |
| `AssumeRoleWithSAML` | Exchange a SAML assertion for credentials |

## SDK Example

```typescript
import { IAMClient, CreateUserCommand, CreateAccessKeyCommand, CreateRoleCommand, AttachRolePolicyCommand } from '@aws-sdk/client-iam';
import { STSClient, GetCallerIdentityCommand, AssumeRoleCommand } from '@aws-sdk/client-sts';

const iam = new IAMClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create user
await iam.send(new CreateUserCommand({ UserName: 'alice' }));

// Create access key
const { AccessKey } = await iam.send(new CreateAccessKeyCommand({ UserName: 'alice' }));
console.log('Key ID:', AccessKey?.AccessKeyId);
console.log('Secret:', AccessKey?.SecretAccessKey);

// Create a role
const { Role } = await iam.send(new CreateRoleCommand({
  RoleName: 'lambda-execution-role',
  AssumeRolePolicyDocument: JSON.stringify({
    Version: '2012-10-17',
    Statement: [{
      Effect: 'Allow',
      Principal: { Service: 'lambda.amazonaws.com' },
      Action: 'sts:AssumeRole',
    }],
  }),
  Description: 'Lambda execution role',
}));

// Attach managed policy
await iam.send(new AttachRolePolicyCommand({
  RoleName: 'lambda-execution-role',
  PolicyArn: 'arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole',
}));

// STS: who am I?
const sts = new STSClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

const identity = await sts.send(new GetCallerIdentityCommand({}));
console.log('Account:', identity.Account);  // 000000000000
console.log('User:', identity.UserId);
console.log('ARN:', identity.Arn);

// Assume a role
const { Credentials } = await sts.send(new AssumeRoleCommand({
  RoleArn: Role!.Arn!,
  RoleSessionName: 'my-session',
  DurationSeconds: 3600,
}));
console.log('Temp Key ID:', Credentials?.AccessKeyId);
```

## CLI Example

```bash
# Create role
aws --endpoint-url http://localhost:4566 iam create-role \
  --role-name my-role \
  --assume-role-policy-document '{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"lambda.amazonaws.com"},"Action":"sts:AssumeRole"}]}'

# Attach policy
aws --endpoint-url http://localhost:4566 iam attach-role-policy \
  --role-name my-role \
  --policy-arn arn:aws:iam::aws:policy/AmazonS3ReadOnlyAccess

# Get caller identity
aws --endpoint-url http://localhost:4566 sts get-caller-identity

# Assume role
aws --endpoint-url http://localhost:4566 sts assume-role \
  --role-arn arn:aws:iam::000000000000:role/my-role \
  --role-session-name test-session
```

## Behavior Notes

- IAM policy evaluation is **not enforced** — all operations succeed regardless of attached policies. Use AWSim for structural testing of your IaC, not authorization logic.
- Credentials issued by `AssumeRole` and `GetSessionToken` are accepted by all AWSim services but are not scoped to any role's permissions.
- IAM is persistent: users, groups, roles, and policies survive AWSim restarts.
- Permission boundaries, service control policies (SCPs), and resource-based policies are stored but not evaluated.
- `GetCallerIdentity` always returns account ID `000000000000` regardless of credentials used.
