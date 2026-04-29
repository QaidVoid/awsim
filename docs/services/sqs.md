# SQS

Amazon Simple Queue Service for decoupled message queuing between microservices, distributed systems, and serverless applications.

**Protocol:** AwsJson1_0 (`X-Amz-Target: AmazonSQS.*`)
**Signing name:** `sqs`
**Persistent:** Yes

## Quick Start

Create a queue, send a message, receive it, and delete it:

```bash
# Create a queue
QUEUE_URL=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.0" \
  -H "X-Amz-Target: AmazonSQS.CreateQueue" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/sqs/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"QueueName":"my-queue"}' \
  | jq -r '.QueueUrl')

echo "Queue URL: $QUEUE_URL"

# Send a message
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.0" \
  -H "X-Amz-Target: AmazonSQS.SendMessage" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/sqs/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"QueueUrl\":\"$QUEUE_URL\",\"MessageBody\":\"{\\\"event\\\":\\\"order_placed\\\",\\\"orderId\\\":\\\"123\\\"}\"}"

# Receive messages
RECEIPT=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.0" \
  -H "X-Amz-Target: AmazonSQS.ReceiveMessage" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/sqs/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"QueueUrl\":\"$QUEUE_URL\",\"MaxNumberOfMessages\":10}" \
  | jq -r '.Messages[0].ReceiptHandle')

# Delete the processed message
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.0" \
  -H "X-Amz-Target: AmazonSQS.DeleteMessage" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/sqs/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"QueueUrl\":\"$QUEUE_URL\",\"ReceiptHandle\":\"$RECEIPT\"}"
```

## Operations

| Operation | Description |
|-----------|-------------|
| `CreateQueue` | Create a standard or FIFO queue. Input: `QueueName` (FIFO queues must end in `.fifo`), `Attributes` (`{VisibilityTimeout, MessageRetentionPeriod, DelaySeconds, MaximumMessageSize, ReceiveMessageWaitTimeSeconds, RedrivePolicy}`). Returns: `QueueUrl` |
| `DeleteQueue` | Delete a queue and all its messages. Input: `QueueUrl` |
| `ListQueues` | List all queue URLs. Input: optional `QueueNamePrefix`, `MaxResults`, `NextToken` |
| `GetQueueUrl` | Get the URL of a queue by name. Input: `QueueName`. Returns: `QueueUrl` |
| `GetQueueAttributes` | Get queue configuration. Input: `QueueUrl`, `AttributeNames` (list; use `["All"]` for all). Returns map of attribute name to value |
| `SetQueueAttributes` | Set queue attributes. Input: `QueueUrl`, `Attributes` map. Use to change visibility timeout, set redrive policy, etc. |
| `SendMessage` | Send a message. Input: `QueueUrl`, `MessageBody` (string, max 256 KB), optional `DelaySeconds` (0–900), `MessageAttributes` (`{key: {DataType, StringValue}}`), `MessageGroupId` (FIFO), `MessageDeduplicationId` (FIFO). Returns: `MessageId`, `MD5OfMessageBody` |
| `SendMessageBatch` | Send up to 10 messages in one call. Input: `QueueUrl`, `Entries` (list of `{Id, MessageBody, DelaySeconds, MessageAttributes}`). Returns: `Successful`, `Failed` |
| `ReceiveMessage` | Receive up to 10 messages. Input: `QueueUrl`, `MaxNumberOfMessages` (1–10), `VisibilityTimeout` (override for this receive), `WaitTimeSeconds` (0–20 for long polling), `MessageAttributeNames`. Returns: `Messages` list with `Body`, `MessageId`, `ReceiptHandle`, `Attributes` |
| `DeleteMessage` | Delete a processed message. Input: `QueueUrl`, `ReceiptHandle` (from ReceiveMessage). Must be called after processing to prevent re-delivery |
| `DeleteMessageBatch` | Batch delete messages. Input: `QueueUrl`, `Entries` (list of `{Id, ReceiptHandle}`). Returns: `Successful`, `Failed` |
| `ChangeMessageVisibility` | Extend or reset the visibility timeout of an in-flight message. Input: `QueueUrl`, `ReceiptHandle`, `VisibilityTimeout` (0 = make immediately visible; max 43200) |
| `ChangeMessageVisibilityBatch` | Change the visibility timeout of up to 10 in-flight messages in one call. Input: `QueueUrl`, `Entries` (list of `{Id, ReceiptHandle, VisibilityTimeout}`). Returns: `Successful`, `Failed` |
| `PurgeQueue` | Delete all messages from a queue instantly. Input: `QueueUrl`. Useful for test cleanup |
| `TagQueue` | Add tags to a queue. Input: `QueueUrl`, `Tags` map |
| `UntagQueue` | Remove tags from a queue |
| `ListQueueTags` | List queue tags. Input: `QueueUrl` |
| `ListDeadLetterSourceQueues` | List queues that have a given queue configured as their dead-letter queue. Input: `QueueUrl`. Returns: `queueUrls` list |

