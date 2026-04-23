# SNS

Amazon Simple Notification Service for pub/sub messaging, fan-out to queues and Lambda, and mobile push notifications.

**Protocol:** AwsJson1_0 (`X-Amz-Target: AmazonSimpleNotificationService.*`)
**Signing name:** `sns`
**Persistent:** Yes

## Quick Start

Create a topic, subscribe an SQS queue, and publish a message:

```bash
# Create a topic
TOPIC_ARN=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.0" \
  -H "X-Amz-Target: AmazonSimpleNotificationService.CreateTopic" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/sns/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"my-topic"}' \
  | jq -r '.TopicArn')

echo "Topic ARN: $TOPIC_ARN"

# Subscribe an SQS queue to the topic
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.0" \
  -H "X-Amz-Target: AmazonSimpleNotificationService.Subscribe" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/sns/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"TopicArn\":\"$TOPIC_ARN\",\"Protocol\":\"sqs\",\"Endpoint\":\"arn:aws:sqs:us-east-1:000000000000:my-queue\"}"

# Publish a message
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.0" \
  -H "X-Amz-Target: AmazonSimpleNotificationService.Publish" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/sns/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"TopicArn\":\"$TOPIC_ARN\",\"Message\":\"Hello subscribers!\",\"Subject\":\"Test message\"}"
```

## Operations

### Topics

| Operation | Description |
|-----------|-------------|
| `CreateTopic` | Create a topic (standard or FIFO). Input: `Name`, optional `Attributes` (`{FifoTopic: "true", ContentBasedDeduplication: "true"}`), `Tags`. Returns: `TopicArn` |
| `DeleteTopic` | Delete a topic and all its subscriptions. Input: `TopicArn` |
| `ListTopics` | List all topics with pagination. Returns: `Topics` list with `TopicArn` |
| `GetTopicAttributes` | Get topic configuration. Input: `TopicArn`. Returns: map of attribute names to values (policy, subscriptions count, etc.) |
| `SetTopicAttributes` | Set a specific topic attribute. Input: `TopicArn`, `AttributeName`, `AttributeValue` |

### Subscriptions

| Operation | Description |
|-----------|-------------|
| `Subscribe` | Subscribe an endpoint to a topic. Input: `TopicArn`, `Protocol` (see below), `Endpoint` (ARN or URL), optional `Attributes` (filter policy, raw message delivery). Returns: `SubscriptionArn` |
| `Unsubscribe` | Remove a subscription. Input: `SubscriptionArn` |
| `ListSubscriptions` | List all subscriptions across all topics. Returns paginated `Subscriptions` |
| `ListSubscriptionsByTopic` | List subscriptions for a specific topic. Input: `TopicArn` |
| `GetSubscriptionAttributes` | Get subscription attributes. Input: `SubscriptionArn` |
| `SetSubscriptionAttributes` | Update subscription attributes (filter policy, raw delivery). Input: `SubscriptionArn`, `AttributeName`, `AttributeValue` |
| `ConfirmSubscription` | Confirm a pending subscription (used for HTTP/HTTPS). In AWSim subscriptions are auto-confirmed |

### Publishing

| Operation | Description |
|-----------|-------------|
| `Publish` | Publish a message to a topic. Input: `TopicArn`, `Message` (required), `Subject`, `MessageAttributes` (`{key: {DataType, StringValue}}`), `MessageStructure` (`json` for per-protocol messages). Returns: `MessageId` |
| `PublishBatch` | Publish up to 10 messages in one call. Input: `TopicArn`, `PublishBatchRequestEntries` (list of `{Id, Message, Subject}`). Returns: `Successful`, `Failed` |

### Tags

| Operation | Description |
|-----------|-------------|
| `TagResource` | Add tags to a topic (by ARN) |
| `UntagResource` | Remove tags from a topic |
| `ListTagsForResource` | List topic tags |

### SMS

| Operation | Description |
|-----------|-------------|
| `CheckIfPhoneNumberIsOptedOut` | Check whether a phone number is opted out of receiving SMS messages. Input: `phoneNumber`. Returns: `isOptedOut` (boolean) |
| `ListPhoneNumbersOptedOut` | List phone numbers that are opted out of SMS. Returns paginated `phoneNumbers` list |
| `GetSMSAttributes` | Get account-level SMS attributes (default sender ID, monthly spend limit, etc.). Returns: `attributes` map |
| `SetSMSAttributes` | Set account-level SMS attributes. Input: `attributes` map |

## Supported Subscription Protocols

