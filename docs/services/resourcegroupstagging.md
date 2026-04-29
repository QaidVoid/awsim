# Resource Groups Tagging API

Cross-service resource discovery by tag — the API SDK clients (Terraform, CDK, AWS CLI's `tag-editor` workflow) call to find every resource in an account that carries a given tag.

**Protocol:** `AwsJson1_1`
**Signing name:** `tagging`
**Persistent:** Yes (snapshotted with the rest of the account state)

## Quick Start

Tag a couple of resources, then list everything that matches:

```bash
# Tag two resources
curl -s -X POST http://localhost:4566/ \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: ResourceGroupsTaggingAPI_20170126.TagResources" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/tagging/aws4_request" \
  -d '{
    "ResourceARNList": [
      "arn:aws:s3:::orders-bucket",
      "arn:aws:dynamodb:us-east-1:000000000000:table/users"
    ],
    "Tags": { "Env": "prod", "Team": "core" }
  }'

# List every resource carrying Env=prod
curl -s -X POST http://localhost:4566/ \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: ResourceGroupsTaggingAPI_20170126.GetResources" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/tagging/aws4_request" \
  -d '{
    "TagFilters": [{ "Key": "Env", "Values": ["prod"] }]
  }'
```

## Operations

| Operation | Description |
|-----------|-------------|
| `TagResources` | Apply a `Tags` map to every ARN in `ResourceARNList`. Returns `FailedResourcesMap` (always empty here — emulator never fails the write) |
| `UntagResources` | Remove the listed `TagKeys` from every ARN in `ResourceARNList` |
| `GetResources` | List tagged resources, optionally filtered by `TagFilters` (`{Key, Values?}`), `ResourceTypeFilters` (`service` or `service:resourceType`), and paginated via `ResourcesPerPage` + `PaginationToken` |
| `GetTagKeys` | Return the union of every tag key currently in use |
| `GetTagValues` | Return all values associated with a single tag `Key` |
| `DescribeReportCreation` | Stub — returns `Status: "NONE"` |
| `StartReportCreation` | Stub — accepts but does not generate a report |
| `GetComplianceSummary` | Stub — returns an empty `SummaryList` |

## SDK Example

```typescript
import {
  ResourceGroupsTaggingAPIClient,
  TagResourcesCommand,
  GetResourcesCommand,
} from '@aws-sdk/client-resource-groups-tagging-api';

const client = new ResourceGroupsTaggingAPIClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Tag a couple of resources
await client.send(new TagResourcesCommand({
  ResourceARNList: [
    'arn:aws:s3:::orders-bucket',
    'arn:aws:dynamodb:us-east-1:000000000000:table/users',
  ],
  Tags: { Env: 'prod', Team: 'core' },
}));

// Find every prod resource
const { ResourceTagMappingList } = await client.send(new GetResourcesCommand({
  TagFilters: [{ Key: 'Env', Values: ['prod'] }],
}));
console.log(ResourceTagMappingList?.map(r => r.ResourceARN));
```

## Behavior Notes

- The service maintains its own ARN → tag map per (account, region). It does **not** consult the owning service's tag store, so resources tagged via (e.g.) `aws s3api put-bucket-tagging` are not visible here unless you also call `TagResources`. Real AWS propagates from the owning service asynchronously; modelling that would mean cross-service plumbing in every tag-aware service. PRs welcome.
- `GetResources` results are sorted by ARN for deterministic pagination cursors. `ResourcesPerPage` is clamped to 100.
- `ResourceTypeFilters` accepts both `service` (e.g. `s3`) and `service:resourceType` (e.g. `dynamodb:table`); matching is case-insensitive against the ARN's service segment.
- `IncludeComplianceDetails` and `ExcludeCompliantResources` on `GetResources` are accepted but not enforced — the emulator has no compliance signal to filter on.
