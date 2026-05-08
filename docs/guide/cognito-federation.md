# Cognito Federation (OIDC IdP)

AWSim ships a built-in mock OIDC identity provider so you can test
the federated sign-in code path against your Cognito user pool
without any external network calls. This is the offline equivalent
of pointing your pool at Google / Microsoft Entra / GitHub.

## TL;DR

```bash
# 1. Spin up an awsim mock OIDC provider.
curl -s -X POST http://localhost:4566/_awsim/idp \
  -H 'content-type: application/json' \
  -d '{"provider_id":"mockidp"}'
# {"provider_id":"mockidp","client_id":"awsim-idp-mockidp",
#  "client_secret":"...","discovery_url":"...","..."}

# 2. Register a matching IdentityProvider on your pool.
aws --endpoint http://localhost:4566 cognito-idp create-identity-provider \
  --user-pool-id us-east-1_abc123 \
  --provider-name MockIdP \
  --provider-type OIDC \
  --provider-details \
    oidc_issuer=http://localhost:4566/_awsim/idp/mockidp,\
client_id=awsim-idp-mockidp,\
client_secret=...,\
authorize_scopes='openid email profile' \
  --attribute-mapping email=email,name=name

# 3. Send the user to the hosted UI with ?identity_provider=MockIdP.
open "http://localhost:4566/cognito/us-east-1_abc123/oauth2/authorize\
?response_type=code&client_id=<APP_CLIENT_ID>\
&redirect_uri=http://localhost:9000/callback\
&scope=openid+email&state=xyz\
&identity_provider=MockIdP"
```

The user lands on the mock IdP's "sign-in" form (free-form JSON
claim entry), submits, and is bounced back to your app with a
Cognito authorization code that exchanges for normal Cognito
tokens. A federated user named `MockIdP_<sub-from-claims>` is
created in the pool on first sign-in and reused on subsequent ones.

## How it works

```
        +---------+      ?identity_provider=Foo      +---------+
        |   App   | ---------------------------> |  Cognito | (1)
        +---------+                              +-----+----+
                                                       |
                              redirect to IdP authorize|
                                                       v
                                              +-----------------+
                                              | awsim mock IdP  | (2)
                                              | (or any OIDC)   |
                                              +--------+--------+
                                                       |
                                       user submits   |
                                       claims form     |
                                                       v
        +---------+      303: code+state         +-----+----+
        |   App   | <--------------------------- |  Cognito | (3)
        +---------+                              +----------+
              |                                       ^
              |  POST /oauth2/token (code)            |
              +---------------------------------------+ (4)
```

1. The app sends the user to Cognito's hosted UI authorize endpoint
   with `?identity_provider=Foo`.
2. Cognito recognises Foo, parks the original authorize request
   under a federation state token, and redirects the user's browser
   to the IdP's authorize URL with our `/oauth2/idpresponse` URL as
   the `redirect_uri`.
3. The IdP redirects back to `/oauth2/idpresponse?code=&state=`.
   Cognito exchanges that code for an ID token at the IdP's `/token`
   endpoint, verifies the signature against the IdP's JWKS, applies
   the `AttributeMapping`, upserts the federated user, and finally
   mints a Cognito authorization code that it sends to the app via
   the original `redirect_uri`.
4. The app redeems that code at Cognito's `/oauth2/token` exactly
   like a native sign-in. The resulting ID and access tokens are
   signed with the pool's key and carry `cognito:username = Foo_<idp-sub>`.

## The mock IdP endpoints

Hosted under `/_awsim/idp/{provider_id}`:

| Endpoint | Purpose |
|---|---|
| `GET .well-known/openid-configuration` | OIDC discovery document |
| `GET .well-known/jwks.json` | RS256 public key for ID-token verification |
| `GET authorize` | Free-form claim entry form |
| `POST authorize` | Mints a 60s authorization code, redirects with `?code=&state=` |
| `POST token` | Exchanges code for `{access_token, id_token, expires_in}` |
| `GET userinfo` | Returns claims for an `Authorization: Bearer <access_token>` |

And under `/_awsim/idp` (admin / control plane):

