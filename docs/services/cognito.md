# Cognito

AWSim emulates two Cognito services: **User Pools** (`cognito-idp`) and **Identity Pools** (`cognito-identity`).

---

## User Pools (cognito-idp)

**Protocol:** AwsJson1_1 (`X-Amz-Target: AWSCognitoIdentityProviderService.*`)
**Signing name:** `cognito-idp`
**Target Prefix:** `AWSCognitoIdentityProviderService`
**Persistent:** Yes

## Quick Start (User Pools)

Create a pool, add a client, create a user, and sign in:

```bash
# Create a user pool
POOL_ID=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSCognitoIdentityProviderService.CreateUserPool" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/cognito-idp/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"PoolName":"my-pool"}' \
  | jq -r '.UserPool.Id')

# Create a client
CLIENT_ID=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSCognitoIdentityProviderService.CreateUserPoolClient" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/cognito-idp/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"UserPoolId\":\"$POOL_ID\",\"ClientName\":\"my-app\",\"ExplicitAuthFlows\":[\"ALLOW_USER_PASSWORD_AUTH\",\"ALLOW_REFRESH_TOKEN_AUTH\"]}" \
  | jq -r '.UserPoolClient.ClientId')

# Create a user and set password
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSCognitoIdentityProviderService.AdminCreateUser" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/cognito-idp/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"UserPoolId\":\"$POOL_ID\",\"Username\":\"alice@example.com\",\"TemporaryPassword\":\"Temp@123!\"}"

curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSCognitoIdentityProviderService.AdminSetUserPassword" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/cognito-idp/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"UserPoolId\":\"$POOL_ID\",\"Username\":\"alice@example.com\",\"Password\":\"MyPassword123!\",\"Permanent\":true}"

# Sign in
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSCognitoIdentityProviderService.InitiateAuth" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/cognito-idp/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"AuthFlow\":\"USER_PASSWORD_AUTH\",\"ClientId\":\"$CLIENT_ID\",\"AuthParameters\":{\"USERNAME\":\"alice@example.com\",\"PASSWORD\":\"MyPassword123!\"}}"
```

### User Pool Management

| Operation | Description |
|-----------|-------------|
| `CreateUserPool` | Create a user pool with schema, password policy, MFA settings |
| `DeleteUserPool` | Delete a user pool |
| `DescribeUserPool` | Get user pool configuration |
| `ListUserPools` | List all user pools |
| `UpdateUserPool` | Update user pool configuration |
| `AddCustomAttributes` | Add custom attributes to the schema |

### User Pool Clients

| Operation | Description |
|-----------|-------------|
| `CreateUserPoolClient` | Create an app client with explicit auth flows |
| `DescribeUserPoolClient` | Get client configuration and client secret |
| `UpdateUserPoolClient` | Update client configuration |
| `DeleteUserPoolClient` | Delete a client |
| `ListUserPoolClients` | List all clients |

### User Management

| Operation | Description |
|-----------|-------------|
| `SignUp` | Self-registration with username and password |
| `ConfirmSignUp` | Confirm registration with code (use `123456` in AWSim) |
| `AdminConfirmSignUp` | Admin-confirm a user without a code |
| `AdminCreateUser` | Create a user as admin with temporary password |
| `AdminDeleteUser` | Delete a user |
| `AdminGetUser` | Get user details and attributes |
| `AdminSetUserPassword` | Set user password (use `Permanent: true` to skip force-change) |
| `AdminEnableUser` | Enable a disabled user |
| `AdminDisableUser` | Disable a user |
| `AdminResetUserPassword` | Force password reset on next login |
| `AdminUpdateUserAttributes` | Update user attributes as admin |
| `AdminDeleteUserAttributes` | Delete user attributes as admin |
| `AdminUserGlobalSignOut` | Sign out all user sessions |
| `ListUsers` | List users with optional filter expression |
| `GetUser` | Get current user's attributes (requires access token) |
| `UpdateUserAttributes` | Update current user's attributes |
| `DeleteUserAttributes` | Delete current user's attributes |
| `DeleteUser` | Delete the current user |
| `VerifyUserAttribute` | Verify an attribute (e.g. email) |
| `GetUserAttributeVerificationCode` | Send attribute verification code |
| `ResendConfirmationCode` | Resend confirmation code |
| `RevokeToken` | Revoke a refresh token |

### Authentication

| Operation | Description |
|-----------|-------------|
| `InitiateAuth` | Start auth flow: `USER_PASSWORD_AUTH`, `REFRESH_TOKEN_AUTH`, `USER_SRP_AUTH` |
| `AdminInitiateAuth` | Admin-initiated auth flow (server-side) |
| `RespondToAuthChallenge` | Respond to a challenge: `NEW_PASSWORD_REQUIRED`, `SOFTWARE_TOKEN_MFA` |
| `AdminRespondToAuthChallenge` | Admin respond to challenge |
| `ForgotPassword` | Initiate forgot password flow (code is always `123456`) |
| `ConfirmForgotPassword` | Confirm new password with code |
| `ChangePassword` | Change password (requires access token) |
| `GlobalSignOut` | Sign out all sessions for current user |

### Groups

