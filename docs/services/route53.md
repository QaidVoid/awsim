# Route 53

Amazon Route 53 DNS service for domain management, hosted zones, and DNS record sets.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `RestXml` |
| Signing Name | `route53` |
| Persistence | No |

## Operations

### Hosted Zones
- `CreateHostedZone` — create a new hosted zone for a domain
- `GetHostedZone` — get details of a hosted zone by ID
- `ListHostedZones` — list all hosted zones in the account
- `DeleteHostedZone` — delete a hosted zone
- `ListHostedZonesByName` — list hosted zones ordered by DNS name

### Record Sets
- `ChangeResourceRecordSets` — create, update, or delete DNS records in a hosted zone
- `ListResourceRecordSets` — list DNS records in a hosted zone

### Health Checks
- `CreateHealthCheck` — create an endpoint health check
- `ListHealthChecks` — list all health checks
- `DeleteHealthCheck` — delete a health check

### Tags
- `ChangeTagsForResource` — add or remove tags on a hosted zone or health check
- `ListTagsForResource` — list tags on a hosted zone or health check

## Example

```bash
# Create a hosted zone
aws --endpoint-url http://localhost:4567 \
  route53 create-hosted-zone \
  --name example.com \
  --caller-reference unique-ref-123

# Add an A record
aws --endpoint-url http://localhost:4567 \
  route53 change-resource-record-sets \
  --hosted-zone-id <zone-id> \
  --change-batch '{"Changes":[{"Action":"CREATE","ResourceRecordSet":{"Name":"api.example.com","Type":"A","TTL":300,"ResourceRecords":[{"Value":"1.2.3.4"}]}}]}'

# List records in a zone
aws --endpoint-url http://localhost:4567 \
  route53 list-resource-record-sets \
  --hosted-zone-id <zone-id>
```

## Notes

- Route 53 is a **global** service — state is shared across all regions under the same account.
- Route 53 uses the `RestXml` protocol (XML request/response bodies with REST routing).
- DNS records are stored in AWSim but no actual DNS resolution is performed.
- Health checks are recorded but not actively polled.
- Zone IDs are returned in the format `/hostedzone/{id}`.
