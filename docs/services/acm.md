# ACM

AWS Certificate Manager for provisioning and managing SSL/TLS certificates for AWS services.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `acm` |
| Target Prefix | `CertificateManager` |
| Persistence | Yes |

## Quick Start

Request a certificate, then retrieve its details and PEM:

```bash
# Request a certificate
CERT_ARN=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: CertificateManager.RequestCertificate" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/acm/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"DomainName":"example.com","SubjectAlternativeNames":["www.example.com","api.example.com"],"ValidationMethod":"DNS"}' \
  | jq -r '.CertificateArn')

echo "Certificate ARN: $CERT_ARN"

# Describe the certificate
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: CertificateManager.DescribeCertificate" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/acm/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"CertificateArn\":\"$CERT_ARN\"}"
```

## Operations

### Certificates
- `RequestCertificate` — request a new public SSL/TLS certificate
  - Input: `DomainName` (required), `SubjectAlternativeNames` (list of additional domains), `ValidationMethod` (`DNS` or `EMAIL`), `Tags`
  - Returns: `CertificateArn` in the format `arn:aws:acm:us-east-1:000000000000:certificate/{uuid}`
  - In AWSim the certificate is immediately issued — no DNS propagation wait

- `DescribeCertificate` — get full certificate details
  - Input: `CertificateArn`
  - Returns: `Certificate` object with `Status` (`ISSUED`), `DomainName`, `SubjectAlternativeNames`, `NotAfter`, `NotBefore`, `Issuer`, `KeyAlgorithm`

- `ListCertificates` — list certificates with optional status filter
  - Input: `CertificateStatuses` (optional list: `ISSUED`, `PENDING_VALIDATION`, etc.), `MaxItems`, `NextToken`
  - Returns: paginated `CertificateSummaryList`

- `DeleteCertificate` — permanently delete a certificate
  - Input: `CertificateArn`
  - Returns: empty response (HTTP 200)

- `GetCertificate` — retrieve the certificate PEM and chain
  - Input: `CertificateArn`
  - Returns: `Certificate` (PEM string) and `CertificateChain` (PEM string of the CA chain)

- `ExportCertificate` — export a private certificate with its key
  - Input: `CertificateArn`, `Passphrase` (base64-encoded, used to encrypt the private key)
  - Returns: `Certificate`, `CertificateChain`, `PrivateKey` all as PEM strings

### Tags
- `AddTagsToCertificate` — add key-value tags to a certificate
  - Input: `CertificateArn`, `Tags` (list of `{Key, Value}`)

- `RemoveTagsFromCertificate` — remove specific tags
  - Input: `CertificateArn`, `Tags`

- `ListTagsForCertificate` — list all tags on a certificate
  - Input: `CertificateArn`
  - Returns: `Tags` list

## Curl Examples

```bash
# 1. Request a certificate
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: CertificateManager.RequestCertificate" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/acm/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"DomainName":"myapp.example.com","ValidationMethod":"DNS","Tags":[{"Key":"env","Value":"prod"}]}'

# 2. List all certificates
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: CertificateManager.ListCertificates" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/acm/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{}'

# 3. Get certificate PEM (useful for attaching to load balancers or CloudFront)
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: CertificateManager.GetCertificate" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/acm/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"CertificateArn":"arn:aws:acm:us-east-1:000000000000:certificate/YOUR_CERT_ID"}'
```

## SDK Example

```typescript
import {
  ACMClient,
  RequestCertificateCommand,
  DescribeCertificateCommand,
  ListCertificatesCommand,
} from '@aws-sdk/client-acm';

const acm = new ACMClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Request a certificate
const { CertificateArn } = await acm.send(new RequestCertificateCommand({
  DomainName: 'myapp.example.com',
  SubjectAlternativeNames: ['www.myapp.example.com', 'api.myapp.example.com'],
  ValidationMethod: 'DNS',
  Tags: [{ Key: 'environment', Value: 'staging' }],
}));

console.log('Certificate ARN:', CertificateArn);

// Describe the certificate
const { Certificate } = await acm.send(new DescribeCertificateCommand({
  CertificateArn,
}));

console.log('Status:', Certificate?.Status); // ISSUED
console.log('Domain:', Certificate?.DomainName);

// List all issued certificates
const { CertificateSummaryList } = await acm.send(new ListCertificatesCommand({
  CertificateStatuses: ['ISSUED'],
}));

console.log('Total certs:', CertificateSummaryList?.length);
```

## Behavior Notes

- Certificates are issued with status `ISSUED` immediately — no DNS or email validation is performed.
- `GetCertificate` returns a locally generated self-signed certificate PEM, not a real CA-signed cert.
- `ExportCertificate` returns a certificate, chain, and passphrase-encrypted private key in PEM format.
- Persistence is enabled: certificates survive AWSim restarts.
- Certificate ARNs follow the real AWS format: `arn:aws:acm:{region}:{account}:certificate/{uuid}`.
