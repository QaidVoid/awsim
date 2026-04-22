# SQS

**Protocol:** JSON (`X-Amz-Target: AmazonSQS.*`)  
**Signing name:** `sqs`  
**Persistent:** Yes

## Implemented Operations

| Operation | Description |
|-----------|-------------|
| `CreateQueue` | Create a queue (standard or FIFO) |
| `DeleteQueue` | Delete a queue |
| `ListQueues` | List all queues |
| `GetQueueUrl` | Get the URL of a queue by name |
| `GetQueueAttributes` | Get queue attributes |
| `SetQueueAttributes` | Set queue attributes |
| `SendMessage` | Send a message to a queue |
| `SendMessageBatch` | Send up to 10 messages in one call |
| `ReceiveMessage` | Receive up to 10 messages |
| `DeleteMessage` | Delete a message by receipt handle |
| `DeleteMessageBatch` | Batch delete messages |
| `ChangeMessageVisibility` | Change the visibility timeout of an in-flight message |
| `PurgeQueue` | Delete all messages from a queue |
| `TagQueue` | Add tags to a queue |
| `UntagQueue` | Remove tags from a queue |
| `ListQueueTags` | List queue tags |

## SDK Example

```typescript
import { SQSClient, CreateQueueCommand, SendMessageCommand, ReceiveMessageCommand, DeleteMessageCommand } from "@aws-sdk/client-sqs";

const sqs = new SQSClient({
  region: "us-east-1",
  endpoint: "http://localhost:4566",
  credentials: { accessKeyId: "test", secretAccessKey: "test" },
});

// Create queue
const { QueueUrl } = await sqs.send(new CreateQueueCommand({ QueueName: "my-queue" }));

// Send
await sqs.send(new SendMessageCommand({
  QueueUrl,
  MessageBody: JSON.stringify({ action: "process" }),
}));

// Receive
const { Messages } = await sqs.send(new ReceiveMessageCommand({
  QueueUrl,
  MaxNumberOfMessages: 10,
  WaitTimeSeconds: 0,
}));

// Delete after processing
for (const msg of Messages ?? []) {
  await sqs.send(new DeleteMessageCommand({
    QueueUrl,
    ReceiptHandle: msg.ReceiptHandle!,
  }));
}
```

## CLI Example

```bash
# Create queue
aws --endpoint-url http://localhost:4566 sqs create-queue --queue-name my-queue

# Send message
aws --endpoint-url http://localhost:4566 sqs send-message \
  --queue-url http://localhost:4566/000000000000/my-queue \
  --message-body "hello"

# Receive messages
aws --endpoint-url http://localhost:4566 sqs receive-message \
  --queue-url http://localhost:4566/000000000000/my-queue
```

## Queue URL Format

AWSim queue URLs follow the pattern:

```
http://localhost:4566/{account_id}/{queue_name}
```

Default: `http://localhost:4566/000000000000/my-queue`

## Lambda Polling

SQS queues can trigger Lambda functions via event source mappings. AWSim polls queues every **2 seconds**. See [Cross-Service Integrations](/guide/integrations#sqs-to-lambda-polling).

## Dead Letter Queues

Redrive policies (`RedrivePolicy` attribute) are stored but not enforced — messages that fail processing are not automatically moved to a DLQ.

## Known Limitations

- Long polling (`WaitTimeSeconds > 0`) is accepted but returns immediately without waiting.
- Message deduplication for FIFO queues is not enforced.
- Visibility timeout countdown is tracked but may not be perfectly precise.
