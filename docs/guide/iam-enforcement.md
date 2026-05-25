# IAM Policy Enforcement

AWSim ships with a real IAM policy evaluation engine (`awsim-iam-policy`) that implements AWS IAM authorization semantics. Enforcement is **opt-in** via the `AWSIM_IAM_ENFORCE` environment variable. By default, AWSim accepts credentials and policies but does not evaluate them (preserving backwards-compatible behavior).

This page covers the policy decision flow. For who can log into the
admin UI and how SDK calls are required to carry a signed identity at
all, see [Operator authentication](operator-auth.md). The three gates
(`AWSIM_IAM_ENFORCE`, `AWSIM_REQUIRE_SIGNED_REQUESTS`,
`AWSIM_REQUIRE_OPERATOR_AUTH`) are independent and compose.

When enforcement is enabled, requests are evaluated against the full IAM decision flow: identity policies, resource policies, permissions boundaries, service control policies (SCPs), and session policies.

## What It Evaluates

The engine models the AWS IAM policy evaluation logic:

- **Identity policies** — inline and managed policies attached to the calling user, its groups, or the assumed role.
- **Resource policies** — policies attached to the target resource (e.g. S3 bucket policy, KMS key policy).
- **Permissions boundaries** — the maximum permissions a user or role can have.
- **Service control policies (SCPs)** — organization-level allow/deny guardrails.
- **Session policies** — inline policies passed to `AssumeRole` that further scope the session.

Decision order follows AWS: an explicit `Deny` anywhere wins; then SCPs must allow; then an `Allow` must be present in identity or resource policies; otherwise the request is implicitly denied.

## Enabling Enforcement

Set the environment variable before launching AWSim:

```bash
AWSIM_IAM_ENFORCE=true ./awsim
```

With Docker Compose:

```yaml
services:
  awsim:
    image: awsim:latest
    environment:
      - AWSIM_IAM_ENFORCE=true
      - AWSIM_DATA_DIR=/data
    volumes:
      - ./data:/data
    ports:
      - "4566:4566"
```

When enforcement is off (default), every request is allowed regardless of policy — useful for rapid prototyping. Turn it on to unit-test IAM policies and negative paths in your IaC.

## Quick Start

Create a user with an access key, attach an S3-restricted policy, and observe an explicit deny:

```bash
# Start AWSim with enforcement on
AWSIM_IAM_ENFORCE=true ./awsim &

# Create user + access key
aws --endpoint-url http://localhost:4566 iam create-user --user-name alice
KEY=$(aws --endpoint-url http://localhost:4566 iam create-access-key --user-name alice)
AK=$(echo "$KEY" | jq -r '.AccessKey.AccessKeyId')
SK=$(echo "$KEY" | jq -r '.AccessKey.SecretAccessKey')

# Attach a narrow inline policy: only allow listing one bucket
aws --endpoint-url http://localhost:4566 iam put-user-policy \
  --user-name alice \
  --policy-name read-only-mybucket \
  --policy-document '{
    "Version":"2012-10-17",
    "Statement":[{"Effect":"Allow","Action":"s3:ListBucket","Resource":"arn:aws:s3:::allowed-bucket"}]
  }'

# Try a request that SHOULD be allowed
AWS_ACCESS_KEY_ID=$AK AWS_SECRET_ACCESS_KEY=$SK \
  aws --endpoint-url http://localhost:4566 s3 ls s3://allowed-bucket

# Try a request that SHOULD be denied (PutObject is not in the policy)
AWS_ACCESS_KEY_ID=$AK AWS_SECRET_ACCESS_KEY=$SK \
  aws --endpoint-url http://localhost:4566 s3 cp README.md s3://allowed-bucket/
# => AccessDenied
```

The same call via curl (SigV4 signed — shown here with a placeholder signature for brevity):

```bash
curl -s -X GET "http://localhost:4566/allowed-bucket?list-type=2" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=$AK/20260421/us-east-1/s3/aws4_request, SignedHeaders=host, Signature=..."
```

## Supported Condition Operators

All 26 AWS condition operators are implemented:

