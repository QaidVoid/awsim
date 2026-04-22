# Cognito

AWSim emulates two Cognito services: **User Pools** (`cognito-idp`) and **Identity Pools** (`cognito-identity`).

---

## User Pools (cognito-idp)

**Protocol:** JSON (`X-Amz-Target: AmazonCognitoIdentityProvider.*`)  
**Signing name:** `cognito-idp`  
**Persistent:** Yes

### User Pool Management

| Operation | Description |
|-----------|-------------|
| `CreateUserPool` | Create a user pool |
| `DeleteUserPool` | Delete a user pool |
| `DescribeUserPool` | Get user pool configuration |
| `ListUserPools` | List all user pools |
| `UpdateUserPool` | Update user pool configuration |
| `AddCustomAttributes` | Add custom attributes to the schema |

### User Pool Clients

| Operation | Description |
|-----------|-------------|
| `CreateUserPoolClient` | Create an app client |
| `DescribeUserPoolClient` | Get client configuration |
| `UpdateUserPoolClient` | Update client configuration |
| `DeleteUserPoolClient` | Delete a client |
| `ListUserPoolClients` | List all clients |

### User Management

| Operation | Description |
|-----------|-------------|
| `SignUp` | Self-registration |
| `ConfirmSignUp` | Confirm registration with code |
| `AdminConfirmSignUp` | Admin-confirm a user |
| `AdminCreateUser` | Create a user as admin |
| `AdminDeleteUser` | Delete a user |
| `AdminGetUser` | Get user details |
| `AdminSetUserPassword` | Set user password |
| `AdminEnableUser` | Enable a disabled user |
| `AdminDisableUser` | Disable a user |
| `AdminResetUserPassword` | Force password reset |
| `AdminUpdateUserAttributes` | Update user attributes as admin |
| `AdminDeleteUserAttributes` | Delete user attributes as admin |
| `AdminUserGlobalSignOut` | Sign out all user sessions |
| `ListUsers` | List users with optional filter |
| `GetUser` | Get current user's attributes (access token) |
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
| `InitiateAuth` | Start auth flow (USER_PASSWORD_AUTH, etc.) |
| `AdminInitiateAuth` | Admin-initiated auth flow |
| `RespondToAuthChallenge` | Respond to a challenge (e.g. NEW_PASSWORD_REQUIRED) |
| `AdminRespondToAuthChallenge` | Admin respond to challenge |
| `ForgotPassword` | Initiate forgot password flow |
| `ConfirmForgotPassword` | Confirm new password with code |
| `ChangePassword` | Change password (authenticated) |
| `GlobalSignOut` | Sign out all sessions |

### Groups

| Operation | Description |
|-----------|-------------|
| `CreateGroup` | Create a group |
| `GetGroup` | Get group details |
| `UpdateGroup` | Update group |
| `DeleteGroup` | Delete group |
| `ListGroups` | List all groups |
| `AdminAddUserToGroup` | Add user to group |
| `AdminRemoveUserFromGroup` | Remove user from group |
| `AdminListGroupsForUser` | List groups for a user |
| `ListUsersInGroup` | List users in a group |

### MFA

| Operation | Description |
|-----------|-------------|
| `SetUserPoolMfaConfig` | Configure MFA for the pool |
| `GetUserPoolMfaConfig` | Get MFA configuration |
| `AssociateSoftwareToken` | Begin TOTP setup |
| `VerifySoftwareToken` | Verify TOTP setup |
| `SetUserMFAPreference` | Set user's MFA preference |
| `AdminSetUserMFAPreference` | Admin set user's MFA preference |

### Resource Servers and Identity Providers

| Operation | Description |
|-----------|-------------|
| `CreateResourceServer` | Create an OAuth resource server |
| `DescribeResourceServer` | Get resource server |
| `UpdateResourceServer` | Update resource server |

### Admin Auth Events

| Operation | Description |
|-----------|-------------|
| `AdminListUserAuthEvents` | List auth events for a user |

---

## Identity Pools (cognito-identity)

**Protocol:** JSON (`X-Amz-Target: AmazonCognitoIdentity.*`)  
**Signing name:** `cognito-identity`  
**Persistent:** Yes

Identity Pools issue temporary AWS credentials via STS-style credential vending based on IAM role mappings.

### Operations

| Operation | Description |
|-----------|-------------|
| `CreateIdentityPool` | Create an identity pool |
| `DeleteIdentityPool` | Delete an identity pool |
| `DescribeIdentityPool` | Get pool configuration |
| `ListIdentityPools` | List all identity pools |
| `UpdateIdentityPool` | Update pool configuration (role mappings) |
| `GetId` | Get or create an identity ID |
| `GetCredentialsForIdentity` | Get temporary credentials for an identity |

---

## OAuth / OIDC

Cognito User Pools expose full OAuth 2.0 / OIDC endpoints. See [Cognito OAuth/OIDC](/guide/cognito-oauth) for the hosted login page, token endpoint, JWKS, and NextAuth.js integration.

## SDK Example (User Pools)

```typescript
import {
  CognitoIdentityProviderClient,
  CreateUserPoolCommand,
  AdminCreateUserCommand,
  InitiateAuthCommand,
} from "@aws-sdk/client-cognito-identity-provider";

const cognito = new CognitoIdentityProviderClient({
  region: "us-east-1",
  endpoint: "http://localhost:4566",
  credentials: { accessKeyId: "test", secretAccessKey: "test" },
});

// Create user pool
const { UserPool } = await cognito.send(new CreateUserPoolCommand({
  PoolName: "my-pool",
}));

// Create user
await cognito.send(new AdminCreateUserCommand({
  UserPoolId: UserPool!.Id!,
  Username: "alice@example.com",
  TemporaryPassword: "Temp@123!",
}));

// Sign in
const { AuthenticationResult } = await cognito.send(new InitiateAuthCommand({
  AuthFlow: "USER_PASSWORD_AUTH",
  ClientId: "<client_id>",
  AuthParameters: {
    USERNAME: "alice@example.com",
    PASSWORD: "MyPassword123!",
  },
}));

console.log(AuthenticationResult?.AccessToken);
```

## Known Limitations

- Email verification and confirmation codes are always `123456` (no real email is sent).
- SMS-based MFA is accepted but no SMS is delivered — use TOTP or skip verification.
- Advanced security features (adaptive authentication, risk scoring) are stubs.
