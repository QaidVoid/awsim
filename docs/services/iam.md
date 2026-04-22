# IAM & STS

## IAM

**Protocol:** Query (`Action=` parameter)  
**Signing name:** `iam`  
**Persistent:** Yes

### Users

| Operation | Description |
|-----------|-------------|
| `CreateUser` | Create an IAM user |
| `GetUser` | Get user details |
| `UpdateUser` | Update user name or path |
| `DeleteUser` | Delete a user |
| `ListUsers` | List all users |
| `CreateAccessKey` | Generate access key for a user |
| `DeleteAccessKey` | Delete an access key |
| `ListAccessKeys` | List access keys for a user |
| `TagUser` | Add tags to a user |
| `UntagUser` | Remove tags |
| `ListUserTags` | List user tags |

### Groups

| Operation | Description |
|-----------|-------------|
| `CreateGroup` | Create a group |
| `GetGroup` | Get group with member list |
| `DeleteGroup` | Delete a group |
| `ListGroups` | List all groups |
| `AddUserToGroup` | Add a user to a group |
| `RemoveUserFromGroup` | Remove a user from a group |

### Roles

| Operation | Description |
|-----------|-------------|
| `CreateRole` | Create a role with a trust policy |
| `GetRole` | Get role details |
| `UpdateRole` | Update role description or max session duration |
| `UpdateRoleDescription` | Update role description |
| `DeleteRole` | Delete a role |
| `ListRoles` | List all roles |
| `UpdateAssumeRolePolicy` | Update the role's trust policy |
| `TagRole` | Add tags to a role |
| `UntagRole` | Remove tags |
| `ListRoleTags` | List role tags |

### Managed Policies

| Operation | Description |
|-----------|-------------|
| `CreatePolicy` | Create a managed policy |
| `GetPolicy` | Get policy metadata |
| `DeletePolicy` | Delete a policy |
| `ListPolicies` | List managed policies |
| `CreatePolicyVersion` | Create a new policy version |
| `GetPolicyVersion` | Get a specific version |
| `DeletePolicyVersion` | Delete a policy version |
| `ListPolicyVersions` | List all versions |
| `SetDefaultPolicyVersion` | Set the default version |

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
| `GetUserPolicy` | Get an inline policy |
| `DeleteUserPolicy` | Delete an inline policy |
| `ListUserPolicies` | List inline policies for a user |
| `PutRolePolicy` | Create/replace an inline policy on a role |
| `GetRolePolicy` | Get an inline policy |
| `DeleteRolePolicy` | Delete an inline policy |
| `ListRolePolicies` | List inline policies for a role |
| `PutGroupPolicy` | Create/replace an inline policy on a group |

### Instance Profiles

| Operation | Description |
|-----------|-------------|
| `CreateInstanceProfile` | Create an instance profile |
| `DeleteInstanceProfile` | Delete an instance profile |
| `GetInstanceProfile` | Get instance profile details |
| `AddRoleToInstanceProfile` | Add a role to an instance profile |
| `RemoveRoleFromInstanceProfile` | Remove a role |
| `TagInstanceProfile` | Tag an instance profile |
| `UntagInstanceProfile` | Untag |
| `ListInstanceProfileTags` | List tags |

### Account

| Operation | Description |
|-----------|-------------|
| `CreateAccountAlias` | Set account alias |
| `DeleteAccountAlias` | Remove account alias |
| `ListAccountAliases` | List aliases |
| `GetAccountPasswordPolicy` | Get password policy |
| `UpdateAccountPasswordPolicy` | Update password policy |
| `DeleteAccountPasswordPolicy` | Delete password policy |
| `GetAccountSummary` | Get account-level summary |
| `GetAccountAuthorizationDetails` | Full account auth details |

### OIDC Providers

| Operation | Description |
|-----------|-------------|
| `CreateOpenIDConnectProvider` | Register an OIDC IdP |
| `GetOpenIDConnectProvider` | Get OIDC provider |
| `ListOpenIDConnectProviders` | List OIDC providers |
| `DeleteOpenIDConnectProvider` | Delete an OIDC provider |
| `AddClientIDToOpenIDConnectProvider` | Add a client ID |
| `RemoveClientIDFromOpenIDConnectProvider` | Remove a client ID |
| `UpdateOpenIDConnectProviderThumbprint` | Update thumbprint |

