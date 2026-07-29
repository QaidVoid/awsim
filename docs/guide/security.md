# Security Model

AWSim is a development emulator. This page states what it assumes, so you can tell whether your deployment matches those assumptions.

## Trust Boundary

**AWSim assumes a trusted local context and trusts its callers.**

By default there is no authentication and no authorization: any client that can reach the port can call any API. That is the correct default for the primary use case, a developer running AWSim on their own machine or a CI job running it as a throwaway container.

It is not a suitable default for a shared host or a network anyone untrusted can reach.

## Features That Make Outbound Requests

Several emulated services fetch URLs supplied by the caller. This is correct emulation of the AWS service, and it also means a caller can induce AWSim to make requests on their behalf:

| Feature | What it fetches |
|---|---|
| API Gateway HTTP integrations | The integration URI configured on a route |
| Cognito federation | The identity provider's JWKS and discovery endpoints |
| Bedrock backend | The configured upstream model endpoint |
| RDS Data API, Lambda | Container images, when a container runtime is in use |

On a trusted network this is the feature working. On an exposed instance it is a server-side request forgery primitive. Bear it in mind before exposing AWSim.

## Hardening

Four gates are available, all off by default. They are independent and compose:

| Setting | What it does |
|---|---|
| `AWSIM_IAM_ENFORCE` | Evaluates IAM policies on every request. Keys bound to an IAM user are enforced; unbound keys act as administrators so the first users can be created. See [IAM enforcement](/guide/iam-enforcement) |
| `AWSIM_REQUIRE_SIGNED_REQUESTS` | Rejects any request whose access key does not resolve to a known IAM user, with `InvalidClientTokenId` |
| `AWSIM_VERIFY_SIGV4` | Cryptographically verifies every SigV4 signature against the bound secret, rejecting forgeries with `SignatureDoesNotMatch` |
| `AWSIM_REQUIRE_OPERATOR_AUTH` | Gates the admin UI and admin endpoints behind a login. See [operator auth](/guide/operator-auth) |

To restrict reachability instead, bind loopback:

```bash
awsim --bind 127.0.0.1
```

The default binds all interfaces, because publishing a container port requires it. When AWSim starts on a non-loopback address with no gate enabled, it logs a one-line notice saying so.

## Container Posture

The published image runs as an unprivileged user (uid 65532), not root. If you mount a volume at `/data` that is owned by root, the process cannot write to it; either `chown` it to `65532:65532` or let Docker create the volume.

## What Is Verified

The following were checked directly rather than assumed:

- SigV4 signature comparison and the operator bootstrap token comparison both use constant-time equality.
- Cognito stores passwords with bcrypt and signs tokens with RS256; no `none` algorithm path exists.
- DynamoDB's SQLite layer binds every caller-supplied value as a parameter and interpolates only static column lists, so it is not injectable.
- Archive extraction, and every path built from caller-supplied text, is contained by a shared helper that rejects traversal components.

## What This Page Is Not

AWSim is not hardened for exposure to an untrusted network, and the gates above do not make it so. They make it usable in a semi-trusted setting such as a shared development host. Do not put it on the public internet.