## Queue URL Format

AWSim queue URLs follow the pattern:

```
http://localhost:4566/{account_id}/{queue_name}
```

Default: `http://localhost:4566/000000000000/my-queue`
FIFO: `http://localhost:4566/000000000000/my-queue.fifo`

## SDK Example

```typescript
import {
  SQSClient,
  CreateQueueCommand,
  SendMessageCommand,
  SendMessageBatchCommand,
  ReceiveMessageCommand,
  DeleteMessageCommand,
  PurgeQueueCommand,
  GetQueueAttributesCommand,
} from '@aws-sdk/client-sqs';

const sqs = new SQSClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create a standard queue
const { QueueUrl } = await sqs.send(new CreateQueueCommand({
  QueueName: 'order-processing',
  Attributes: {
    VisibilityTimeout: '30',        // 30 seconds in-flight
    MessageRetentionPeriod: '86400', // 1 day
  },
}));

// Send a message with attributes
await sqs.send(new SendMessageCommand({
  QueueUrl,
  MessageBody: JSON.stringify({ orderId: 'ord-123', userId: 'usr-456', amount: 99.99 }),
  MessageAttributes: {
    orderType: { DataType: 'String', StringValue: 'STANDARD' },
    priority: { DataType: 'Number', StringValue: '1' },
  },
}));

// Send a batch
await sqs.send(new SendMessageBatchCommand({
  QueueUrl,
  Entries: [
    { Id: '1', MessageBody: JSON.stringify({ orderId: 'ord-124' }) },
    { Id: '2', MessageBody: JSON.stringify({ orderId: 'ord-125' }), DelaySeconds: 5 },
    { Id: '3', MessageBody: JSON.stringify({ orderId: 'ord-126' }) },
  ],
}));

// Receive and process messages
const { Messages } = await sqs.send(new ReceiveMessageCommand({
  QueueUrl,
  MaxNumberOfMessages: 10,
  WaitTimeSeconds: 0,
  MessageAttributeNames: ['All'],
  AttributeNames: ['All'],
}));

for (const message of Messages ?? []) {
  const order = JSON.parse(message.Body!);
  console.log('Processing order:', order.orderId);

  // Process the message...

  // Delete after successful processing
  await sqs.send(new DeleteMessageCommand({
    QueueUrl,
    ReceiptHandle: message.ReceiptHandle!,
  }));
}

// Get queue stats
const { Attributes } = await sqs.send(new GetQueueAttributesCommand({
  QueueUrl,
  AttributeNames: ['All'],
}));
console.log('Messages available:', Attributes?.ApproximateNumberOfMessages);
console.log('In flight:', Attributes?.ApproximateNumberOfMessagesNotVisible);

// Clean up (test utility)
await sqs.send(new PurgeQueueCommand({ QueueUrl }));
```

