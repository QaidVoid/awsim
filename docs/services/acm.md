# ACM

AWS Certificate Manager for provisioning and managing SSL/TLS certificates for AWS services.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `acm` |
| Persistence | Yes |

## Operations

### Certificates
- `RequestCertificate` — request a new public SSL/TLS certificate for a domain
- `DescribeCertificate` — get full certificate details including validation status and domain names
- `ListCertificates` — list certificates with optional status filter
- `DeleteCertificate` — delete a certificate
- `GetCertificate` — retrieve the certificate PEM and chain
- `ExportCertificate` — export a certificate with private key (for private CAs)

### Tags
- `AddTagsToCertificate` — add tags to a certificate
- `RemoveTagsFromCertificate` — remove tags from a certificate
- `ListTagsForCertificate` — list tags on a certificate

## Example

```bash
# Request a certificate for a domain
aws --endpoint-url http://localhost:4567 \
  acm request-certificate \
  --domain-name example.com \
  --subject-alternative-names www.example.com api.example.com \
  --validation-method DNS

# Describe the certificate
aws --endpoint-url http://localhost:4567 \
  acm describe-certificate \
  --certificate-arn <arn>

# List all certificates
aws --endpoint-url http://localhost:4567 \
  acm list-certificates

# Get certificate PEM
aws --endpoint-url http://localhost:4567 \
  acm get-certificate \
  --certificate-arn <arn>
```

## Notes

- ACM certificates in AWSim are immediately issued with status `ISSUED` — no DNS or email validation is performed.
- `GetCertificate` returns a self-signed certificate PEM generated locally.
- `ExportCertificate` returns a certificate, chain, and private key in PEM format.
- Persistence is enabled: certificates survive AWSim restarts.
