# SNS

**Protocol:** JSON (`X-Amz-Target: AmazonSimpleNotificationService.*`)  
**Signing name:** `sns`  
**Persistent:** Yes

## Implemented Operations

### Topics

| Operation | Description |
|-----------|-------------|
| `CreateTopic` | Create a topic (standard or FIFO) |
| `DeleteTopic` | Delete a topic and all its subscriptions |
| `ListTopics` | List all topics |
| `GetTopicAttributes` | Get topic attributes |
| `SetTopicAttributes` | Set topic attributes |

### Subscriptions

| Operation | Description |
|-----------|-------------|
| `Subscribe` | Subscribe an endpoint to a topic |
| `Unsubscribe` | Remove a subscription |
| `ListSubscriptions` | List all subscriptions |
| `ListSubscriptionsByTopic` | List subscriptions for a specific topic |
| `GetSubscriptionAttributes` | Get subscription attributes |
| `SetSubscriptionAttributes` | Set subscription attributes |
| `ConfirmSubscription` | Confirm a pending subscription |

### Publishing

| Operation | Description |
|-----------|-------------|
| `Publish` | Publish a message to a topic |
| `PublishBatch` | Publish up to 10 messages in one call |

### Tags

| Operation | Description |
|-----------|-------------|
| `TagResource` | Add tags to a topic |
| `UntagResource` | Remove tags from a topic |
| `ListTagsForResource` | List topic tags |

## Supported Subscription Protocols

| Protocol | Description |
|----------|-------------|
| `sqs` | Delivers to an SQS queue |
| `lambda` | Invokes a Lambda function |
| `http` / `https` | Delivers to an HTTP endpoint (not enforced in local mode) |

## SDK Example

```typescript
import { SNSClient, CreateTopicCommand, SubscribeCommand, PublishCommand } from "@aws-sdk/client-sns";

const sns = new SNSClient({
  region: "us-east-1",
  endpoint: "http://localhost:4566",
  credentials: { accessKeyId: "test", secretAccessKey: "test" },
});

// Create topic
const { TopicArn } = await sns.send(new CreateTopicCommand({ Name: "my-topic" }));

// Subscribe an SQS queue
await sns.send(new SubscribeCommand({
  TopicArn,
  Protocol: "sqs",
  Endpoint: "arn:aws:sqs:us-east-1:000000000000:my-queue",
}));

// Publish
await sns.send(new PublishCommand({
  TopicArn,
  Message: JSON.stringify({ event: "user_created", userId: "123" }),
  Subject: "UserEvent",
}));
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
  --message "hello world"
```

## Fan-out

When a message is published, AWSim immediately delivers it to all subscribed SQS queues and Lambda functions. See [Cross-Service Integrations](/guide/integrations#sns-to-sqs-fan-out).

## Known Limitations

- HTTP/HTTPS subscription confirmation handshake is not performed — subscriptions are auto-confirmed.
- Subscription filter policies are stored but not enforced.
- Message attributes are passed through but attribute-based filtering is not applied.
