# Cognito OAuth / OIDC

AWSim emulates Cognito's hosted UI and OAuth 2.0 / OIDC endpoints. You can use it as a drop-in OIDC provider for local development.

## Endpoints

All OAuth endpoints are scoped to the user pool ID. The base URL is `http://localhost:4566`.

### OIDC Discovery

```
GET /cognito/{pool_id}/.well-known/openid-configuration
```

Returns the standard OIDC discovery document with all endpoint URLs, supported scopes, and JWKS URI.

### JWKS

```
GET /.well-known/jwks.json
```

Returns the JSON Web Key Set used to verify JWT tokens issued by AWSim.

### Authorization Endpoint

```
GET /oauth2/authorize
```

Renders an HTML login form. Supports:

- `response_type=code`
- `client_id`
- `redirect_uri`
- `scope`
- `state`
- `code_challenge` / `code_challenge_method` (PKCE, S256)

After the user submits the form, AWSim redirects to `redirect_uri` with the authorization code.

### Token Endpoint

```
POST /oauth2/token
```

Supports three grant types:

**Authorization code exchange:**

```bash
curl -X POST http://localhost:4566/oauth2/token \
  -d "grant_type=authorization_code" \
  -d "code=<code>" \
  -d "redirect_uri=http://localhost:3000/callback" \
  -d "client_id=<client_id>" \
  -d "code_verifier=<verifier>"
```

**Client credentials:**

```bash
curl -X POST http://localhost:4566/oauth2/token \
  -u "<client_id>:<client_secret>" \
  -d "grant_type=client_credentials" \
  -d "scope=<scope>"
```

**Refresh token:**

```bash
curl -X POST http://localhost:4566/oauth2/token \
  -d "grant_type=refresh_token" \
  -d "refresh_token=<token>" \
  -d "client_id=<client_id>"
```

All responses include `access_token`, `id_token`, `refresh_token`, and `expires_in`.

### UserInfo Endpoint

```
GET /oauth2/userInfo
Authorization: Bearer <access_token>
```

Returns the user's profile attributes (sub, email, etc.) as standard OIDC claims.

### Revoke Endpoint

```
POST /oauth2/revoke
```

Revokes an access or refresh token.

```bash
curl -X POST http://localhost:4566/oauth2/revoke \
  -d "token=<token>" \
  -d "client_id=<client_id>"
```

## JWT Claims

Tokens issued by AWSim include standard Cognito claims:

- `sub` — user's UUID
- `email`
- `cognito:username`
- `cognito:groups` — list of Cognito groups the user belongs to
- `cognito:roles` — IAM role ARNs mapped from group membership
- `cognito:preferred_role` — highest-precedence role ARN

## NextAuth.js Integration

```typescript
// auth.ts
import NextAuth from "next-auth";
import Cognito from "next-auth/providers/cognito";

export const { handlers, auth } = NextAuth({
  providers: [
    Cognito({
      clientId: process.env.COGNITO_CLIENT_ID!,
      clientSecret: process.env.COGNITO_CLIENT_SECRET!,
      issuer: `http://localhost:4566/cognito/${process.env.COGNITO_POOL_ID}`,
    }),
  ],
});
```

Set `NEXTAUTH_URL=http://localhost:3000` and `NEXTAUTH_SECRET=any-value` in your `.env.local`.

## Notes

- The hosted UI login form is a simple HTML page — it is not styled like the real Cognito hosted UI.
- PKCE (S256) is supported.
- `id_token` is a signed JWT. Use the JWKS endpoint to verify it in your app.
