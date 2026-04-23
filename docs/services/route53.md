# Route 53

Amazon Route 53 DNS service for domain management, hosted zones, and DNS record sets.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `RestXml` |
| Signing Name | `route53` |
| API Version | `2013-04-01` |
| Persistence | No |

Route 53 uses the `RestXml` protocol: XML request/response bodies with REST routing. It is a **global** service — state is shared across all regions. Zone IDs are returned in the format `/hostedzone/{id}`.

## Quick Start

Create a hosted zone and add an A record:

```bash
# Create a hosted zone
ZONE_ID=$(aws --endpoint-url http://localhost:4566 \
  route53 create-hosted-zone \
  --name example.com \
  --caller-reference ref-$(date +%s) \
  | jq -r '.HostedZone.Id' | sed 's|/hostedzone/||')

echo "Zone ID: $ZONE_ID"

# Add an A record
aws --endpoint-url http://localhost:4566 \
  route53 change-resource-record-sets \
  --hosted-zone-id $ZONE_ID \
  --change-batch '{
    "Changes": [{
      "Action": "CREATE",
      "ResourceRecordSet": {
        "Name": "api.example.com",
        "Type": "A",
        "TTL": 300,
        "ResourceRecords": [{"Value": "1.2.3.4"}]
      }
    }]
  }'

# List records
aws --endpoint-url http://localhost:4566 \
  route53 list-resource-record-sets \
  --hosted-zone-id $ZONE_ID
```

## Operations

### Hosted Zones
- `CreateHostedZone` — create a new hosted zone for a domain
  - Input: `Name` (domain name, e.g., `example.com`), `CallerReference` (unique string to prevent duplicate creation), optional `Comment`, `PrivateZone` (boolean), `VPC` (for private zones)
  - Returns: `HostedZone` with `Id` (`/hostedzone/{id}`), `Name`, `Config`, `CallerReference`, and `DelegationSet` (nameservers)

- `GetHostedZone` — get details of a hosted zone
  - Path: `GET /2013-04-01/hostedzone/{Id}`
  - Returns: `HostedZone`, `DelegationSet` (NS records), `VPCs` (if private)

- `ListHostedZones` — list all hosted zones in the account
  - Input: optional `Marker`, `MaxItems`
  - Returns: paginated `HostedZones` list

- `DeleteHostedZone` — delete a hosted zone (must have no records except SOA and NS)
  - Path: `DELETE /2013-04-01/hostedzone/{Id}`

- `ListHostedZonesByName` — list hosted zones ordered by DNS name
  - Input: optional `DNSName`, `HostedZoneId`, `MaxItems`
  - Returns: alphabetically ordered `HostedZones` list

### Record Sets
- `ChangeResourceRecordSets` — create, update, or delete DNS records in a hosted zone
  - Input: `HostedZoneId`, `ChangeBatch` (`{Comment, Changes: [{Action: "CREATE"/"UPSERT"/"DELETE", ResourceRecordSet: {...}}]}`)
  - `ResourceRecordSet` fields: `Name`, `Type` (`A`, `AAAA`, `CNAME`, `MX`, `TXT`, `NS`, `SOA`, `SRV`, `PTR`, `CAA`), `TTL`, `ResourceRecords` (list of `{Value}`), or `AliasTarget` (for alias records)
  - Returns: `ChangeInfo` with `Id`, `Status` (`PENDING` → `INSYNC`), `SubmittedAt`

- `ListResourceRecordSets` — list DNS records in a hosted zone
  - Input: `HostedZoneId`, optional `StartRecordName`, `StartRecordType`, `MaxItems`
  - Returns: paginated `ResourceRecordSets` list

- `GetHostedZoneCount` — return the total number of hosted zones in the account
  - Path: `GET /2013-04-01/hostedzonecount`
  - Returns: `HostedZoneCount` (integer)

- `ListHostedZonesByVPC` — list private hosted zones associated with a VPC (stub returning empty list)
  - Path: `GET /2013-04-01/hostedzonesbyvpc`

### DNSSEC
- `GetDNSSEC` — get DNSSEC status for a hosted zone (stub returns disabled)
  - Path: `GET /2013-04-01/hostedzone/{Id}/dnssec`
  - Returns: `Status.ServeSignature: "NOT_SIGNING"`, empty `KeySigningKeys`

### DNS Testing
- `TestDNSAnswer` — test how Route 53 would answer a DNS query (stub returns mock answer)
  - Path: `GET /2013-04-01/testdnsanswer`
  - Input query params: `RecordName`, `RecordType`, `HostedZoneId`
  - Returns: `Nameserver`, `RecordData`, `ResponseCode: "NOERROR"`, `Protocol: "UDP"`

### Checker IP Ranges
- `GetCheckerIpRanges` — return the IP ranges that Route 53 health checkers use
  - Path: `GET /2013-04-01/checkeripranges`
  - Returns: `CheckerIpRanges` (list of CIDR blocks)

### Health Checks
- `CreateHealthCheck` — create an endpoint health check
  - Input: `CallerReference`, `HealthCheckConfig` (`{IPAddress, Type: "HTTP"/"HTTPS"/"TCP", Port, ResourcePath}`)
  - Returns: `HealthCheck` with `Id`, `CallerReference`, `HealthCheckConfig`

- `ListHealthChecks` — list all health checks
  - Returns: paginated `HealthChecks` list

- `DeleteHealthCheck` — delete a health check
  - Input: `HealthCheckId`