## CLI Example

```bash
# Create queue
aws --endpoint-url http://localhost:4566 sqs create-queue --queue-name my-queue

# Create FIFO queue
aws --endpoint-url http://localhost:4566 sqs create-queue \
  --queue-name my-queue.fifo \
  --attributes FifoQueue=true,ContentBasedDeduplication=true

# Send message
aws --endpoint-url http://localhost:4566 sqs send-message \
  --queue-url http://localhost:4566/000000000000/my-queue \
  --message-body '{"event":"test"}'

# Receive messages
aws --endpoint-url http://localhost:4566 sqs receive-message \
  --queue-url http://localhost:4566/000000000000/my-queue \
  --max-number-of-messages 10

# Delete message
aws --endpoint-url http://localhost:4566 sqs delete-message \
  --queue-url http://localhost:4566/000000000000/my-queue \
  --receipt-handle RECEIPT_HANDLE_FROM_RECEIVE

# Purge queue (delete all messages)
aws --endpoint-url http://localhost:4566 sqs purge-queue \
  --queue-url http://localhost:4566/000000000000/my-queue
```

## Lambda Polling

SQS queues can trigger Lambda functions via event source mappings. AWSim polls queues every **2 seconds**. When messages are found, the function is invoked with a batch of records. See [Cross-Service Integrations](/guide/integrations#sqs-to-lambda-polling).

## Dead Letter Queues

Configure a redrive policy to route failed messages to a DLQ:

```bash
# Create DLQ
aws --endpoint-url http://localhost:4566 sqs create-queue --queue-name my-dlq

# Get DLQ ARN
DLQ_ARN=$(aws --endpoint-url http://localhost:4566 sqs get-queue-attributes \
  --queue-url http://localhost:4566/000000000000/my-dlq \
  --attribute-names QueueArn | jq -r '.Attributes.QueueArn')

# Set redrive policy on main queue
aws --endpoint-url http://localhost:4566 sqs set-queue-attributes \
  --queue-url http://localhost:4566/000000000000/my-queue \
  --attributes "RedrivePolicy={\"deadLetterTargetArn\":\"$DLQ_ARN\",\"maxReceiveCount\":3}"
```

## Behavior Notes

- SQS is persistent: queues and messages survive AWSim restarts.
- When `--data-dir` is set, message bodies are written to `{data_dir}/sqs/{queue}/{message_id}` on `SendMessage`/`SendMessageBatch`. `DeleteMessage`, `PurgeQueue`, and `DeleteQueue` remove the corresponding files. See [Persistence: SQS message bodies](../guide/persistence.md#sqs-message-bodies) for details.
- Long polling (`WaitTimeSeconds > 0`) is accepted but returns immediately without actually waiting.
- Visibility timeout countdown is tracked but may not be perfectly precise at millisecond granularity.
- `RedrivePolicy` (dead-letter queue) is stored but messages that fail processing are not automatically moved to the DLQ.
- `ApproximateNumberOfMessages` in `GetQueueAttributes` returns the accurate current count.

### Attribute and message-attribute filtering

`ReceiveMessage` honors the SQS spec for the `AttributeNames` and `MessageAttributeNames` parameters: omitting the field returns **no** attributes; only an explicit `["All"]` returns all attributes. Earlier versions treated empty as "all", which masked client bugs that depended on attribute filtering.

### MessageAttributes BinaryValue

`SendMessage` decodes the wire-format base64 `BinaryValue` and stores it alongside `StringValue`; `ReceiveMessage` re-encodes it on the way out. Both fields can coexist on a single attribute, matching the SQS spec.

### FIFO-only fields rejected on standard queues

`SendMessage` / `SendMessageBatch` reject `MessageGroupId` or `MessageDeduplicationId` on non-FIFO queues with `InvalidParameterValue`. Earlier versions silently dropped these fields, which let test code that relied on FIFO semantics ship green against a standard queue.
