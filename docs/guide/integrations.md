# Cross-Service Integrations

AWSim wires services together via an internal async event bus. These integrations mirror how AWS services interact in production.

## SNS to SQS Fan-out

When a message is published to an SNS topic that has SQS subscriptions, AWSim delivers the message to each subscribed queue immediately.

```bash
# Create topic
aws --endpoint-url http://localhost:4566 sns create-topic --name my-topic

# Create queue
aws --endpoint-url http://localhost:4566 sqs create-queue --queue-name my-queue

# Subscribe queue to topic
aws --endpoint-url http://localhost:4566 sns subscribe \
  --topic-arn arn:aws:sns:us-east-1:000000000000:my-topic \
  --protocol sqs \
  --notification-endpoint arn:aws:sqs:us-east-1:000000000000:my-queue

# Publish — message appears in the queue
aws --endpoint-url http://localhost:4566 sns publish \
  --topic-arn arn:aws:sns:us-east-1:000000000000:my-topic \
  --message "hello"
```

## SQS to Lambda (Polling)

When a Lambda function has an SQS event source mapping, AWSim polls the queue every **2 seconds** and invokes the function with a batch of messages.

```bash
aws --endpoint-url http://localhost:4566 lambda create-event-source-mapping \
  --function-name my-function \
  --event-source-arn arn:aws:sqs:us-east-1:000000000000:my-queue \
  --batch-size 10
```

Messages are deleted from the queue after a successful invocation.

## Kinesis to Lambda (Polling)

Similar to SQS, but AWSim polls Kinesis shards every **5 seconds**. Each shard is polled independently.

```bash
aws --endpoint-url http://localhost:4566 lambda create-event-source-mapping \
  --function-name my-function \
  --event-source-arn arn:aws:kinesis:us-east-1:000000000000:stream/my-stream \
  --starting-position TRIM_HORIZON
```

## DynamoDB Streams to Lambda

When a DynamoDB table has streams enabled and a Lambda event source mapping is configured, AWSim delivers stream records (INSERT, MODIFY, REMOVE) to the function.

```bash
# Enable streams on the table
aws --endpoint-url http://localhost:4566 dynamodb update-table \
  --table-name my-table \
  --stream-specification StreamEnabled=true,StreamViewType=NEW_AND_OLD_IMAGES

# Create the event source mapping
aws --endpoint-url http://localhost:4566 lambda create-event-source-mapping \
  --function-name my-function \
  --event-source-arn arn:aws:dynamodb:us-east-1:000000000000:table/my-table/stream/... \
  --starting-position TRIM_HORIZON
```

## S3 Event Notifications

S3 can send event notifications for `ObjectCreated` and `ObjectRemoved` events to:

- **SNS topics**
- **SQS queues**
- **Lambda functions**

Configure via `PutBucketNotificationConfiguration`:

```bash
aws --endpoint-url http://localhost:4566 s3api put-bucket-notification-configuration \
  --bucket my-bucket \
  --notification-configuration '{
    "QueueConfigurations": [{
      "QueueArn": "arn:aws:sqs:us-east-1:000000000000:my-queue",
      "Events": ["s3:ObjectCreated:*"]
    }]
  }'
```

Supported events: `s3:ObjectCreated:Put`, `s3:ObjectCreated:Copy`, `s3:ObjectRemoved:Delete`, and wildcard patterns like `s3:ObjectCreated:*`.

## EventBridge to Lambda / SQS / SNS

EventBridge rules can route events to Lambda functions, SQS queues, or SNS topics. Rules are evaluated when `PutEvents` is called.

```bash
# Create a rule
aws --endpoint-url http://localhost:4566 events put-rule \
  --name my-rule \
  --event-pattern '{"source": ["my-app"]}' \
  --state ENABLED

# Add a Lambda target
aws --endpoint-url http://localhost:4566 events put-targets \
  --rule my-rule \
  --targets 'Id=1,Arn=arn:aws:lambda:us-east-1:000000000000:function:my-function'

# Send an event
aws --endpoint-url http://localhost:4566 events put-events \
  --entries 'Source=my-app,DetailType=MyEvent,Detail={"key":"value"}'
```

## CloudFormation Resource Provisioning

AWSim's CloudFormation implementation actually provisions resources by calling the appropriate service APIs internally. Supported resource types include S3 buckets, DynamoDB tables, SQS queues, SNS topics, Lambda functions, IAM roles, and more.

```bash
aws --endpoint-url http://localhost:4566 cloudformation create-stack \
  --stack-name my-stack \
  --template-body file://template.yaml
```

## Cognito Lambda Triggers

Cognito user pools support Lambda triggers for authentication flow customization:

- `PreSignUp`
- `PostConfirmation`
- `PreAuthentication`
- `PostAuthentication`
- `PreTokenGeneration`
- `CustomMessage`

Set a trigger in `CreateUserPool` or `UpdateUserPool` via the `LambdaConfig` field.
