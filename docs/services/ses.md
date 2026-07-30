# SES

Amazon Simple Email Service for sending transactional and marketing emails.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `RestJson1` (v2), `AwsQuery` (classic) |
| Signing Name | `ses` |
| API Version | v2 and classic (v1) |
| Persistence | Yes (sent mail lives in SQLite) |

SES has two APIs and AWSim serves both from the same account state, so an
identity verified through one is visible to the other.

- **v2** uses REST-style routing with JSON bodies under `/v2/email/...`.
  This is what `@aws-sdk/client-sesv2` and `aws sesv2` speak.
- **Classic (v1)** is the original form-encoded query API at `POST /`.
  This is what `aws ses` speaks, along with `@aws-sdk/client-ses` and
  boto3's `ses` client.

Which one you get is decided by the request: a form body carrying
`Action=` is answered as XML, everything else as JSON.

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
- `SendEmail`: send an email to one or more recipients
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
- `CreateEmailIdentity`: register a domain or email address as a verified sender identity
  - Path: `POST /v2/email/identities`
  - Input: `EmailIdentity` (email address or domain name), optional `Tags`
  - Returns: `IdentityType` (`EMAIL_ADDRESS` or `DOMAIN`), `VerifiedForSendingStatus` (`true` in AWSim), `DkimAttributes`

- `GetEmailIdentity`: get details of a verified identity
  - Path: `GET /v2/email/identities/{EmailIdentity}`
  - Returns: `IdentityType`, `VerifiedForSendingStatus`, `DkimAttributes`, `Tags`

- `ListEmailIdentities`: list all verified sender identities
  - Path: `GET /v2/email/identities`
  - Returns: paginated `EmailIdentities` list with `IdentityName`, `IdentityType`, `SendingEnabled`

- `DeleteEmailIdentity`: remove a verified identity
  - Path: `DELETE /v2/email/identities/{EmailIdentity}`

### Templates
- `CreateEmailTemplate`: create a reusable email template with variable substitution
  - Path: `POST /v2/email/templates`
  - Input: `TemplateName`, `TemplateContent` with `Subject`, `Text`, `Html` (use `{{VariableName}}` for substitutions)

- `GetEmailTemplate`: get a template by name
  - Path: `GET /v2/email/templates/{TemplateName}`

- `ListEmailTemplates`: list all email templates
  - Path: `GET /v2/email/templates`

- `DeleteEmailTemplate`: delete a template
  - Path: `DELETE /v2/email/templates/{TemplateName}`

### Account
- `GetAccount`: get account-level sending details and limits
  - Path: `GET /v2/email/account`
  - Returns: `SendingEnabled: true`, `SendQuota` (`Max24HourSend`, `MaxSendRate`, `SentLast24Hours`), `ProductionAccessEnabled`

## Classic (v1) API

Everything above has a classic equivalent, plus the operations that only
exist on the older API. All of it is `POST /` with a form-encoded body,
which is what `aws ses` sends.

```bash
# Verify a sender, then send through it
aws --endpoint-url http://localhost:4566 ses verify-email-identity \
  --email-address dev@example.com
aws --endpoint-url http://localhost:4566 ses send-email \
  --from dev@example.com \
  --destination ToAddresses=alice@example.com \
  --message 'Subject={Data=Hello},Body={Text={Data=Hi there}}'
```