| Verb | Endpoint | Purpose |
|---|---|---|
| `POST` | `/_awsim/idp` | Register a provider; returns `{provider_id, client_id, client_secret, discovery_url, ...}` |
| `GET` | `/_awsim/idp` | List registered providers |
| `DELETE` | `/_awsim/idp/{provider_id}` | Remove |

All providers share one process-wide RSA-2048 keypair distinct from
the Cognito pool key, so the IdP looks like a real external trust
root.

## Choosing what claims the user signs in with

The mock IdP's `/authorize` page renders the `default_claims` you
passed at registration as a JSON textarea. Edit it freely before
submitting - whatever you put lands as claims on the ID token + the
`/userinfo` response. AWS-side `AttributeMapping` then translates
those claims to Cognito user attributes.

A common pattern:

```json
{
  "sub": "alice-123",
  "email": "alice@external.test",
  "email_verified": true,
  "name": "Alice External",
  "given_name": "Alice",
  "family_name": "External",
  "groups": ["admins", "engineering"]
}
```

If you want fixed identities for a regression suite, register
multiple providers (`mockidp-alice`, `mockidp-bob`) each with their
own `default_claims`.

## Admin console shortcut

Pool detail -> **Federation** tab has an **Add awsim mock IdP**
button that:

1. POSTs to `/_awsim/idp` with the id and default claims you supply.
2. Creates a matching Cognito-side IdentityProvider (`Type=OIDC`)
   with `oidc_issuer`, `client_id`, `client_secret` already wired up
   and a starter AttributeMapping (`email -> email`, `name -> name`).

After that, your app's authorize URL with `?identity_provider=<name>`
just works.

## Pointing at a different OIDC provider

The federation code path is generic. If you want to test against an
OIDC provider you control (e.g. a Keycloak running locally), skip
the mock IdP and register the IdentityProvider with that provider's
issuer URL directly:

```bash
aws --endpoint http://localhost:4566 cognito-idp create-identity-provider \
  --user-pool-id us-east-1_abc123 \
  --provider-name Keycloak \
  --provider-type OIDC \
  --provider-details \
    oidc_issuer=http://keycloak:8080/realms/dev,\
client_id=cognito-test,\
client_secret=secret,\
authorize_scopes='openid email profile'
```

AWSim will fetch the discovery doc from
`{issuer}/.well-known/openid-configuration`, cache it, and use the
`authorization_endpoint`/`token_endpoint`/`jwks_uri` from there.
You can also pre-supply explicit URLs in `ProviderDetails`
(`authorize_url`, `token_url`, `jwks_uri`, `attributes_url`) to skip
the discovery fetch entirely.

## Federated user records

On first federated sign-in AWSim creates a user with:

- `Username = <ProviderName>_<idp-sub>` (matches Cognito's
  documented federated-user naming)
- `Status = EXTERNAL_PROVIDER`
- No password - any local password-flow attempt against this user
  fails closed.
- `linked_providers` carries `{provider_name, provider_attribute_name=Cognito_Subject, provider_attribute_value=<idp-sub>}`

Subsequent sign-ins refresh the mapped attributes but keep the
existing `sub` and link record.

## Caveats

- **OIDC only in tier 1.** SAML and the social-provider quick paths
  (`Google`, `Facebook`, `LoginWithAmazon`, `SignInWithApple`) parse
  fine into the IdentityProvider model but the federation runtime
  only handles `Type=OIDC` for now.
- **Claim signature is verified**, but `iss` / `aud` / `exp` are
  the only OIDC validations performed. `nonce` round-tripping is
  best-effort.
- **The mock IdP is in-process.** It binds to the same port as the
  rest of awsim, so the discovery URL is always
  `http://<host>:<port>/_awsim/idp/<id>`. Self-call works correctly
  over TLS too (the federation HTTP client accepts the bundled
  self-signed cert).
- **The 50-attribute custom-attribute cap still applies** to the
  Cognito-side schema - declare any custom OIDC claims you want to
  map (`AddCustomAttributes` or via the **Attributes** tab) before
  setting them via `AttributeMapping`.