| Category | Operators |
|----------|-----------|
| String | `StringEquals`, `StringNotEquals`, `StringEqualsIgnoreCase`, `StringNotEqualsIgnoreCase`, `StringLike`, `StringNotLike` |
| Numeric | `NumericEquals`, `NumericNotEquals`, `NumericLessThan`, `NumericLessThanEquals`, `NumericGreaterThan`, `NumericGreaterThanEquals` |
| Date | `DateEquals`, `DateNotEquals`, `DateLessThan`, `DateLessThanEquals`, `DateGreaterThan`, `DateGreaterThanEquals` |
| Boolean | `Bool` |
| Binary | `BinaryEquals` |
| IP Address | `IpAddress`, `NotIpAddress` (CIDR-aware, IPv4 + IPv6) |
| ARN | `ArnEquals`, `ArnLike`, `ArnNotEquals`, `ArnNotLike` |
| Null | `Null` |

### Qualifiers

Each base operator may be wrapped with one prefix and/or one suffix:

- **`ForAllValues:<op>`** — matches only when every value in a multi-valued context key satisfies the condition.
- **`ForAnyValue:<op>`** — matches when at least one value in the context key satisfies it.
- **`<op>IfExists`** — only applies the check when the context key is present; absent keys pass.

These compose: `ForAllValues:StringEqualsIfExists` is a valid operator.

## Enforced Services

The gateway checks identity policies for every request, but only a subset of services register **resource policy lookups** and therefore participate in full resource-policy evaluation:

| Service | Identity policy | Resource policy |
|---------|-----------------|-----------------|
| S3 | Yes | Bucket policies |
| DynamoDB | Yes | — |
| KMS | Yes | Key policies |
| SQS | Yes | Queue policies |
| SNS | Yes | — |
| Secrets Manager | Yes | Resource-based policies |
| Lambda | Yes | Function policies |
| IAM | Yes | — |

Services not listed above are **silently bypassed** — the enforcement hook is not wired in yet, so requests succeed regardless of policy. This lets you roll out enforcement incrementally without breaking existing tests.

## Policy Simulator API

`SimulateCustomPolicy` and `SimulatePrincipalPolicy` are no longer stubs — they run the real engine against the supplied policy documents and return one `EvaluationResult` per action/resource pair with `EvalDecision` set to `allowed`, `explicitDeny`, or `implicitDeny`.

```bash
aws --endpoint-url http://localhost:4566 iam simulate-custom-policy \
  --policy-input-list '{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"arn:aws:s3:::bucket/*"}]}' \
  --action-names s3:GetObject s3:PutObject \
  --resource-arns arn:aws:s3:::bucket/key
```

Returns two results: `s3:GetObject` → `allowed`, `s3:PutObject` → `implicitDeny`.

## Policy Validation

`CreatePolicy`, `CreatePolicyVersion`, `PutUserPolicy`, `PutGroupPolicy`, `PutRolePolicy`, `CreateRole`, and `UpdateAssumeRolePolicy` all parse their `PolicyDocument` / `AssumeRolePolicyDocument` input. A syntactically invalid policy now returns HTTP `400` with error code `MalformedPolicyDocument`:

```
Syntax errors in policy. unknown condition operator: StringEqualsFoo
```

Validation runs regardless of `AWSIM_IAM_ENFORCE` — a bad policy is always rejected at write time.

## AssumeRole Trust Policies

`sts:AssumeRole` (and its WebIdentity / SAML variants) routes the calling principal through the target role's `AssumeRolePolicyDocument`. The trust policy is evaluated as a resource-based policy with the role ARN as the resource and these condition variables populated from the request:

- `sts:ExternalId` — set from the `ExternalId` request parameter. Use with `StringEquals` to harden cross-account assumes against the confused-deputy attack.
- `aws:MultiFactorAuthPresent` — `true` when the caller supplied both `SerialNumber` and `TokenCode`, `false` otherwise. Use with `Bool` to gate the role behind an MFA challenge.
- `aws:MultiFactorAuthAge` — set to `0` when MFA is present. Use with `NumericLessThan` to enforce a recent challenge.
- `aws:SourceIp` — taken from the request's client IP. Use with `IpAddress` to restrict the role to a known network.
- `aws:PrincipalArn` / `aws:PrincipalAccount` / `aws:SourceAccount` — mirror the caller's identity for `StringEquals` checks.

A trust policy that declares any of these conditions rejects the assume request with `AccessDenied` when the condition isn't satisfied.

## Not Yet Implemented

- **Session tags** (`aws:PrincipalTag/*`, `aws:ResourceTag/*`) — parsed but empty context.
- **CloudTrail audit log** — denied requests are not recorded.
- **IAM Access Analyzer** — no public-access or external-principal findings.
- **VPC endpoint policies** — not modeled; no VPC concept in AWSim.
- **Resource control policies (RCPs)** — not evaluated.

See the [IAM & STS](/services/iam) service page for the operation catalog.
