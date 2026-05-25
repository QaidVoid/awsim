# Multi-tenant Isolation

AWSim partitions every service's state by `(account_id, region)`, so two
tenants sharing the same AWSim instance never see each other's
resources. A request issued under a different region than the resource
was created in also misses, mirroring how AWS itself scopes resources to
a regional control plane.

## How the scoping works

Each service that stores state holds an `AccountRegionStore<T>` keyed by
`(account_id, region)` and constructs a fresh `T` on first access for
any new tenant slot. Every handler resolves its working set through
`store.get(&ctx.account_id, &ctx.region)`, so:

- Two callers with different access keys (mapping to different
  accounts) get independent state.
- The same access key calling against two regions gets independent
  state per region.

The conformance test
`crates/awsim-conformance/tests/account_region_isolation.rs` exercises
this end-to-end on a representative sample (SQS, DynamoDB, Secrets
Manager) and any service added to that test will need the same `(account,
region)` discipline to pass.

## Global services

A small set of AWS services intentionally have a single global control
plane: IAM, Route 53, Organizations, CloudFront. AWSim treats those as
per-account only and ignores `ctx.region`. Their resource ARNs leave the
region segment empty, matching AWS.

## Cross-account access

When tenant A wants to grant tenant B read access to a resource, AWS
uses two mechanisms:

- **Resource-based policies** (S3 bucket policies, KMS key policies,
  SecretsManager resource policies, Lambda function policies). AWSim
  evaluates these through the `AuthzEngine`'s `resource_policy_lookups`,
  so a bucket policy that grants `arn:aws:iam::222222222222:root` lets
  account 222 read.
- **Resource Access Manager (RAM) shares.** AWSim implements the RAM
  surface; consumers that have accepted a share see the shared resource
  in their account context.

Both paths require IAM enforcement (`AWSIM_IAM_ENFORCE=true`). Without
enforcement, AWSim still partitions state by `(account, region)` but
doesn't gate cross-account reads on a policy.

## Cross-region access

Most AWS services keep resources strictly regional. The exceptions are:

- **KMS multi-region keys** — replicated across regions by design; the
  `MultiRegion` flag on the key opts into this.
- **S3 cross-region replication** — explicit per-bucket configuration.
- **DynamoDB Global Tables** — replicas in named regions.
- **RDS read replicas** — opt-in per replica.

Outside those, a foreign-region ARN typically surfaces as
`ResourceNotFoundException` or `ValidationException`, depending on the
service. The conformance test treats this as expected behavior.
