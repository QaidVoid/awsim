/**
 * Typed SQS API client.
 *
 * Wraps the LocalStack-style AWS JSON 1.0 SQS API with strong TypeScript
 * types so component code never has to touch fetch headers or raw payloads.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const TARGET_PREFIX = "AmazonSQS";
const SERVICE = "sqs";

// ---------- Types ----------

export interface Queue {
  url: string;
  name: string;
}

export interface QueueAttributes {
  approximateNumberOfMessages: number;
  approximateNumberOfMessagesNotVisible: number;
  approximateNumberOfMessagesDelayed: number;
  createdTimestamp: string;
  lastModifiedTimestamp: string;
  visibilityTimeout: number;
  messageRetentionPeriod: number;
  delaySeconds: number;
  receiveMessageWaitTimeSeconds: number;
  maximumMessageSize: number;
  isFifo: boolean;
  contentBasedDeduplication: boolean;
  arn: string;
  redrivePolicy: RedrivePolicy | null;
  raw: Record<string, string>;
}

export interface RedrivePolicy {
  deadLetterTargetArn: string;
  maxReceiveCount: number;
}

export interface Message {
  messageId: string;
  receiptHandle: string;
  body: string;
  md5OfBody: string;
  attributes: Record<string, string>;
  messageAttributes: Record<string, MessageAttribute>;
}

export interface MessageAttribute {
  dataType: string;
  stringValue?: string;
  binaryValue?: string;
}

export interface SendMessageInput {
  queueUrl: string;
  body: string;
  delaySeconds?: number;
  messageGroupId?: string;
  messageDeduplicationId?: string;
  messageAttributes?: Record<string, MessageAttribute>;
}

export interface CreateQueueInput {
  name: string;
  fifo?: boolean;
  contentBasedDeduplication?: boolean;
  visibilityTimeout?: number;
  messageRetentionPeriod?: number;
  delaySeconds?: number;
  receiveMessageWaitTimeSeconds?: number;
}

// ---------- Internal request helper ----------

async function request<T>(
  action: string,
  params: Record<string, unknown> = {},
): Promise<T> {
  const res = await loggedFetch(SERVICE, action, "POST", ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.0",
      "X-Amz-Target": `${TARGET_PREFIX}.${action}`,
      Authorization: authHeader(SERVICE),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(params),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`SQS ${action} failed (HTTP ${res.status}): ${text}`);
  }
  const text = await res.text();
  return (text ? JSON.parse(text) : {}) as T;
}

// ---------- Operations ----------

export async function listQueues(): Promise<Queue[]> {
  const data = await request<{ QueueUrls?: string[] }>("ListQueues", {});
  const urls = data.QueueUrls ?? [];
  return urls.map((url) => ({
    url,
    name: url.split("/").pop() ?? url,
  }));
}

function parseAttrs(raw: Record<string, string>): QueueAttributes {
  const num = (k: string, dflt = 0) => parseInt(raw[k] ?? String(dflt), 10);
  let redrive: RedrivePolicy | null = null;
  const rdRaw = raw["RedrivePolicy"];
  if (rdRaw) {
    try {
      const parsed = JSON.parse(rdRaw) as {
        deadLetterTargetArn?: string;
        maxReceiveCount?: number | string;
      };
      redrive = {
        deadLetterTargetArn: parsed.deadLetterTargetArn ?? "",
        maxReceiveCount:
          typeof parsed.maxReceiveCount === "string"
            ? parseInt(parsed.maxReceiveCount, 10)
            : (parsed.maxReceiveCount ?? 0),
      };
    } catch {
      redrive = null;
    }
  }
  return {
    approximateNumberOfMessages: num("ApproximateNumberOfMessages"),
    approximateNumberOfMessagesNotVisible: num(
      "ApproximateNumberOfMessagesNotVisible",
    ),
    approximateNumberOfMessagesDelayed: num(
      "ApproximateNumberOfMessagesDelayed",
    ),
    createdTimestamp: raw["CreatedTimestamp"] ?? "",
    lastModifiedTimestamp: raw["LastModifiedTimestamp"] ?? "",
    visibilityTimeout: num("VisibilityTimeout", 30),
    messageRetentionPeriod: num("MessageRetentionPeriod", 345600),
    delaySeconds: num("DelaySeconds"),
    receiveMessageWaitTimeSeconds: num("ReceiveMessageWaitTimeSeconds"),
    maximumMessageSize: num("MaximumMessageSize", 262144),
    isFifo: raw["FifoQueue"] === "true",
    contentBasedDeduplication: raw["ContentBasedDeduplication"] === "true",
    arn: raw["QueueArn"] ?? "",
    redrivePolicy: redrive,
    raw,
  };
}

export async function getQueueAttributes(
  queueUrl: string,
): Promise<QueueAttributes> {
  const data = await request<{ Attributes?: Record<string, string> }>(
    "GetQueueAttributes",
    { QueueUrl: queueUrl, AttributeNames: ["All"] },
  );
  return parseAttrs(data.Attributes ?? {});
}

export async function setQueueAttributes(
  queueUrl: string,
  attributes: Record<string, string>,
): Promise<void> {
  await request("SetQueueAttributes", {
    QueueUrl: queueUrl,
    Attributes: attributes,
  });
}

export async function createQueue(
  input: CreateQueueInput,
): Promise<{ queueUrl: string }> {
  const attributes: Record<string, string> = {};
  const name = input.fifo
    ? input.name.endsWith(".fifo")
      ? input.name
      : `${input.name}.fifo`
    : input.name;
  if (input.fifo) attributes["FifoQueue"] = "true";
  if (input.contentBasedDeduplication)
    attributes["ContentBasedDeduplication"] = "true";
  if (input.visibilityTimeout !== undefined)
    attributes["VisibilityTimeout"] = String(input.visibilityTimeout);
  if (input.messageRetentionPeriod !== undefined)
    attributes["MessageRetentionPeriod"] = String(input.messageRetentionPeriod);
  if (input.delaySeconds !== undefined)
    attributes["DelaySeconds"] = String(input.delaySeconds);
  if (input.receiveMessageWaitTimeSeconds !== undefined)
    attributes["ReceiveMessageWaitTimeSeconds"] = String(
      input.receiveMessageWaitTimeSeconds,
    );
  const params: Record<string, unknown> = { QueueName: name };
  if (Object.keys(attributes).length > 0) params["Attributes"] = attributes;
  const data = await request<{ QueueUrl?: string }>("CreateQueue", params);
  return { queueUrl: data.QueueUrl ?? "" };
}

export async function deleteQueue(queueUrl: string): Promise<void> {
  await request("DeleteQueue", { QueueUrl: queueUrl });
}

export async function purgeQueue(queueUrl: string): Promise<void> {
  await request("PurgeQueue", { QueueUrl: queueUrl });
}

export async function sendMessage(input: SendMessageInput): Promise<{
  messageId: string;
  md5OfBody: string;
}> {
  const params: Record<string, unknown> = {
    QueueUrl: input.queueUrl,
    MessageBody: input.body,
  };
  if (input.delaySeconds !== undefined)
    params["DelaySeconds"] = input.delaySeconds;
  if (input.messageGroupId) params["MessageGroupId"] = input.messageGroupId;
  if (input.messageDeduplicationId)
    params["MessageDeduplicationId"] = input.messageDeduplicationId;
  if (input.messageAttributes && Object.keys(input.messageAttributes).length) {
    const attrs: Record<string, Record<string, string>> = {};
    for (const [k, v] of Object.entries(input.messageAttributes)) {
      const a: Record<string, string> = { DataType: v.dataType };
      if (v.stringValue !== undefined) a["StringValue"] = v.stringValue;
      if (v.binaryValue !== undefined) a["BinaryValue"] = v.binaryValue;
      attrs[k] = a;
    }
    params["MessageAttributes"] = attrs;
  }
  const data = await request<{ MessageId?: string; MD5OfMessageBody?: string }>(
    "SendMessage",
    params,
  );
  return {
    messageId: data.MessageId ?? "",
    md5OfBody: data.MD5OfMessageBody ?? "",
  };
}

export async function receiveMessages(
  queueUrl: string,
  maxMessages = 10,
  waitTimeSeconds = 0,
  visibilityTimeout?: number,
): Promise<Message[]> {
  const params: Record<string, unknown> = {
    QueueUrl: queueUrl,
    MaxNumberOfMessages: maxMessages,
    WaitTimeSeconds: waitTimeSeconds,
    AttributeNames: ["All"],
    MessageAttributeNames: ["All"],
  };
  if (visibilityTimeout !== undefined)
    params["VisibilityTimeout"] = visibilityTimeout;
  const data = await request<{
    Messages?: {
      MessageId: string;
      ReceiptHandle: string;
      Body: string;
      MD5OfBody?: string;
      Attributes?: Record<string, string>;
      MessageAttributes?: Record<
        string,
        {
          DataType: string;
          StringValue?: string;
          BinaryValue?: string;
        }
      >;
    }[];
  }>("ReceiveMessage", params);
  return (data.Messages ?? []).map((m) => ({
    messageId: m.MessageId,
    receiptHandle: m.ReceiptHandle,
    body: m.Body,
    md5OfBody: m.MD5OfBody ?? "",
    attributes: m.Attributes ?? {},
    messageAttributes: Object.fromEntries(
      Object.entries(m.MessageAttributes ?? {}).map(([k, v]) => [
        k,
        {
          dataType: v.DataType,
          stringValue: v.StringValue,
          binaryValue: v.BinaryValue,
        },
      ]),
    ),
  }));
}

export async function deleteMessage(
  queueUrl: string,
  receiptHandle: string,
): Promise<void> {
  await request("DeleteMessage", {
    QueueUrl: queueUrl,
    ReceiptHandle: receiptHandle,
  });
}

/**
 * Drain ALL messages currently visible on `sourceUrl` and re-send them to
 * `targetUrl`. Used for DLQ redrive in the UI.
 */
export async function redriveMessages(
  sourceUrl: string,
  targetUrl: string,
  batchLimit = 10,
): Promise<{ moved: number }> {
  let moved = 0;
  for (let i = 0; i < batchLimit; i++) {
    const msgs = await receiveMessages(sourceUrl, 10, 0);
    if (msgs.length === 0) break;
    for (const m of msgs) {
      await sendMessage({ queueUrl: targetUrl, body: m.body });
      await deleteMessage(sourceUrl, m.receiptHandle);
      moved++;
    }
  }
  return { moved };
}
