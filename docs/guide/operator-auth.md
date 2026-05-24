# Operator authentication

By default AWSim is loginless: anyone with network access to the
gateway port can call every API and visit the admin UI. That's the
right behavior for single-user local development, but not for any
shared deployment.

Setting `AWSIM_REQUIRE_OPERATOR_AUTH=true` turns AWSim into a
multi-tenant service that gates the admin UI and admin HTTP
endpoints behind login. The simulated AWS gateway can be locked
down separately with `AWSIM_REQUIRE_SIGNED_REQUESTS=true`, which
makes the dispatcher require every SDK call to carry a SigV4
signature whose access key resolves to a known IAM user.

Authentication uses the existing IAM service: operators are real
IAM users created via the standard `aws iam create-user` and
`aws iam create-login-profile` API. Passwords are bcrypt-hashed
on the user's `LoginProfile`; an optional virtual MFA device is
verified against its base32 seed using RFC 6238 TOTP. See
[IAM enforcement](iam-enforcement.md) for the policy-evaluation
side of the same model.

## Endpoints

All routes are POST unless noted, return JSON, and share the
`/_awsim/auth/` prefix.

| Route | Purpose |
|-------|---------|
| `POST /_awsim/auth/setup` | First-run bootstrap. Consumes the printed token, creates the root operator. Returns the root access keys once. |
| `POST /_awsim/auth/login` | Verify password (+ optional MFA), mint a 12-hour HMAC-signed session token, set the `awsim_session` HTTP-only cookie. |
| `POST /_awsim/auth/logout` | Clear the session cookie. Sessions are stateless so there is nothing server-side to revoke. |
| `GET /_awsim/auth/whoami` | Return `{auth_required, setup_required, principal}` so the UI can distinguish loginless dev from "auth on, no session". Always 200. |
| `GET /_awsim/auth/credentials` | Return the signed-in operator's first active IAM access key + secret for the UI's SigV4 signer. Gated by the session cookie. |
| `POST /_awsim/auth/reveal-access-key` | Return the plaintext secret for an existing access key. AWSim retains secrets locally, unlike real AWS which hides them after creation. Gated. |

## First-run bootstrap

When `AWSIM_REQUIRE_OPERATOR_AUTH=true` is on and the IAM
snapshot has no `root` login profile, AWSim prints a setup
token and a curl one-liner to stdout, then refuses every admin
request with `503 OperatorSetupRequired` until setup runs.

```text
===================================================================
 AWSim operator setup required
-------------------------------------------------------------------
 AWSIM_REQUIRE_OPERATOR_AUTH=true and no root login profile
 exists. Pick a root password and POST to /_awsim/auth/setup:

 curl -s -X POST http://localhost:4566/_awsim/auth/setup \
      -H 'content-type: application/json' \
      -d '{"bootstrap_token":"<printed>","password":"<choose>"}'
===================================================================
```

`setup` creates an IAM user named `root`, attaches a login
profile with the chosen password (bcrypt-hashed, never stored
in plaintext), and mints an initial access key pair which is
returned to the caller as the response body. Save the keys; the
secret is not retrievable later. On every subsequent boot the
snapshot contains the root user, so the gate goes straight to
Complete and no token is printed.

The bootstrap token itself is a 64-character hex string backed by
32 bytes of OS randomness. AWSim stores only its SHA-256 hash in
memory; the raw token never touches disk. The compare is
constant-time. Bootstrap tokens are not persisted across
restarts: if you restart before running setup, a fresh token is
printed.

## Login from the admin UI

When operator auth is on, the admin UI redirects to `/login` on
first load. Sign in with an IAM username, password, and (if the
user has an enabled virtual MFA device) the current 6-digit code
from your authenticator app.

A successful login sets the `awsim_session` HTTP-only cookie
and returns a JSON body containing the same token (for non-
browser clients). The top bar shows the logged-in principal and
a sign-out button. Sign-out clears the cookie via the logout
endpoint and bounces back to `/login`.

## Login from a non-browser client

```bash
curl -s -X POST http://localhost:4566/_awsim/auth/login \
     -H 'content-type: application/json' \
     -d '{"username":"alice","password":"hunter2","mfa_code":"123456"}'
# => {"session_token": "...", "expires_in": 43200, "principal": "iam-user:000000000000/alice"}

curl http://localhost:4566/_awsim/health \
     -H 'authorization: Bearer <session_token>'
```

The 12-hour TTL matches the AWS IAM console default. Sessions
do not survive a process restart: the HMAC signing key is
regenerated at boot, so every active token becomes invalid.
Clients should re-authenticate on restart, the same way they
already handle regional failover against real AWS.

## Creating more operator users

Once setup runs you can sign in as root and create additional
IAM users via the standard API:

```bash
export AWS_ACCESS_KEY_ID=<root-key-from-setup>
export AWS_SECRET_ACCESS_KEY=<root-secret-from-setup>

aws iam create-user --user-name alice
aws iam create-login-profile \
    --user-name alice \
    --password 'Hunter2!Strong' \
    --no-password-reset-required
aws iam attach-user-policy \
    --user-name alice \
    --policy-arn arn:aws:iam::aws:policy/ReadOnlyAccess
```