### SAML Providers

| Operation | Description |
|-----------|-------------|
| `CreateSAMLProvider` | Create a SAML IdP |
| `GetSAMLProvider` | Get SAML provider |
| `ListSAMLProviders` | List SAML providers |
| `DeleteSAMLProvider` | Delete a SAML provider |
| `UpdateSAMLProvider` | Update SAML metadata |

### MFA Devices

| Operation | Description |
|-----------|-------------|
| `CreateVirtualMFADevice` | Create a virtual MFA device |
| `ListVirtualMFADevices` | List virtual MFA devices |
| `DeleteVirtualMFADevice` | Delete a virtual MFA device |
| `EnableMFADevice` | Enable MFA for a user |
| `DeactivateMFADevice` | Disable MFA for a user |
| `ListMFADevices` | List MFA devices for a user |

### SSH Keys and Server Certificates

| Operation | Description |
|-----------|-------------|
| `UploadSSHPublicKey` | Upload an SSH public key |
| `GetSSHPublicKey` | Get an SSH public key |
| `ListSSHPublicKeys` | List SSH public keys |
| `DeleteSSHPublicKey` | Delete an SSH public key |
| `UpdateSSHPublicKey` | Update status of an SSH public key |
| `UploadServerCertificate` | Upload a server certificate |
| `GetServerCertificate` | Get a server certificate |
| `ListServerCertificates` | List server certificates |
| `DeleteServerCertificate` | Delete a server certificate |
| `TagServerCertificate` | Tag a server certificate |
| `UntagServerCertificate` | Untag |
| `ListServerCertificateTags` | List tags |

### Service-Linked Roles

| Operation | Description |
|-----------|-------------|
| `CreateServiceLinkedRole` | Create a service-linked role |
| `DeleteServiceLinkedRole` | Delete a service-linked role |
| `GetServiceLinkedRoleDeletionStatus` | Check deletion status |

### Credential Report

| Operation | Description |
|-----------|-------------|
| `GenerateCredentialReport` | Generate the credential report |
| `GetCredentialReport` | Download the credential report |
| `GenerateServiceLastAccessedDetails` | Generate service access report |
| `GetServiceLastAccessedDetails` | Get service access report |

---

## STS

**Protocol:** Query (`Action=` parameter)  
**Signing name:** `sts`  
**Persistent:** No

| Operation | Description |
|-----------|-------------|
| `GetCallerIdentity` | Return the account/user/ARN of the caller |
| `AssumeRole` | Get temporary credentials for a role |
| `GetSessionToken` | Get temporary credentials for a user |
| `AssumeRoleWithWebIdentity` | Exchange a web identity token for credentials |
| `AssumeRoleWithSAML` | Exchange a SAML assertion for credentials |

## SDK Example

```typescript
import { IAMClient, CreateUserCommand, CreateAccessKeyCommand } from "@aws-sdk/client-iam";
import { STSClient, GetCallerIdentityCommand } from "@aws-sdk/client-sts";

const iam = new IAMClient({
  region: "us-east-1",
  endpoint: "http://localhost:4566",
  credentials: { accessKeyId: "test", secretAccessKey: "test" },
});

// Create user
await iam.send(new CreateUserCommand({ UserName: "alice" }));

// Create access key
const { AccessKey } = await iam.send(new CreateAccessKeyCommand({ UserName: "alice" }));
console.log(AccessKey?.AccessKeyId, AccessKey?.SecretAccessKey);

// STS - who am I?
const sts = new STSClient({
  region: "us-east-1",
  endpoint: "http://localhost:4566",
  credentials: { accessKeyId: "test", secretAccessKey: "test" },
});

const identity = await sts.send(new GetCallerIdentityCommand({}));
console.log(identity.Account, identity.UserId, identity.Arn);
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
```

## Known Limitations

- IAM policy evaluation is **not enforced** — all operations succeed regardless of attached policies.
- Credentials issued by `AssumeRole` are accepted by AWSim but not actually scoped to the role's permissions.
- Permission boundaries are stored but not enforced.