| Operation | Description |
|-----------|-------------|
| `CreateGroup` | Create a group with optional IAM role |
| `GetGroup` | Get group details |
| `UpdateGroup` | Update group description or role |
| `DeleteGroup` | Delete group |
| `ListGroups` | List all groups |
| `AdminAddUserToGroup` | Add user to group |
| `AdminRemoveUserFromGroup` | Remove user from group |
| `AdminListGroupsForUser` | List groups for a user |
| `ListUsersInGroup` | List users in a group |

### MFA

| Operation | Description |
|-----------|-------------|
| `SetUserPoolMfaConfig` | Configure MFA for the pool (TOTP, SMS, optional/required) |
| `GetUserPoolMfaConfig` | Get MFA configuration |
| `AssociateSoftwareToken` | Begin TOTP setup — returns a secret key |
| `VerifySoftwareToken` | Verify TOTP setup with a valid code |
| `SetUserMFAPreference` | Set user's preferred MFA method |
| `AdminSetUserMFAPreference` | Admin set user's MFA preference |

### Resource Servers and Identity Providers

| Operation | Description |
|-----------|-------------|
| `CreateResourceServer` | Create an OAuth resource server with custom scopes |
| `DescribeResourceServer` | Get resource server |
| `UpdateResourceServer` | Update resource server |

---

## Identity Pools (cognito-identity)

**Protocol:** AwsJson1_1 (`X-Amz-Target: AWSCognitoIdentityService.*`)
**Signing name:** `cognito-identity`
**Target Prefix:** `AWSCognitoIdentityService`
**Persistent:** Yes

Identity Pools issue temporary AWS credentials via STS-style credential vending based on IAM role mappings.

## Quick Start (Identity Pools)

```bash
# Create an identity pool
POOL_ID=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSCognitoIdentityService.CreateIdentityPool" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/cognito-identity/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"IdentityPoolName":"my-identity-pool","AllowUnauthenticatedIdentities":true}' \
  | jq -r '.IdentityPoolId')

# Get credentials for an identity
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSCognitoIdentityService.GetCredentialsForIdentity" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/cognito-identity/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"IdentityId\":\"us-east-1:some-identity-id\"}"
```

### Operations

| Operation | Description |
|-----------|-------------|
| `CreateIdentityPool` | Create an identity pool with authentication providers |
| `DeleteIdentityPool` | Delete an identity pool |
| `DescribeIdentityPool` | Get pool configuration and role mappings |
| `ListIdentityPools` | List all identity pools |
| `UpdateIdentityPool` | Update pool configuration (role mappings, providers) |
| `GetId` | Get or create an identity ID for a user |
| `GetCredentialsForIdentity` | Get temporary AWS credentials for an identity |

---

## OAuth / OIDC

Cognito User Pools expose full OAuth 2.0 / OIDC endpoints. See [Cognito OAuth/OIDC](/guide/cognito-oauth) for the hosted login page, token endpoint, JWKS, and NextAuth.js integration.

## SDK Example (User Pools)

```typescript
import {
  CognitoIdentityProviderClient,
  CreateUserPoolCommand,
  CreateUserPoolClientCommand,
  AdminCreateUserCommand,
  AdminSetUserPasswordCommand,
  InitiateAuthCommand,
} from '@aws-sdk/client-cognito-identity-provider';

const cognito = new CognitoIdentityProviderClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create user pool
const { UserPool } = await cognito.send(new CreateUserPoolCommand({
  PoolName: 'my-pool',
  Policies: {
    PasswordPolicy: {
      MinimumLength: 8,
      RequireUppercase: true,
      RequireLowercase: true,
      RequireNumbers: true,
    },
  },
}));
const userPoolId = UserPool!.Id!;

// Create client
const { UserPoolClient } = await cognito.send(new CreateUserPoolClientCommand({
  UserPoolId: userPoolId,
  ClientName: 'my-app',
  ExplicitAuthFlows: ['ALLOW_USER_PASSWORD_AUTH', 'ALLOW_REFRESH_TOKEN_AUTH'],
}));
const clientId = UserPoolClient!.ClientId!;

// Create user with permanent password
await cognito.send(new AdminCreateUserCommand({
  UserPoolId: userPoolId,
  Username: 'alice@example.com',
  TemporaryPassword: 'Temp@123!',
}));
await cognito.send(new AdminSetUserPasswordCommand({
  UserPoolId: userPoolId,
  Username: 'alice@example.com',
  Password: 'MyPassword123!',
  Permanent: true,
}));

// Sign in
const { AuthenticationResult } = await cognito.send(new InitiateAuthCommand({
  AuthFlow: 'USER_PASSWORD_AUTH',
  ClientId: clientId,
  AuthParameters: {
    USERNAME: 'alice@example.com',
    PASSWORD: 'MyPassword123!',
  },
}));

console.log('Access Token:', AuthenticationResult?.AccessToken);
console.log('Refresh Token:', AuthenticationResult?.RefreshToken);
```

## Behavior Notes

- Email verification and confirmation codes are always `123456` — no real email is sent.
- SMS-based MFA is accepted but no SMS is delivered — use TOTP or skip verification in tests.
- Tokens are real JWTs signed with a locally generated RSA key; they can be verified against the JWKS endpoint at `http://localhost:4566/{userPoolId}/.well-known/jwks.json`.
- Advanced security features (adaptive authentication, risk scoring) are stubs.
- Identity pool credentials are valid for testing SDK calls — they use the AWSim account.
