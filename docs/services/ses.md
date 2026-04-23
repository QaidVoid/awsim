# SES

Amazon Simple Email Service v2 for sending transactional and marketing emails.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `RestJson1` |
| Signing Name | `ses` |
| API Version | v2 |
| Persistence | No |

SES v2 uses REST-style routing with JSON bodies. All paths are under `/v2/email/...`.

## Quick Start

Verify a sender identity and send an email:

```bash
# Create (verify) a sender identity
curl -s -X POST http://localhost:4566/v2/email/identities \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ses/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"EmailIdentity":"sender@example.com","Tags":[]}'

# Send an email
curl -s -X POST http://localhost:4566/v2/email/outbound-emails \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ses/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{
    "FromEmailAddress": "sender@example.com",
    "Destination": {"ToAddresses": ["recipient@example.com"], "CcAddresses": [], "BccAddresses": []},
    "Content": {
      "Simple": {
        "Subject": {"Data": "Hello from AWSim!"},
        "Body": {
          "Text": {"Data": "This is a plain text email."},
          "Html": {"Data": "<h1>Hello!</h1><p>This is an HTML email.</p>"}
        }
      }
    }
  }'
```

## Operations

### Emails
- `SendEmail` — send an email to one or more recipients
  - Path: `POST /v2/email/outbound-emails`
  - Input:
    - `FromEmailAddress` (required, must be a verified identity)
    - `Destination`: `{ToAddresses, CcAddresses, BccAddresses}` (lists of email addresses)
    - `Content`: one of:
      - `Simple`: `{Subject: {Data}, Body: {Text: {Data}, Html: {Data}}}`
      - `Template`: `{TemplateName, TemplateData}` (JSON string with substitution variables)
      - `Raw`: `{Data}` (base64-encoded raw MIME message)
    - Optional: `ReplyToAddresses`, `FeedbackForwardingEmailAddress`, `EmailTags` (list of `{Name, Value}`)
  - Returns: `MessageId`

### Identities
- `CreateEmailIdentity` — register a domain or email address as a verified sender identity
  - Path: `POST /v2/email/identities`
  - Input: `EmailIdentity` (email address or domain name), optional `Tags`
  - Returns: `IdentityType` (`EMAIL_ADDRESS` or `DOMAIN`), `VerifiedForSendingStatus` (`true` in AWSim), `DkimAttributes`

- `GetEmailIdentity` — get details of a verified identity
  - Path: `GET /v2/email/identities/{EmailIdentity}`
  - Returns: `IdentityType`, `VerifiedForSendingStatus`, `DkimAttributes`, `Tags`

- `ListEmailIdentities` — list all verified sender identities
  - Path: `GET /v2/email/identities`
  - Returns: paginated `EmailIdentities` list with `IdentityName`, `IdentityType`, `SendingEnabled`

- `DeleteEmailIdentity` — remove a verified identity
  - Path: `DELETE /v2/email/identities/{EmailIdentity}`

### Templates
- `CreateEmailTemplate` — create a reusable email template with variable substitution
  - Path: `POST /v2/email/templates`
  - Input: `TemplateName`, `TemplateContent` with `Subject`, `Text`, `Html` (use `{{VariableName}}` for substitutions)

- `GetEmailTemplate` — get a template by name
  - Path: `GET /v2/email/templates/{TemplateName}`

- `ListEmailTemplates` — list all email templates
  - Path: `GET /v2/email/templates`

- `DeleteEmailTemplate` — delete a template
  - Path: `DELETE /v2/email/templates/{TemplateName}`

### Account
- `GetAccount` — get account-level sending details and limits
  - Path: `GET /v2/email/account`
  - Returns: `SendingEnabled: true`, `SendQuota` (`Max24HourSend`, `MaxSendRate`, `SentLast24Hours`), `ProductionAccessEnabled`

## Curl Examples

```bash
# 1. Verify a domain identity
curl -s -X POST http://localhost:4566/v2/email/identities \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ses/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"EmailIdentity":"example.com"}'

# 2. List all verified identities
curl -s http://localhost:4566/v2/email/identities \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ses/aws4_request, SignedHeaders=host, Signature=fake"

# 3. Create an email template
curl -s -X POST http://localhost:4566/v2/email/templates \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ses/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{
    "TemplateName": "welcome-email",
    "TemplateContent": {
      "Subject": "Welcome, {{name}}!",
      "Text": "Hi {{name}}, welcome to {{company}}. Your account is ready.",
      "Html": "<h1>Welcome, {{name}}!</h1><p>Hi {{name}}, welcome to <strong>{{company}}</strong>.</p>"
    }
  }'

# 4. Send using a template
curl -s -X POST http://localhost:4566/v2/email/outbound-emails \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ses/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{
    "FromEmailAddress": "no-reply@example.com",
    "Destination": {"ToAddresses": ["alice@example.com"]},
    "Content": {
      "Template": {
        "TemplateName": "welcome-email",
        "TemplateData": "{\"name\":\"Alice\",\"company\":\"Acme Corp\"}"
      }
    }
  }'
```

## SDK Example

```typescript
import {
  SESv2Client,
  CreateEmailIdentityCommand,
  SendEmailCommand,
  CreateEmailTemplateCommand,
  GetAccountCommand,
} from '@aws-sdk/client-sesv2';

const ses = new SESv2Client({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Verify sender identity
await ses.send(new CreateEmailIdentityCommand({
  EmailIdentity: 'sender@example.com',
}));

// Send a simple email
const { MessageId } = await ses.send(new SendEmailCommand({
  FromEmailAddress: 'sender@example.com',
  Destination: {
    ToAddresses: ['recipient@example.com'],
    CcAddresses: ['cc@example.com'],
  },
  Content: {
    Simple: {
      Subject: { Data: 'Order Confirmation #12345', Charset: 'UTF-8' },
      Body: {
        Text: { Data: 'Your order has been confirmed. Thank you for shopping!', Charset: 'UTF-8' },
        Html: {
          Data: '<h2>Order Confirmed</h2><p>Your order #12345 has been confirmed. Thank you!</p>',
          Charset: 'UTF-8',
        },
      },
    },
  },
  EmailTags: [
    { Name: 'category', Value: 'transactional' },
    { Name: 'order_id', Value: '12345' },
  ],
}));

console.log('Message ID:', MessageId);

// Create a template for repeated use
await ses.send(new CreateEmailTemplateCommand({
  TemplateName: 'password-reset',
  TemplateContent: {
    Subject: 'Reset your password',
    Text: 'Click this link to reset your password: {{resetLink}}',
    Html: '<p>Click <a href="{{resetLink}}">here</a> to reset your password. Link expires in {{expiryMinutes}} minutes.</p>',
  },
}));

// Check account sending quotas
const account = await ses.send(new GetAccountCommand({}));
console.log('Daily sending limit:', account.SendQuota?.Max24HourSend);
console.log('Sent in last 24h:', account.SendQuota?.SentLast24Hours);
```

## Behavior Notes

- SES uses the REST/JSON v2 API (`/v2/email/...` paths), not the legacy form-encoded `ses` protocol.
- Emails are accepted and recorded internally but **not actually delivered** — no SMTP connection is made.
- Identity verification status is set to `SUCCESS` immediately without DNS verification or email confirmation.
- Template variable substitution (`{{variable}}`) is stored but **not rendered** during send in the current implementation.
- `MessageId` is returned as a UUID for each sent email.
- `GetAccount` always reports `SendingEnabled: true` and generous quota limits.
- State is in-memory only and lost on restart.