| Protocol | Description |
|----------|-------------|
| `sqs` | Delivers to an SQS queue (by queue ARN). AWSim immediately fans out to the queue |
| `lambda` | Invokes a Lambda function (by function ARN). AWSim calls the Lambda synchronously |
| `http` / `https` | Delivers to an HTTP endpoint. Not enforced in local mode (no HTTP call is made) |
| `email` | Sends email (no email is delivered in AWSim) |
| `email-json` | Sends JSON-formatted email |

## SDK Example

```typescript
import {
  SNSClient,
  CreateTopicCommand,
  SubscribeCommand,
  PublishCommand,
  PublishBatchCommand,
  SetSubscriptionAttributesCommand,
} from '@aws-sdk/client-sns';

const sns = new SNSClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create a standard topic
const { TopicArn } = await sns.send(new CreateTopicCommand({
  Name: 'user-events',
  Attributes: {},
}));

// Subscribe SQS queue for fan-out
const { SubscriptionArn } = await sns.send(new SubscribeCommand({
  TopicArn,
  Protocol: 'sqs',
  Endpoint: 'arn:aws:sqs:us-east-1:000000000000:user-events-queue',
}));

// Subscribe Lambda for processing
await sns.send(new SubscribeCommand({
  TopicArn,
  Protocol: 'lambda',
  Endpoint: 'arn:aws:lambda:us-east-1:000000000000:function:process-user-event',
}));

// Add a filter policy (stored but not enforced in AWSim)
await sns.send(new SetSubscriptionAttributesCommand({
  SubscriptionArn: SubscriptionArn!,
  AttributeName: 'FilterPolicy',
  AttributeValue: JSON.stringify({ eventType: ['signup', 'login'] }),
}));

// Publish with message attributes
const { MessageId } = await sns.send(new PublishCommand({
  TopicArn,
  Message: JSON.stringify({ userId: '123', action: 'signup', timestamp: Date.now() }),
  Subject: 'UserEvent',
  MessageAttributes: {
    eventType: { DataType: 'String', StringValue: 'signup' },
    userId: { DataType: 'String', StringValue: '123' },
  },
}));

console.log('Published message ID:', MessageId);

// Publish batch (up to 10 messages)
const { Successful, Failed } = await sns.send(new PublishBatchCommand({
  TopicArn,
  PublishBatchRequestEntries: [
    { Id: '1', Message: JSON.stringify({ userId: '1', action: 'view' }) },
    { Id: '2', Message: JSON.stringify({ userId: '2', action: 'purchase' }) },
    { Id: '3', Message: JSON.stringify({ userId: '3', action: 'logout' }) },
  ],
}));
console.log('Published:', Successful?.length, 'Failed:', Failed?.length);
```

## CLI Example

```bash
# Create topic
aws --endpoint-url http://localhost:4566 sns create-topic --name my-topic

# Subscribe SQS queue
aws --endpoint-url http://localhost:4566 sns subscribe \
  --topic-arn arn:aws:sns:us-east-1:000000000000:my-topic \
  --protocol sqs \
  --notification-endpoint arn:aws:sqs:us-east-1:000000000000:my-queue

# Publish message
aws --endpoint-url http://localhost:4566 sns publish \
  --topic-arn arn:aws:sns:us-east-1:000000000000:my-topic \
  --message '{"event":"order_placed","orderId":"abc123"}' \
  --subject "OrderEvent" \
  --message-attributes '{"eventType":{"DataType":"String","StringValue":"order_placed"}}'

# List subscriptions by topic
aws --endpoint-url http://localhost:4566 sns list-subscriptions-by-topic \
  --topic-arn arn:aws:sns:us-east-1:000000000000:my-topic
```

## Fan-out

When a message is published, AWSim **immediately delivers** it to:
- All subscribed SQS queues (message appears in the queue)
- All subscribed Lambda functions (function is invoked synchronously)

HTTP/HTTPS and email endpoints are registered but not called.

See [Cross-Service Integrations](/guide/integrations#sns-to-sqs-fan-out).

## Behavior Notes

- SNS is persistent: topics and subscriptions survive AWSim restarts.
- HTTP/HTTPS subscription confirmation handshake is not performed — subscriptions are auto-confirmed immediately.
- Subscription filter policies are stored but not enforced — all messages are delivered to all subscribers regardless of attributes.
- Message attributes are passed through to SQS/Lambda but attribute-based filtering is not applied.
- FIFO topics (`Name.fifo`) are accepted but message ordering and deduplication are not strictly enforced.
- SMS opt-out state (`CheckIfPhoneNumberIsOptedOut`, `ListPhoneNumbersOptedOut`) is stored in memory and always returns no opted-out numbers by default.
- SMS attributes (`GetSMSAttributes`, `SetSMSAttributes`) are accepted and stored but no actual SMS messages are delivered in AWSim.
