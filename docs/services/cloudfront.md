# CloudFront

Amazon CloudFront CDN for distributing content globally with low latency using edge locations.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `RestXml` |
| Signing Name | `cloudfront` |
| Persistence | No |

## Operations

### Distributions
- `CreateDistribution` — create a CloudFront distribution with origin and cache settings
- `GetDistribution` — get a specific distribution by ID
- `ListDistributions` — list all distributions in the account
- `DeleteDistribution` — delete a distribution
- `UpdateDistribution` — update distribution configuration (origins, behaviors, etc.)

### Origin Access Controls
- `CreateOriginAccessControl` — create an OAC for restricting S3 access to CloudFront
- `ListOriginAccessControls` — list all origin access controls
- `DeleteOriginAccessControl` — delete an origin access control

### Cache Policies
- `ListCachePolicies` — list available cache policies (returns a built-in set of managed policies)

### Tags
- `TagResource` — add tags to a distribution
- `ListTagsForResource` — list tags on a distribution

## Example

```bash
# Create a distribution
aws --endpoint-url http://localhost:4567 \
  cloudfront create-distribution \
  --distribution-config '{
    "CallerReference": "ref-123",
    "Origins": {
      "Quantity": 1,
      "Items": [{"Id":"my-s3","DomainName":"my-bucket.s3.amazonaws.com","S3OriginConfig":{"OriginAccessIdentity":""}}]
    },
    "DefaultCacheBehavior": {
      "TargetOriginId": "my-s3",
      "ViewerProtocolPolicy": "redirect-to-https",
      "CachePolicyId": "658327ea-f89d-4fab-a63d-7e88639e58f6",
      "ForwardedValues": {"QueryString":false,"Cookies":{"Forward":"none"}}
    },
    "Comment": "My distribution",
    "Enabled": true
  }'

# List distributions
aws --endpoint-url http://localhost:4567 \
  cloudfront list-distributions
```

## Notes

- CloudFront is a **global** service — state is shared across all regions under the same account.
- CloudFront uses the `RestXml` protocol (XML request/response with REST routing), using API version `2020-05-31`.
- Distributions are recorded in AWSim but no actual CDN edge routing or caching occurs.
- `ListCachePolicies` returns a single built-in `CachingOptimized` managed policy.
- State is in-memory only and lost on restart.
