# SES

Amazon Simple Email Service v2 for sending transactional and marketing emails.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `RestJson1` |
| Signing Name | `ses` |
| Persistence | No |

## Operations

### Emails
- `SendEmail` — send an email to one or more recipients

### Identities
- `CreateEmailIdentity` — register a domain or email address as a verified sender identity
- `GetEmailIdentity` — get details of a verified identity
- `ListEmailIdentities` — list all verified sender identities
- `DeleteEmailIdentity` — remove a verified identity

### Templates
- `CreateEmailTemplate` — create a reusable email template
- `GetEmailTemplate` — get a template by name
- `ListEmailTemplates` — list all email templates
- `DeleteEmailTemplate` — delete a template

### Account
- `GetAccount` — get account-level sending details and limits

## Example

```bash
# Create a verified identity
aws --endpoint-url http://localhost:4567 \
  sesv2 create-email-identity \
  --email-identity sender@example.com

# Send a simple email
aws --endpoint-url http://localhost:4567 \
  sesv2 send-email \
  --from-email-address sender@example.com \
  --destination '{"ToAddresses":["recipient@example.com"]}' \
  --content '{"Simple":{"Subject":{"Data":"Hello"},"Body":{"Text":{"Data":"Hello from AWSim!"}}}}'

# Create an email template
aws --endpoint-url http://localhost:4567 \
  sesv2 create-email-template \
  --template-name welcome \
  --template-content '{"Subject":"Welcome {{name}}!","Text":"Hello {{name}}, welcome aboard."}'
```

## Notes

- SES uses the REST/JSON v2 API (`/v2/email/...` paths), not the legacy form-encoded protocol.
- Emails are accepted and recorded internally but not actually delivered — no SMTP connection is made.
- Identity verification status is set to `SUCCESS` immediately without DNS verification.
- Template variable substitution is stored but not rendered during send in the current implementation.
