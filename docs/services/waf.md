# WAF

AWS WAF v2 (Web Application Firewall) for protecting web applications from common exploits using rules and rule groups.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `wafv2` |
| Persistence | Yes |

## Operations

### Web ACLs
- `CreateWebACL` — create a Web ACL with rules and a default action
- `GetWebACL` — get a Web ACL by name and scope
- `ListWebACLs` — list Web ACLs for a given scope (REGIONAL or CLOUDFRONT)
- `DeleteWebACL` — delete a Web ACL
- `UpdateWebACL` — update Web ACL rules or default action

### IP Sets
- `CreateIPSet` — create an IP set containing CIDR ranges for rule conditions
- `GetIPSet` — get an IP set by name and scope
- `ListIPSets` — list IP sets for a given scope
- `DeleteIPSet` — delete an IP set

### Rule Groups
- `CreateRuleGroup` — create a reusable rule group
- `ListRuleGroups` — list rule groups for a given scope
- `DeleteRuleGroup` — delete a rule group

## Example

```bash
# Create a Web ACL (REGIONAL scope)
aws --endpoint-url http://localhost:4567 \
  wafv2 create-web-acl \
  --name my-web-acl \
  --scope REGIONAL \
  --default-action Allow={} \
  --visibility-config SampledRequestsEnabled=true,CloudWatchMetricsEnabled=true,MetricName=my-web-acl

# Create an IP set (blocklist)
aws --endpoint-url http://localhost:4567 \
  wafv2 create-ip-set \
  --name bad-ips \
  --scope REGIONAL \
  --ip-address-version IPV4 \
  --addresses 192.168.1.0/24 10.0.0.0/8

# List Web ACLs
aws --endpoint-url http://localhost:4567 \
  wafv2 list-web-acls \
  --scope REGIONAL
```

## Notes

- WAF in AWSim records Web ACLs, IP sets, and rule groups but does not actually filter HTTP traffic.
- Persistence is enabled: WAF resources survive AWSim restarts.
- The `scope` parameter (`REGIONAL` or `CLOUDFRONT`) is recorded but both use the same underlying store.
- WAF rule evaluation (blocking, allowing, counting) is not performed — use AWSim WAF for SDK and IaC testing.