To require MFA on Alice's login:

```bash
aws iam create-virtual-mfa-device \
    --virtual-mfa-device-name alice-totp \
    --query VirtualMFADevice.Base32StringSeed \
    --output text
# scan the base32 seed into your authenticator, then enable:
aws iam enable-mfa-device \
    --user-name alice \
    --serial-number arn:aws:iam::000000000000:mfa/alice-totp \
    --authentication-code1 <first-6-digit-code> \
    --authentication-code2 <next-6-digit-code>
```

The two consecutive codes are verified against the seed exactly
the way real IAM verifies them; supplying random digits is
rejected with `InvalidAuthenticationCode`.

## Login throttling

The login endpoint counts failures per username over a 60-second
sliding window. The sixth failure within the window returns
`429 ThrottlingException` with a `Retry-After` header naming the
seconds to wait. A successful login clears the counter so a
typo before a correct password does not trip the lockout.

The counter is scoped per username, so flooding one account does
not deny service to unrelated operators logging in concurrently.

## Gateway gating with AWSIM_REQUIRE_SIGNED_REQUESTS

Operator auth governs admin access; the simulated AWS gateway
is a separate layer. Set `AWSIM_REQUIRE_SIGNED_REQUESTS=true` to
require every SDK call to carry a SigV4 signature whose access
key ID matches a known IAM user.

```text
Authorization: AWS4-HMAC-SHA256
   Credential=AKIA.../20260101/us-east-1/s3/aws4_request,
   SignedHeaders=host;x-amz-date, Signature=...
```

Unsigned calls return `MissingAuthenticationTokenException`,
unknown keys return `InvalidClientTokenId`, both with HTTP 400.
The check runs before the per-service handler so unauthorized
clients cannot probe resource existence via error timing.

The two gates compose: `AWSIM_IAM_ENFORCE=true` plus
`AWSIM_REQUIRE_SIGNED_REQUESTS=true` plus
`AWSIM_REQUIRE_OPERATOR_AUTH=true` gives a deployment where
every SDK call is signed, authorized, and every admin call
authenticated.

## Cryptographic signature verification with AWSIM_VERIFY_SIGV4

`AWSIM_REQUIRE_SIGNED_REQUESTS=true` only checks that the
Authorization header is present and the access key ID resolves to
a principal. The signature itself is trusted as-is, which means a
stolen access key ID is sufficient to impersonate the principal.

Set `AWSIM_VERIFY_SIGV4=true` to add real cryptographic
verification: the gateway recomputes the canonical request,
derives the signing key from the bound secret, and compares the
resulting hex digest against the supplied signature in constant
time. The check has a five-minute clock-skew window either side,
matching AWS. Off by default so legacy clients sending
`Signature=fakesignature` keep working.

The admin access key (`AWSIM_ADMIN_ACCESS_KEY`, default
`awsim-admin`) bypasses verification because it has no real
secret; it's the break-glass key, not a real principal. Real
operator IAM credentials must produce valid signatures.

## Root user protection

When operator auth is on, the IAM service hard-rejects any
mutation that targets the `root` user with
`AccessDeniedException`, regardless of the caller's policies. Real
AWS treats root as the account-owner identity that exists outside
the IAM principal hierarchy; AWSim mirrors that for every
operation that takes a `UserName` parameter and changes state:
delete / update user, password profile changes, access-key
mutations, policy attach / detach / put / delete, group
membership, MFA, SSH keys, signing certificates, and
service-specific credentials. Read-only operations (Get*, List*)
stay readable so root metadata is auditable.

The first-run bootstrap is allowed to create root via
`CreateUser("root")` + `CreateLoginProfile` + `CreateAccessKey`
because the setup endpoint constructs its `RequestContext` with
`internal_bypass = true`. No external HTTP call can set that
flag.

## UI signs with operator credentials

After sign-in the admin UI fetches the operator's IAM access key
+ secret from `GET /_awsim/auth/credentials` and uses them to
SigV4-sign every outbound AWS request. The gateway then sees the
real principal at policy-evaluation time, so an IAM user with a
restrictive policy gets 403s on the operations they're not
allowed to perform. When operator auth is off (or no session is
present), the UI falls back to the admin bypass key and works
without setup.

Credentials are cached client-side for the session lifetime and
auto-refreshed when within 5 minutes of expiry. They are never
exposed to other scripts: callers go through `sign()` in
`$lib/credentials.svelte` which returns ready-to-use headers
without disclosing the raw secret.

## Break-glass

If you lose every IAM user with a working access key, set
`AWSIM_ADMIN_ACCESS_KEY=<some-key-id>` and use that key. The
gateway short-circuits enforcement for that key so you can
reset IAM state without rebuilding the deployment.

## Threat model

AWSim is an emulator, not a security boundary. Operator auth
exists to keep the admin UI from being a one-click takeover of a
shared dev environment, and to make AWS SDK retry / signing
logic behave consistently across local and prod. The
implementation is straightforward (bcrypt, HMAC-signed bearer
tokens, RFC 6238 TOTP) but the simulator does not run penetration
tests and does not promise resistance to side channels. Do not
expose AWSim to a public network.
