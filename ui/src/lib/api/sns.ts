/**
 * Typed SNS API client.
 *
 * Wraps the AWS Query API (XML responses) for SNS.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const SERVICE = "sns";
const VERSION = "2010-03-31";

// ---------- Types ----------

export interface Topic {
  arn: string;
  name: string;
}

export interface TopicAttributes {
  arn: string;
  displayName: string;
  subscriptionsConfirmed: number;
  subscriptionsPending: number;
  subscriptionsDeleted: number;
  isFifo: boolean;
  contentBasedDeduplication: boolean;
  policy: string;
  raw: Record<string, string>;
}

export interface Subscription {
  arn: string;
  protocol: string;
  endpoint: string;
  topicArn: string;
  owner: string;
}

export interface PublishInput {
  topicArn: string;
  message: string;
  subject?: string;
  messageStructure?: "json";
  messageGroupId?: string;
  messageDeduplicationId?: string;
}

// ---------- Internal request ----------

async function request(
  action: string,
  params: Record<string, string> = {},
): Promise<string> {
  const body = new URLSearchParams({
    Action: action,
    Version: VERSION,
    ...params,
  });
  const res = await loggedFetch(SERVICE, action, "POST", ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-www-form-urlencoded",
      Authorization: authHeader(SERVICE),
      "X-Amz-Date": amzDate(),
    },
    body: body.toString(),
  });
  const text = await res.text();
  if (!res.ok) {
    throw new Error(`SNS ${action} failed (HTTP ${res.status}): ${text}`);
  }
  return text;
}

function xmlValue(xml: string, tag: string): string {
  const match = new RegExp(`<${tag}>([\\s\\S]*?)<\\/${tag}>`).exec(xml);
  return match ? match[1] : "";
}

function xmlMembers(xml: string, listTag: string, fields: string[]) {
  const out: Record<string, string>[] = [];
  const listRe = new RegExp(`<${listTag}>([\\s\\S]*?)<\\/${listTag}>`);
  const list = listRe.exec(xml);
  if (!list) return out;
  const memberRe = /<member>([\s\S]*?)<\/member>/g;
  let m: RegExpExecArray | null;
  while ((m = memberRe.exec(list[1])) !== null) {
    const row: Record<string, string> = {};
    for (const f of fields) row[f] = xmlValue(m[1], f);
    out.push(row);
  }
  return out;
}

function xmlEntries(xml: string, attrTag: string): Record<string, string> {
  const map: Record<string, string> = {};
  const attrRe = new RegExp(`<${attrTag}>([\\s\\S]*?)<\\/${attrTag}>`);
  const block = attrRe.exec(xml);
  if (!block) return map;
  const entryRe = /<entry>([\s\S]*?)<\/entry>/g;
  let m: RegExpExecArray | null;
  while ((m = entryRe.exec(block[1])) !== null) {
    const key = xmlValue(m[1], "key");
    const value = xmlValue(m[1], "value");
    if (key) map[key] = value;
  }
  return map;
}

// ---------- Operations ----------

export async function listTopics(): Promise<Topic[]> {
  const xml = await request("ListTopics");
  const rows = xmlMembers(xml, "Topics", ["TopicArn"]);
  return rows.map((r) => {
    const arn = r["TopicArn"] ?? "";
    return { arn, name: arn.split(":").pop() ?? arn };
  });
}

export async function getTopicAttributes(
  topicArn: string,
): Promise<TopicAttributes> {
  const xml = await request("GetTopicAttributes", { TopicArn: topicArn });
  const raw = xmlEntries(xml, "Attributes");
  const num = (k: string) => parseInt(raw[k] ?? "0", 10);
  return {
    arn: raw["TopicArn"] ?? topicArn,
    displayName: raw["DisplayName"] ?? "",
    subscriptionsConfirmed: num("SubscriptionsConfirmed"),
    subscriptionsPending: num("SubscriptionsPending"),
    subscriptionsDeleted: num("SubscriptionsDeleted"),
    isFifo: raw["FifoTopic"] === "true",
    contentBasedDeduplication: raw["ContentBasedDeduplication"] === "true",
    policy: raw["Policy"] ?? "",
    raw,
  };
}

export async function createTopic(
  name: string,
  fifo = false,
): Promise<{ topicArn: string }> {
  const params: Record<string, string> = {
    Name: fifo ? (name.endsWith(".fifo") ? name : `${name}.fifo`) : name,
  };
  if (fifo) {
    params["Attributes.entry.1.key"] = "FifoTopic";
    params["Attributes.entry.1.value"] = "true";
  }
  const xml = await request("CreateTopic", params);
  return { topicArn: xmlValue(xml, "TopicArn") };
}

export async function deleteTopic(topicArn: string): Promise<void> {
  await request("DeleteTopic", { TopicArn: topicArn });
}

export async function listSubscriptionsByTopic(
  topicArn: string,
): Promise<Subscription[]> {
  const xml = await request("ListSubscriptionsByTopic", { TopicArn: topicArn });
  const rows = xmlMembers(xml, "Subscriptions", [
    "SubscriptionArn",
    "Protocol",
    "Endpoint",
    "TopicArn",
    "Owner",
  ]);
  return rows.map((r) => ({
    arn: r["SubscriptionArn"] ?? "",
    protocol: r["Protocol"] ?? "",
    endpoint: r["Endpoint"] ?? "",
    topicArn: r["TopicArn"] ?? topicArn,
    owner: r["Owner"] ?? "",
  }));
}

export async function subscribe(
  topicArn: string,
  protocol: string,
  endpoint: string,
): Promise<{ subscriptionArn: string }> {
  const xml = await request("Subscribe", {
    TopicArn: topicArn,
    Protocol: protocol,
    Endpoint: endpoint,
    ReturnSubscriptionArn: "true",
  });
  return { subscriptionArn: xmlValue(xml, "SubscriptionArn") };
}

export async function unsubscribe(subscriptionArn: string): Promise<void> {
  await request("Unsubscribe", { SubscriptionArn: subscriptionArn });
}

export async function publish(
  input: PublishInput,
): Promise<{ messageId: string }> {
  const params: Record<string, string> = {
    TopicArn: input.topicArn,
    Message: input.message,
  };
  if (input.subject) params["Subject"] = input.subject;
  if (input.messageStructure)
    params["MessageStructure"] = input.messageStructure;
  if (input.messageGroupId) params["MessageGroupId"] = input.messageGroupId;
  if (input.messageDeduplicationId)
    params["MessageDeduplicationId"] = input.messageDeduplicationId;
  const xml = await request("Publish", params);
  return { messageId: xmlValue(xml, "MessageId") };
}