- `GetHealthCheckCount` — return the total number of health checks
  - Path: `GET /2013-04-01/healthcheckcount`
  - Returns: `HealthCheckCount` (integer)

- `GetHealthCheckStatus` — return the status of a health check from all checkers (always healthy)
  - Path: `GET /2013-04-01/healthcheck/{Id}/status`
  - Returns: `HealthCheckObservations` list with per-region status

- `UpdateHealthCheck` — update health check configuration
  - Path: `POST /2013-04-01/healthcheck/{Id}`
  - Input: `HealthCheckConfig` fields to update (merged into existing config)
  - Returns: updated `HealthCheck`

### Query Logging
- `CreateQueryLoggingConfig` — configure DNS query logging to a CloudWatch Logs group
  - Path: `POST /2013-04-01/queryloggingconfig`
  - Input: `HostedZoneId`, `CloudWatchLogsLogGroupArn`
  - Returns: `QueryLoggingConfig` with `Id`

- `DeleteQueryLoggingConfig` — delete a query logging configuration
  - Path: `DELETE /2013-04-01/queryloggingconfig/{Id}`

- `ListQueryLoggingConfigs` — list query logging configurations
  - Path: `GET /2013-04-01/queryloggingconfig`
  - Input: optional `HostedZoneId` filter

### Tags
- `ChangeTagsForResource` — add or remove tags on a hosted zone or health check
  - Input: `ResourceType` (`hostedzone` or `healthcheck`), `ResourceId`, `AddTags`, `RemoveTagKeys`

- `ListTagsForResource` — list tags on a resource
  - Input: `ResourceType`, `ResourceId`

## Curl Examples

```bash
# 1. Create a hosted zone via curl (RestXml)
curl -s -X POST http://localhost:4566/2013-04-01/hostedzone \
  -H "Content-Type: application/xml" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/route53/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '<?xml version="1.0" encoding="UTF-8"?>
<CreateHostedZoneRequest>
  <Name>myapp.internal</Name>
  <CallerReference>ref-001</CallerReference>
  <HostedZoneConfig>
    <Comment>Internal DNS</Comment>
    <PrivateZone>false</PrivateZone>
  </HostedZoneConfig>
</CreateHostedZoneRequest>'

# 2. List all hosted zones
curl -s http://localhost:4566/2013-04-01/hostedzone \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/route53/aws4_request, SignedHeaders=host, Signature=fake"

# 3. Create a CNAME record
aws --endpoint-url http://localhost:4566 route53 change-resource-record-sets \
  --hosted-zone-id YOUR_ZONE_ID \
  --change-batch '{
    "Changes": [
      {"Action":"CREATE","ResourceRecordSet":{"Name":"www.example.com","Type":"CNAME","TTL":300,"ResourceRecords":[{"Value":"myapp.example.com"}]}},
      {"Action":"CREATE","ResourceRecordSet":{"Name":"api.example.com","Type":"A","TTL":60,"ResourceRecords":[{"Value":"10.0.1.100"}]}}
    ]
  }'
```

## SDK Example

```typescript
import {
  Route53Client,
  CreateHostedZoneCommand,
  ChangeResourceRecordSetsCommand,
  ListResourceRecordSetsCommand,
} from '@aws-sdk/client-route-53';

const r53 = new Route53Client({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create hosted zone
const { HostedZone } = await r53.send(new CreateHostedZoneCommand({
  Name: 'example.com',
  CallerReference: `ref-${Date.now()}`,
}));

const hostedZoneId = HostedZone!.Id!.replace('/hostedzone/', '');
console.log('Zone ID:', hostedZoneId);

// Add multiple DNS records
await r53.send(new ChangeResourceRecordSetsCommand({
  HostedZoneId: hostedZoneId,
  ChangeBatch: {
    Changes: [
      {
        Action: 'CREATE',
        ResourceRecordSet: {
          Name: 'api.example.com',
          Type: 'A',
          TTL: 300,
          ResourceRecords: [{ Value: '1.2.3.4' }],
        },
      },
      {
        Action: 'CREATE',
        ResourceRecordSet: {
          Name: 'www.example.com',
          Type: 'CNAME',
          TTL: 3600,
          ResourceRecords: [{ Value: 'api.example.com' }],
        },
      },
      {
        Action: 'CREATE',
        ResourceRecordSet: {
          Name: 'example.com',
          Type: 'MX',
          TTL: 3600,
          ResourceRecords: [{ Value: '10 mail.example.com' }],
        },
      },
    ],
  },
}));

// List all records
const { ResourceRecordSets } = await r53.send(new ListResourceRecordSetsCommand({
  HostedZoneId: hostedZoneId,
}));

ResourceRecordSets?.forEach(rrs => {
  console.log(`${rrs.Name} ${rrs.Type} ${rrs.TTL}s`);
  rrs.ResourceRecords?.forEach(rr => console.log(`  -> ${rr.Value}`));
});
```

## Behavior Notes

- Route 53 is a **global** service — state is shared across all regions under the same account.
- DNS records are stored in AWSim but no actual DNS resolution is performed — you cannot resolve these names in a real browser or `dig` command.
- Health checks are recorded but not actively polled; health status is always reported as healthy.
- `ChangeResourceRecordSets` returns `Status: INSYNC` immediately (real Route 53 may take up to 60 seconds).
- Zone IDs are returned in the full format `/hostedzone/{id}` in most responses; strip the prefix when using as a URL path parameter.
- State is in-memory only and lost on restart.
