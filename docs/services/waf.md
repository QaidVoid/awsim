# WAF

AWS WAF v2 (Web Application Firewall) for protecting web applications from common exploits using rules and rule groups.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `wafv2` |
| Target Prefix | `AWSWAF_20190729` |
| Persistence | Yes |

## Quick Start

Create a Web ACL with an IP set rule to block specific CIDR ranges:

```bash
# Create an IP set for blocking
IPSET_ID=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSWAF_20190729.CreateIPSet" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/wafv2/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"blocked-ips","Scope":"REGIONAL","IPAddressVersion":"IPV4","Addresses":["192.168.100.0/24","10.0.0.0/8"]}' \
  | jq -r '.Summary.Id')

IPSET_LOCKTOKEN=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSWAF_20190729.GetIPSet" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/wafv2/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"Name\":\"blocked-ips\",\"Scope\":\"REGIONAL\",\"Id\":\"$IPSET_ID\"}" \
  | jq -r '.LockToken')

# Create a Web ACL that blocks the IP set
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSWAF_20190729.CreateWebACL" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/wafv2/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"Name\":\"my-web-acl\",\"Scope\":\"REGIONAL\",\"DefaultAction\":{\"Allow\":{}},\"Rules\":[{\"Name\":\"block-bad-ips\",\"Priority\":1,\"Statement\":{\"IPSetReferenceStatement\":{\"ARN\":\"arn:aws:wafv2:us-east-1:000000000000:regional/ipset/blocked-ips/$IPSET_ID\"}},\"Action\":{\"Block\":{}},\"VisibilityConfig\":{\"SampledRequestsEnabled\":true,\"CloudWatchMetricsEnabled\":true,\"MetricName\":\"block-bad-ips\"}}],\"VisibilityConfig\":{\"SampledRequestsEnabled\":true,\"CloudWatchMetricsEnabled\":true,\"MetricName\":\"my-web-acl\"}}"
```

## Operations

### Web ACLs
- `CreateWebACL` — create a Web ACL with rules and a default action
  - Input: `Name` (required), `Scope` (`REGIONAL` for ALB/API Gateway, `CLOUDFRONT` for CloudFront), `DefaultAction` (`{Allow:{}}` or `{Block:{}}`), `Rules` (list of rules), `VisibilityConfig` (`{SampledRequestsEnabled, CloudWatchMetricsEnabled, MetricName}`)
  - Returns: `Summary` with `Id`, `ARN`, `Name`, `LockToken`

- `GetWebACL` — get a Web ACL by name, scope, and ID
  - Input: `Name`, `Scope`, `Id`
  - Returns: `WebACL` (full configuration), `LockToken` (required for updates)

- `ListWebACLs` — list Web ACLs for a given scope
  - Input: `Scope`, optional `NextMarker`, `Limit`
  - Returns: `WebACLs` list with `Name`, `Id`, `ARN`, `Description`

- `DeleteWebACL` — delete a Web ACL
  - Input: `Name`, `Scope`, `Id`, `LockToken` (must be retrieved from `GetWebACL` first)

- `UpdateWebACL` — update Web ACL rules or default action
  - Input: `Name`, `Scope`, `Id`, `LockToken`, `DefaultAction`, `Rules`, `VisibilityConfig`

### IP Sets
- `CreateIPSet` — create an IP set containing CIDR ranges for use in rule conditions
  - Input: `Name` (required), `Scope`, `IPAddressVersion` (`IPV4` or `IPV6`), `Addresses` (list of CIDR strings, e.g., `["1.2.3.4/32", "10.0.0.0/8"]`), optional `Description`
  - Returns: `Summary` with `Id`, `ARN`, `Name`, `LockToken`

- `GetIPSet` — get an IP set by name, scope, and ID
  - Input: `Name`, `Scope`, `Id`
  - Returns: `IPSet` with `Name`, `Id`, `ARN`, `Description`, `IPAddressVersion`, `Addresses`; and `LockToken`

- `ListIPSets` — list IP sets for a given scope
  - Input: `Scope`, optional `NextMarker`, `Limit`

- `DeleteIPSet` — delete an IP set
  - Input: `Name`, `Scope`, `Id`, `LockToken`