| Group | Calls |
|-------|-------|
| Identities | `VerifyEmailIdentity`, `VerifyDomainIdentity`, `VerifyEmailAddress`, `ListIdentities`, `ListVerifiedEmailAddresses`, `DeleteIdentity`, `DeleteVerifiedEmailAddress`, `GetIdentityVerificationAttributes` |
| Notifications | `SetIdentityNotificationTopic`, `GetIdentityNotificationAttributes`, `SetIdentityFeedbackForwardingEnabled`, `SetIdentityHeadersInNotificationsEnabled` |
| MAIL FROM | `SetIdentityMailFromDomain`, `GetIdentityMailFromDomainAttributes` |
| Identity policies | `PutIdentityPolicy`, `GetIdentityPolicies`, `ListIdentityPolicies`, `DeleteIdentityPolicy` |
| Templates | `CreateTemplate`, `GetTemplate`, `UpdateTemplate`, `DeleteTemplate`, `ListTemplates`, `TestRenderTemplate` |
| Sending | `SendEmail`, `SendTemplatedEmail`, `SendRawEmail`, `SendBulkTemplatedEmail` |
| Account | `GetSendQuota`, `GetSendStatistics`, `GetAccountSendingEnabled`, `UpdateAccountSendingEnabled` |
| Configuration sets | `DescribeConfigurationSet`, `ListConfigurationSets`, `UpdateConfigurationSetSendingEnabled`, `UpdateConfigurationSetReputationMetricsEnabled`, `UpdateConfigurationSetEventDestination`, tracking options |
| Receiving | `CreateReceiptFilter`, `DeleteReceiptFilter`, `ListReceiptFilters`, `CloneReceiptRuleSet`, `SetReceiptRulePosition`, plus the rule and rule-set calls |

`SendBounce` is the one classic operation AWSim does not implement.

### Where the two APIs disagree

A handful of fields are spelled differently on each API. AWSim accepts
both spellings on input, so the same handler serves either client:

| Concept | Classic | v2 |
|---------|---------|-----|
| Sender | `Source` | `FromEmailAddress` |
| Body | `Message.Subject` / `Message.Body` | `Content.Simple` |
| Message tags | `Tags` | `EmailTags` |
| Event types | `send`, `renderingFailure` | `SEND`, `RENDERING_FAILURE` |
| SNS event target | `SNSDestination.TopicARN` | `SnsDestination.TopicArn` |

`ListConfigurationSets` is the one response the two cannot share: the
classic API returns objects with a `Name`, v2 returns bare strings.
AWSim picks by the path the request arrived on.

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

## Outbox

Awsim captures every outbound email into a SQLite store, covering `SendEmail`, `SendBulkEmail` and `SendCustomVerificationEmail`, so you can inspect what was actually sent without parsing the SDK call.

**UI:** open `/ses` and switch to the **Outbox** tab (default). Lists every captured message newest-first, with a search box that filters by subject / from / recipient. Click a row to expand the body. It picks the best view automatically, preferring Text, then HTML in a sandboxed iframe, then Raw. The dialog shows message ID, full To / Cc / Bcc, account, region, and timestamp.

**Admin endpoint:**

```bash
# All captured emails, newest first
curl http://localhost:4566/_awsim/ses/sent | jq .

# Scope to one account / region
curl 'http://localhost:4566/_awsim/ses/sent?account=000000000000&region=us-east-1'
```

Returns `{ count, emails: [...] }`; each email has `messageId`, `from`, `to`, `cc`, `bcc`, `subject`, `bodyText`, `bodyHtml`, `raw`, `sentAt`, `account`, `region`.

### Persistence + retention

- With `--data-dir`, the outbox lives in `{data-dir}/ses.db` and survives restarts. Without it, an ephemeral tempdir holds the DB and is cleaned up on shutdown.
- An hourly background sweep deletes emails older than `--ses-retention-hours` (default 720h / 30 days). Set to `0` to disable.

## Behavior Notes

- Emails are accepted, recorded into the [Outbox](#outbox), and **not actually delivered**. No SMTP connection is made.
- Identity verification succeeds immediately, with no DNS record to publish and no confirmation link to click. A domain still gets a verification token, and the same domain always gets the same one so re-running provisioning does not churn your DNS fixtures.
- Template variables (`{{variable}}`) are rendered at send time, and `TestRenderTemplate` renders one without sending. A variable the data omits collapses to an empty string, which is what AWS does.
- `MessageId` is returned as a UUID for each sent email.
- `GetAccount` reports the quota of a production account rather than the sandbox, so a local run does not trip a limit that only exists to gate real outbound mail. `UpdateAccountSendingEnabled` still turns sending off if you want to exercise that path.
- Outbound emails persist in SQLite (see [Outbox](#outbox)). Configuration persists through the JSON snapshot: identities and their notification, MAIL FROM and policy settings, templates, configuration sets, receipt rule sets, and receipt filters.
