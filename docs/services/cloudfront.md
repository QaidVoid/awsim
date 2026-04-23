# CloudFront

Amazon CloudFront CDN for distributing content globally with low latency using edge locations.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `RestXml` |
| Signing Name | `cloudfront` |
| API Version | `2020-05-31` |
| Persistence | No |

CloudFront uses the `RestXml` protocol: XML request/response bodies with REST routing. Paths follow `/{apiVersion}/distribution/...`. It is a **global** service — state is shared across all regions.

## Quick Start

Create a CloudFront distribution pointing to an S3 origin:

```bash
# Create a distribution
curl -s -X POST http://localhost:4566/2020-05-31/distribution \
  -H "Content-Type: application/xml" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/cloudfront/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '<?xml version="1.0" encoding="UTF-8"?>
<DistributionConfig>
  <CallerReference>ref-001</CallerReference>
  <Comment>My distribution</Comment>
  <Enabled>true</Enabled>
  <Origins>
    <Quantity>1</Quantity>
    <Items>
      <Origin>
        <Id>my-s3-origin</Id>
        <DomainName>my-bucket.s3.amazonaws.com</DomainName>
        <S3OriginConfig><OriginAccessIdentity></OriginAccessIdentity></S3OriginConfig>
      </Origin>
    </Items>
  </Origins>
  <DefaultCacheBehavior>
    <TargetOriginId>my-s3-origin</TargetOriginId>
    <ViewerProtocolPolicy>redirect-to-https</ViewerProtocolPolicy>
    <CachePolicyId>658327ea-f89d-4fab-a63d-7e88639e58f6</CachePolicyId>
  </DefaultCacheBehavior>
</DistributionConfig>'
```

## Operations

### Distributions
- `CreateDistribution` — create a CloudFront distribution with origin and cache settings
  - Input: `DistributionConfig` XML with `CallerReference` (unique string), `Origins`, `DefaultCacheBehavior` (requires `TargetOriginId`, `ViewerProtocolPolicy`), `Enabled`
  - Returns: `Distribution` with `Id`, `DomainName` (e.g., `d1234abcd.cloudfront.net`), `Status` (`InProgress` then `Deployed`)
  - `CallerReference` must be unique per request to prevent duplicate creation

- `GetDistribution` — get a specific distribution by ID
  - Path: `GET /2020-05-31/distribution/{Id}`
  - Returns: `Distribution` including `DistributionConfig` and current `Status`

- `ListDistributions` — list all distributions in the account
  - Path: `GET /2020-05-31/distribution`
  - Returns: `DistributionList` with paginated `Items`

- `DeleteDistribution` — delete a distribution (must be disabled first)
  - Path: `DELETE /2020-05-31/distribution/{Id}`
  - Requires `If-Match` header with the ETag from a previous `GetDistribution`

- `UpdateDistribution` — update distribution configuration
  - Path: `PUT /2020-05-31/distribution/{Id}/config`
  - Requires `If-Match` ETag header

### Origin Access Controls
- `CreateOriginAccessControl` — create an OAC for restricting S3 access to CloudFront only
  - Input: `OriginAccessControlConfig` with `Name`, `SigningProtocol` (`sigv4`), `SigningBehavior` (`always`, `never`, `no-override`)
  - Returns: `OriginAccessControl` with `Id`

- `ListOriginAccessControls` — list all origin access controls
- `DeleteOriginAccessControl` — delete an origin access control

### Cache Policies
- `ListCachePolicies` — list available cache policies
  - Returns: a built-in set of AWS managed policies including `CachingOptimized` (`658327ea-f89d-4fab-a63d-7e88639e58f6`)

### Tags
- `TagResource` — add tags to a distribution (ARN-based)
- `ListTagsForResource` — list tags on a distribution

## Curl Examples

```bash
# 1. Create a distribution (using AWS CLI is much easier for CloudFront's XML)
aws --endpoint-url http://localhost:4566 \
  cloudfront create-distribution \
  --distribution-config '{
    "CallerReference":"ref-123",
    "Origins":{"Quantity":1,"Items":[{"Id":"s3","DomainName":"my-bucket.s3.amazonaws.com","S3OriginConfig":{"OriginAccessIdentity":""}}]},
    "DefaultCacheBehavior":{"TargetOriginId":"s3","ViewerProtocolPolicy":"redirect-to-https","CachePolicyId":"658327ea-f89d-4fab-a63d-7e88639e58f6","ForwardedValues":{"QueryString":false,"Cookies":{"Forward":"none"}}},
    "Comment":"My CDN","Enabled":true
  }'

# 2. List all distributions
aws --endpoint-url http://localhost:4566 cloudfront list-distributions

# 3. Create an Origin Access Control
aws --endpoint-url http://localhost:4566 \
  cloudfront create-origin-access-control \
  --origin-access-control-config '{
    "Name":"my-oac",
    "Description":"OAC for S3",
    "SigningProtocol":"sigv4",
    "SigningBehavior":"always",
    "OriginAccessControlOriginType":"s3"
  }'
```

## SDK Example

```typescript
import {
  CloudFrontClient,
  CreateDistributionCommand,
  ListDistributionsCommand,
} from '@aws-sdk/client-cloudfront';

const cf = new CloudFrontClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create a distribution
const { Distribution } = await cf.send(new CreateDistributionCommand({
  DistributionConfig: {
    CallerReference: `ref-${Date.now()}`,
    Comment: 'My distribution',
    Enabled: true,
    Origins: {
      Quantity: 1,
      Items: [{
        Id: 'my-s3',
        DomainName: 'my-bucket.s3.amazonaws.com',
        S3OriginConfig: { OriginAccessIdentity: '' },
      }],
    },
    DefaultCacheBehavior: {
      TargetOriginId: 'my-s3',
      ViewerProtocolPolicy: 'redirect-to-https',
      CachePolicyId: '658327ea-f89d-4fab-a63d-7e88639e58f6',
      TrustedSigners: { Enabled: false, Quantity: 0 },
      ForwardedValues: {
        QueryString: false,
        Cookies: { Forward: 'none' },
      },
      MinTTL: 0,
    },
  },
}));

console.log('Distribution ID:', Distribution?.Id);
console.log('Domain:', Distribution?.DomainName); // e.g., d1234abcd.cloudfront.net

// List all distributions
const { DistributionList } = await cf.send(new ListDistributionsCommand({}));
console.log('Total distributions:', DistributionList?.Quantity);
```

## Behavior Notes

- CloudFront is a **global** service — state is shared across all regions under the same account.
- Distributions are recorded in AWSim but no actual CDN edge routing or caching occurs.
- `ListCachePolicies` returns a single built-in `CachingOptimized` managed policy (ID `658327ea-f89d-4fab-a63d-7e88639e58f6`).
- Distribution status starts as `InProgress` and transitions to `Deployed` quickly (simulated).
- `DeleteDistribution` requires the distribution to be disabled first (`Enabled: false`), matching real CloudFront behavior.
- State is in-memory only and lost on restart.