- `UpdateIPSet` — add or remove CIDR addresses from an IP set
  - Input: `Name`, `Scope`, `Id`, `LockToken`, `Addresses` (complete new list replaces existing)

### Rule Groups
- `CreateRuleGroup` — create a reusable rule group
  - Input: `Name`, `Scope`, `Capacity` (integer, WAF capacity units), `Rules` (list), `VisibilityConfig`
  - Returns: `Summary` with `Id`, `ARN`, `Name`

- `GetRuleGroup` — get a rule group by name, scope, and ID
- `ListRuleGroups` — list rule groups for a given scope
- `DeleteRuleGroup` — delete a rule group

## Curl Examples

```bash
# 1. Create an IP set
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSWAF_20190729.CreateIPSet" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/wafv2/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"allowlist","Scope":"REGIONAL","IPAddressVersion":"IPV4","Addresses":["203.0.113.0/24","198.51.100.0/24"],"Description":"Trusted IPs"}'

# 2. List all Web ACLs
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSWAF_20190729.ListWebACLs" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/wafv2/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Scope":"REGIONAL","Limit":100}'

# 3. List all IP sets
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AWSWAF_20190729.ListIPSets" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/wafv2/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Scope":"REGIONAL","Limit":100}'
```

## SDK Example

```typescript
import {
  WAFV2Client,
  CreateIPSetCommand,
  CreateWebACLCommand,
  GetWebACLCommand,
  ListWebACLsCommand,
} from '@aws-sdk/client-wafv2';

const waf = new WAFV2Client({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create an IP set for blocking
const { Summary: ipSetSummary } = await waf.send(new CreateIPSetCommand({
  Name: 'blocked-ips',
  Scope: 'REGIONAL',
  IPAddressVersion: 'IPV4',
  Addresses: ['192.0.2.0/24', '198.51.100.0/24'],
  Description: 'Known bad IP ranges',
}));

console.log('IP Set ID:', ipSetSummary?.Id);
console.log('IP Set ARN:', ipSetSummary?.ARN);

// Create a Web ACL
const { Summary: aclSummary } = await waf.send(new CreateWebACLCommand({
  Name: 'api-protection',
  Scope: 'REGIONAL',
  DefaultAction: { Allow: {} }, // allow by default
  Rules: [
    {
      Name: 'block-bad-actors',
      Priority: 1,
      Statement: {
        IPSetReferenceStatement: {
          ARN: ipSetSummary!.ARN!,
        },
      },
      Action: { Block: {} },
      VisibilityConfig: {
        SampledRequestsEnabled: true,
        CloudWatchMetricsEnabled: true,
        MetricName: 'block-bad-actors',
      },
    },
    {
      Name: 'rate-limit',
      Priority: 2,
      Statement: {
        RateBasedStatement: {
          Limit: 1000,
          AggregateKeyType: 'IP',
        },
      },
      Action: { Block: {} },
      VisibilityConfig: {
        SampledRequestsEnabled: true,
        CloudWatchMetricsEnabled: true,
        MetricName: 'rate-limit',
      },
    },
  ],
  VisibilityConfig: {
    SampledRequestsEnabled: true,
    CloudWatchMetricsEnabled: true,
    MetricName: 'api-protection',
  },
}));

console.log('Web ACL ARN:', aclSummary?.ARN);

// List all Web ACLs
const { WebACLs } = await waf.send(new ListWebACLsCommand({
  Scope: 'REGIONAL',
}));
console.log('Web ACLs:', WebACLs?.map(a => a.Name));
```

## Behavior Notes

- WAF in AWSim records Web ACLs, IP sets, and rule groups but does **not** actually filter HTTP traffic.
- Persistence is enabled: WAF resources survive AWSim restarts.
- The `Scope` parameter (`REGIONAL` or `CLOUDFRONT`) is recorded but both use the same underlying storage.
- `LockToken` is required for update and delete operations — retrieve it from `GetWebACL` or `GetIPSet` before modifying.
- WAF rule evaluation (blocking, allowing, counting requests) is not performed — use AWSim WAF for IaC testing and SDK integration verification.
- `UpdateIPSet` replaces the entire `Addresses` list — include all desired CIDRs, not just the new ones.
