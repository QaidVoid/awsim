# Request authentication

AWSim accepts three independent layers of caller authentication for
the simulated AWS API surface. Each is opt-in via its own environment
variable; the layers compose, so a deployment can require any
combination. The defaults stay loginless so single-user local
development just works.

| Variable | Default | Purpose |
|----------|---------|---------|
| `AWSIM_VERIFY_SIGV4` | `false` | Recompute SigV4 (header or presigned URL) and reject mismatches with HTTP 403 `SignatureDoesNotMatch`. |
| `AWSIM_REQUIRE_SIGNED_REQUESTS` | `false` | Reject any request that does not present a SigV4 signature (header or presigned URL) at all. The signature itself is still only cryptographically verified when `AWSIM_VERIFY_SIGV4=true`. |
| `AWSIM_REQUIRE_OPERATOR_AUTH` | `false` | Lock down the admin UI and admin HTTP surface behind a logged-in operator. Covered in detail in [Operator authentication](operator-auth.md). |

See [IAM enforcement](iam-enforcement.md) for the *authorization*
layer (policy evaluation) that runs after authentication succeeds.

## SigV4 verification

AWS clients sign every request with SigV4. The dispatcher in
`awsim-core/gateway.rs` recognises both forms:

- **Header signing.** `Authorization: AWS4-HMAC-SHA256 Credential=...,
  SignedHeaders=..., Signature=...` is the standard SDK path. Body
  hash either comes from `x-amz-content-sha256` (trusted when present,
  matching AWS) or is computed over the body.
- **Presigned URLs.** Every SigV4 input lives in the query string
  (`X-Amz-Algorithm`, `X-Amz-Credential`, `X-Amz-Date`,
  `X-Amz-Expires`, `X-Amz-SignedHeaders`, `X-Amz-Signature`). The
  signer's `Host` header is canonicalised against the request's
  inbound `Host`.

### Outcomes

The verifier returns one of three outcomes, each mapped to an
AWS-shaped error so SDK error handling works unchanged:

| Outcome | HTTP | Error code | Trigger |
|---------|------|------------|---------|
| `Ok` | (proceed) | (none) | Signature matched and the request fell inside the clock-skew window. |
| `IncompleteSignature` | 400 | `IncompleteSignatureException` | One of the SigV4 inputs (date, credential, signed-headers) was absent or malformed. |
| `SignatureMismatch` | 403 | `SignatureDoesNotMatch` | Signature did not match the recomputed value, body hash mismatch, or clock skew > 5 minutes either side. Real AWS surfaces this as 403, not 400. |

Clock skew tolerance is 5 minutes either side, matching AWS. The
admin access key (`AWSIM_ADMIN_ACCESS_KEY`) is exempt from
verification: the management UI and bootstrap flows already mint
signed requests through the admin path, so cryptographic verification
of the admin key would be redundant.

### Body-hash sentinels

AWS allows callers to opt out of payload hashing in two scenarios.
AWSim honours both:

- `UNSIGNED-PAYLOAD` — S3 large uploads and presigned PUTs where the
  body isn't available at signing time. The verifier accepts the
  literal and skips the body-hash check.
- `STREAMING-AWS4-HMAC-SHA256-PAYLOAD` — S3 chunked uploads. Treated
  like `UNSIGNED-PAYLOAD` for now; chunk-level hashing can be added
  when a workload demands it.

### When to turn it on

For local development, the default `false` lets SDK clients send
`Signature=fakesignature` and still get a routed response. Flip
`AWSIM_VERIFY_SIGV4=true` when you need to reproduce real AWS
behaviour: rotating access keys, debugging a client that mis-signs,
or asserting that a stolen access-key ID alone can't impersonate a
principal (the holder of the secret is the only party able to
produce a valid signature).

## Bearer tokens

Several AWS services issue an opaque bearer token instead of an
access-key pair: CodeArtifact's `GetAuthorizationToken`, IAM Identity
Center's SCIM endpoint, ECR's `GetAuthorizationToken`. AWSim ships a
shared `awsim_core::bearer_token` helper:

```rust
use std::time::Duration;
use awsim_core::bearer_token;

let token = bearer_token::mint("arn:aws:iam::000000000000:user/alice",
                               Duration::from_secs(12 * 60 * 60));
// later, on a request carrying `Authorization: Bearer <token>`:
let principal = bearer_token::verify(&token)?;
```

The token is a URL-safe base64 envelope of `{version, expiry,
principal, hmac}` signed with a process-local HMAC-SHA256 key
regenerated each boot. Verification is self-contained — the server
stores nothing per-token, which is what makes the issuer
unforgeable across process restarts without a persisted shared
secret.

Tokens carry an absolute expiry; expired tokens surface
`AccessDeniedException` so handlers can `?` the result. Clients
must reissue after the expiry rather than treating it as a
refreshable session.

ECR's `GetAuthorizationToken` uses a similar HMAC-signed token but
embeds the auth in the AWS-documented `AWS:<credential>` wire format
docker / OCI clients sign with Basic auth. See `awsim-ecr` for the
service-specific signer.

## SigV4 + bearer composition

Bearer tokens authenticate the *service-specific data plane* (the
CodeArtifact repository URL, the ECR registry URL, the SCIM endpoint)
while SigV4 authenticates the *AWS control plane* (the API operations
themselves). A typical CodeArtifact flow:

1. Client calls `codeartifact:GetAuthorizationToken` over SigV4. The
   API call is authenticated like any other AWS call.
2. Server returns a bearer token with the requested TTL.
3. Client calls the repository HTTP surface with
   `Authorization: Bearer <token>` — no SigV4 involved on the data
   plane.

Both layers go through the same `PrincipalLookup` so a single IAM
user can be referenced by either.

## Strict-signed mode

`AWSIM_REQUIRE_SIGNED_REQUESTS=true` rejects every request that
arrives without a `Authorization: AWS4-HMAC-SHA256 ...` header or a
presigned URL carrying `X-Amz-Signature`. The exact error mirrors
real AWS:

```
HTTP/1.1 403 Forbidden
{"__type":"MissingAuthenticationTokenException",
 "message":"Request is missing Authentication Token"}
```

Strict mode does not by itself verify the signature; pair it with
`AWSIM_VERIFY_SIGV4=true` for the full "must be signed AND signed
correctly" gate. Together they reproduce the behaviour of a real AWS
region: anonymous requests bounce immediately, mis-signed requests
bounce after cryptographic verification.

## Future authentication channels

These layers are sketched in spec 0011 and not yet implemented in
this section of the docs:

- **SNS HTTPS notification signing.** Subscribers receive a payload
  signed with RSA-SHA1 over the canonical message; the
  `x-amz-sns-signing-cert-url` header points at a fetchable cert
  served by AWSim itself. The cert URL endpoint will live under
  `/_awsim/sns-cert/<keyid>.pem`.
- **IoT mTLS.** The MQTT broker will validate the client certificate
  against a Thing principal at connect time. Helper lives in
  `awsim-core::auth::mtls` once spec 0011 lands.

Once those ship, this page picks up the same pattern: a quick
reference for the env flag, the wire format, and the error surface.
